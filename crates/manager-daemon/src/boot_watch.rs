//! The login screen watch (ADR 0033, ticket 140).
//!
//! `better-greeter-watch.service` runs `better-manager-daemon --watch-login-screen`
//! once per boot. It waits for GDM to start, then asks logind whether GDM's
//! greeter, or any graphical session on seat0, is there. If none is there 30
//! seconds after GDM started, it records the evidence and restarts GDM, once,
//! then records whether the login screen came back.
//!
//! Restarting the display manager is the only change it makes. It never makes
//! one on a boot where a person's session exists, never twice on one boot, and
//! not at all when the person turned it off or GDM is not the display
//! manager. Everything it touches is behind [`WatchHost`], so every rule here
//! is tested without logind, systemd, or a journal.

use std::path::Path;
use std::time::Duration;

use manager_ipc::boot_watch::{
    Event, Evidence, LastBoot, LastBootOutcome, MAX_SECTION_BYTES, RECORD_VERSION, Records,
    RestartOutcome, SessionSeen, Setting, is_boot_id, tail,
};

/// The display manager units this watch knows. Anything else is left alone.
pub const GDM_UNITS: [&str; 2] = ["gdm.service", "gdm3.service"];

/// How a unit looked to systemd.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitState {
    /// The unit `display-manager.service` resolves to.
    pub id: String,
    /// `loaded`, `not-found`, ...
    pub load_state: String,
    /// `active`, `activating`, `failed`, ...
    pub active_state: String,
    /// When it last became active, on the monotonic clock.
    pub active_since: Option<Duration>,
}

/// Everything the watch reads or changes on the machine.
pub trait WatchHost {
    /// This boot's id, dashes removed.
    fn boot_id(&self) -> Option<String>;
    fn display_manager(&self) -> Result<UnitState, String>;
    /// Time since boot.
    fn monotonic(&self) -> Duration;
    /// Unix seconds.
    fn wall_clock(&self) -> u64;
    fn sleep(&self, duration: Duration);
    fn sessions(&self) -> Result<Vec<SessionSeen>, String>;
    /// Reads the evidence for `unit`; `sessions` is filled in by the caller.
    fn evidence(&self, unit: &str) -> Evidence;
    /// Asks systemd to restart `unit`, without waiting for the job.
    fn restart(&self, unit: &str) -> Result<(), String>;
    /// The text of every GDM configuration file there is.
    fn gdm_config(&self) -> Vec<String>;
    /// Whether a `gdm-session-worker` runs in `unit`'s cgroup: GDM is in the
    /// middle of a login (a greeter starting, or a password being checked).
    fn login_worker_running(&self, unit: &str) -> bool;
}

/// GDM's configuration files, in the order GDM reads them.
pub const GDM_CONFIG_FILES: [&str; 4] = [
    "/etc/gdm3/custom.conf",
    "/etc/gdm3/daemon.conf",
    "/etc/gdm/custom.conf",
    "/etc/gdm/daemon.conf",
];

/// What GDM's configuration says about logging someone in without the login
/// screen, read from every file's `[daemon]` sections (a section may repeat;
/// a header may carry spaces). Boolean keys accept `true` and `1`, as
/// GKeyFile does; anything else is off.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GdmLogin {
    /// `AutomaticLoginEnable`: no login screen at all, so the watch never
    /// restarts GDM.
    pub automatic: bool,
    /// `TimedLoginEnable`, with `TimedLoginDelay` (GDM's default 30 s): the
    /// login screen shows and logs someone in after the delay.
    pub timed_delay: Option<Duration>,
}

/// GDM's own default `TimedLoginDelay`.
pub const GDM_TIMED_LOGIN_DELAY: Duration = Duration::from_secs(30);

pub fn gdm_login(files: &[String]) -> GdmLogin {
    let mut automatic = false;
    let mut timed = false;
    let mut delay = None;
    for text in files {
        let mut in_daemon = false;
        for line in text.lines() {
            let line = line.trim();
            if let Some(header) = line.strip_prefix('[') {
                let name = header.trim_end().strip_suffix(']').unwrap_or(header);
                in_daemon = name.trim().eq_ignore_ascii_case("daemon");
                continue;
            }
            if !in_daemon || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            let on = matches!(value.to_ascii_lowercase().as_str(), "true" | "1");
            match key.trim() {
                "AutomaticLoginEnable" => automatic |= on,
                "TimedLoginEnable" => timed |= on,
                "TimedLoginDelay" => delay = value.parse::<u64>().ok().map(Duration::from_secs),
                _ => {}
            }
        }
    }
    GdmLogin {
        automatic,
        timed_delay: timed.then(|| delay.unwrap_or(GDM_TIMED_LOGIN_DELAY)),
    }
}

/// The watch's timings. [`Timings::BOOT`] is what ships.
#[derive(Clone, Copy, Debug)]
pub struct Timings {
    /// How long GDM may take to become active before the watch gives up on
    /// this boot (a GDM that does not start is systemd's to report).
    pub gdm_start: Duration,
    /// How long after GDM started the login screen must be there. A normal
    /// boot on the owner's machine takes about 4 s (ticket 139).
    pub deadline: Duration,
    /// How long the watch waits for the login screen after the restart.
    pub after_restart: Duration,
    pub poll: Duration,
}

impl Timings {
    /// Together under the unit's `TimeoutStartSec=180`, evidence included.
    pub const BOOT: Timings = Timings {
        gdm_start: Duration::from_secs(60),
        deadline: Duration::from_secs(30),
        after_restart: Duration::from_secs(30),
        poll: Duration::from_secs(1),
    };
}

enum Seen {
    Person,
    LoginScreen,
    Nothing,
}

fn look(sessions: &[SessionSeen]) -> Seen {
    if sessions.iter().any(SessionSeen::is_person) {
        Seen::Person
    } else if sessions.iter().any(SessionSeen::is_login_screen) {
        Seen::LoginScreen
    } else {
        Seen::Nothing
    }
}

fn seconds(duration: Duration) -> u32 {
    u32::try_from(duration.as_secs()).unwrap_or(u32::MAX)
}

/// One boot's watch. Writes `last-boot.json` every time, and an event record
/// when the login screen was late. Returns what it wrote to `last-boot.json`.
pub fn run(host: &dyn WatchHost, records: &Records, timings: Timings) -> LastBootOutcome {
    let boot_id = host.boot_id().unwrap_or_default();
    let outcome = decide(host, records, timings, &boot_id);
    let last = LastBoot {
        version: RECORD_VERSION,
        boot_id,
        checked_at_unix_seconds: host.wall_clock(),
        outcome: outcome.clone(),
    };
    if let Err(error) = write_record(&records.last_boot_path(), &last, Readers::Everyone) {
        eprintln!("better-greeter-watch: could not write the last boot record: {error}");
    }
    outcome
}

fn decide(
    host: &dyn WatchHost,
    records: &Records,
    timings: Timings,
    boot_id: &str,
) -> LastBootOutcome {
    match records.setting() {
        Setting::Off => return LastBootOutcome::TurnedOff,
        Setting::Unreadable => return LastBootOutcome::SettingUnreadable,
        Setting::On | Setting::OnByDefault => {}
    }
    // Without a boot id "once per boot" cannot be kept, so nothing is done.
    if !is_boot_id(boot_id) {
        return LastBootOutcome::CouldNotWatch {
            details: "no boot id".to_string(),
        };
    }
    if records.handled(boot_id) {
        return LastBootOutcome::AlreadyHandled;
    }
    let gdm = gdm_login(&host.gdm_config());
    if gdm.automatic {
        return LastBootOutcome::AutomaticLogin;
    }

    // GDM is the display manager, and it is running.
    let started_waiting = host.monotonic();
    let (unit, active_since) = loop {
        let state = match host.display_manager() {
            Ok(state) => state,
            Err(details) => return LastBootOutcome::GdmNotStarted { state: details },
        };
        if state.load_state != "loaded" {
            return LastBootOutcome::NoGdm {
                display_manager: None,
            };
        }
        if !GDM_UNITS.contains(&state.id.as_str()) {
            return LastBootOutcome::NoGdm {
                display_manager: Some(state.id),
            };
        }
        if state.active_state == "active"
            && let Some(since) = state.active_since
        {
            break (state.id, since);
        }
        if host.monotonic().saturating_sub(started_waiting) >= timings.gdm_start {
            return LastBootOutcome::GdmNotStarted {
                state: state.active_state,
            };
        }
        host.sleep(timings.poll);
    };

    // The login screen, or a person, before the deadline.
    // Timed login shows the login screen too, so it is watched; the person
    // gets the whole delay, and the usual 30 s past it, before anything acts.
    let wait = match gdm.timed_delay {
        Some(delay) => timings.deadline.max(delay + timings.deadline),
        None => timings.deadline,
    };
    let deadline = active_since + wait;
    let sessions = loop {
        let sessions = match host.sessions() {
            Ok(sessions) => sessions,
            Err(details) => return LastBootOutcome::CouldNotWatch { details },
        };
        match look(&sessions) {
            Seen::Person => return LastBootOutcome::PersonPresent,
            Seen::LoginScreen => {
                return LastBootOutcome::LoginScreenAppeared {
                    after_seconds: seconds(host.monotonic().saturating_sub(active_since)),
                };
            }
            Seen::Nothing => {}
        }
        if host.monotonic() >= deadline {
            break sessions;
        }
        host.sleep(timings.poll);
    };

    // Late. The evidence is recorded first, and the record is on disk before
    // the restart is asked for: a watch killed after it still leaves the
    // boot marked, so it is never restarted twice.
    let mut evidence = host.evidence(&unit);
    evidence.sessions = sessions;
    let detected_at = host.wall_clock();
    let mut event = Event {
        version: RECORD_VERSION,
        boot_id: boot_id.to_string(),
        detected_at_unix_seconds: detected_at,
        display_manager: unit.clone(),
        waited_seconds: seconds(host.monotonic().saturating_sub(active_since)),
        evidence,
        restart: RestartOutcome::Pending,
    };
    let path = records.event_path(detected_at, boot_id);
    if let Err(error) = write_event(records, &path, &event) {
        // Without the record, "once per boot" would rest on nothing.
        eprintln!(
            "better-greeter-watch: could not write {}: {error}",
            path.display()
        );
        return LastBootOutcome::CouldNotWatch {
            details: format!("record not written: {error}"),
        };
    }
    prune(records, boot_id);

    // A person who logged in on a text console in the meantime keeps GDM, and
    // so does a login GDM is in the middle of.
    event.restart = match host.sessions().map(|sessions| look(&sessions)) {
        Ok(Seen::Person) => RestartOutcome::PersonAppeared,
        Ok(Seen::LoginScreen) => RestartOutcome::AppearedBeforeRestart,
        Err(details) => RestartOutcome::RestartFailed {
            details: format!("logind: {details}"),
        },
        Ok(Seen::Nothing) if host.login_worker_running(&unit) => RestartOutcome::LoginInProgress,
        Ok(Seen::Nothing) => match host.restart(&unit) {
            Err(details) => RestartOutcome::RestartFailed { details },
            Ok(()) => wait_after_restart(host, timings),
        },
    };
    if let Err(error) = write_event(records, &path, &event) {
        eprintln!(
            "better-greeter-watch: could not update {}: {error}",
            path.display()
        );
    }
    LastBootOutcome::Restarted
}

fn wait_after_restart(host: &dyn WatchHost, timings: Timings) -> RestartOutcome {
    let restarted = host.monotonic();
    loop {
        let elapsed = host.monotonic().saturating_sub(restarted);
        if let Ok(sessions) = host.sessions()
            && sessions
                .iter()
                .any(|s| s.is_login_screen() || s.is_person())
        {
            return RestartOutcome::Recovered {
                after_seconds: seconds(elapsed),
            };
        }
        if elapsed >= timings.after_restart {
            return RestartOutcome::StillMissing {
                waited_seconds: seconds(elapsed),
            };
        }
        host.sleep(timings.poll);
    }
}

// The records' writers live with their format, because the Better OS login
// supervisor (ticket 267) writes the same directory.
pub use manager_ipc::boot_watch::{Readers, prune, write_event, write_record, write_summary};

/// Turns the watch on or off. Written by the privileged service, never by a
/// window (ADR 0033).
pub fn set_enabled(records: &Records, enabled: bool) -> std::io::Result<()> {
    write_record(
        &records.settings_path(),
        &manager_ipc::boot_watch::Settings { enabled },
        Readers::Everyone,
    )
}

/// `org.betteros.BootWatch1`: the person's off switch, written here because
/// the records directory is root's (ADR 0033).
pub struct BootWatchService<A: crate::authorize::Authorizer + 'static> {
    authorizer: A,
    records: Records,
    gate: std::sync::Arc<crate::service::CallGate>,
    /// See `ManagerService::runtime`: zbus polls this on its own executor.
    runtime: tokio::runtime::Handle,
}

impl<A: crate::authorize::Authorizer + 'static> BootWatchService<A> {
    /// `gate` is the manager service's, so the daemon never leaves in the
    /// middle of this write either.
    pub fn new(
        authorizer: A,
        records: Records,
        gate: std::sync::Arc<crate::service::CallGate>,
    ) -> Self {
        Self {
            authorizer,
            records,
            gate,
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

#[zbus::interface(name = "org.betteros.BootWatch1")]
impl<A: crate::authorize::Authorizer + 'static> BootWatchService<A> {
    /// Turns the login screen watch on or off and returns whether it is on,
    /// as read back from the file.
    async fn set_enabled(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        enabled: bool,
    ) -> Result<bool, zbus::fdo::Error> {
        use crate::authorize::Authorization;
        use crate::service::refuse;
        let _call = self.gate.enter().map_err(refuse)?;
        match self
            .authorizer
            .decide(&header, crate::authorize::LOGIN_SCREEN_WATCH_ACTION)
            .await
        {
            Ok(Authorization::Granted) => {}
            Ok(Authorization::Dismissed) => return Err(refuse(crate::DaemonError::AuthCancelled)),
            Ok(Authorization::Denied) => return Err(refuse(crate::DaemonError::Unauthorized)),
            Err(error) => return Err(refuse(error)),
        }
        let records = self.records.clone();
        self.runtime
            .spawn_blocking(move || {
                set_enabled(&records, enabled)
                    .map_err(|error| crate::DaemonError::WriteFailed(error.to_string()))?;
                Ok(records.setting().watches())
            })
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?
            .map_err(refuse)
    }
}

/// The machine, as `better-greeter-watch.service` sees it.
pub struct SystemWatchHost {
    bus: Option<zbus::blocking::Connection>,
}

impl SystemWatchHost {
    pub fn new() -> Self {
        Self {
            bus: zbus::blocking::Connection::system().ok(),
        }
    }

    fn bus(&self) -> Result<&zbus::blocking::Connection, String> {
        self.bus
            .as_ref()
            .ok_or_else(|| "the system bus is not reachable".to_string())
    }

    fn can_graphical(&self) -> Result<bool, String> {
        let proxy = zbus::blocking::Proxy::new(
            self.bus()?,
            "org.freedesktop.login1",
            "/org/freedesktop/login1/seat/seat0",
            "org.freedesktop.login1.Seat",
        )
        .map_err(|e| e.to_string())?;
        proxy
            .get_property::<bool>("CanGraphical")
            .map_err(|e| e.to_string())
    }
}

impl Default for SystemWatchHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs a fixed command and returns its standard output, or why not.
fn command(program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(arguments)
        .env_clear()
        .env("LANG", "C.UTF-8")
        .env("SYSTEMD_PAGER", "")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "{program} {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// The PAM services of a person logging in. GDM's greeter is launched by a
/// worker for `gdm-launch-environment`, which is never counted: a greeter
/// that hangs after its worker started must still be restarted.
pub const PERSON_LOGIN_SERVICES: [&str; 4] = [
    "pam/gdm-password",
    "pam/gdm-autologin",
    "pam/gdm-fingerprint",
    "pam/gdm-smartcard",
];

/// Whether a `gdm-session-worker` (its `comm` is cut to 15 bytes) logging a
/// person in runs in `unit`'s cgroup, read from `proc`. The worker names its
/// PAM service in its process title, `gdm-session-worker [pam/gdm-password]`.
pub fn login_worker_in(proc: &Path, unit: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(proc) else {
        return false;
    };
    let cgroup_end = format!("/{unit}");
    entries.filter_map(Result::ok).any(|entry| {
        let dir = entry.path();
        let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
        let title = std::fs::read(dir.join("cmdline"))
            .map(|bytes| String::from_utf8_lossy(&bytes).replace('\0', " "))
            .unwrap_or_default();
        comm.trim() == "gdm-session-wor"
            && PERSON_LOGIN_SERVICES
                .iter()
                .any(|service| title.contains(&format!("[{service}]")))
            && std::fs::read_to_string(dir.join("cgroup"))
                .unwrap_or_default()
                .lines()
                .any(|line| line.trim_end().ends_with(&cgroup_end))
    })
}

/// The D-Bus error name of a failed call, or its text when it has none.
fn error_name(error: &zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(name, _, _) => name.as_str().to_string(),
        zbus::Error::FDO(error) => zbus::DBusError::name(error.as_ref()).to_string(),
        other => other.to_string(),
    }
}

/// The errors logind and the bus answer with for a session that has closed.
pub fn session_gone(error_name: &str) -> bool {
    matches!(
        error_name,
        "org.freedesktop.DBus.Error.UnknownObject"
            | "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.login1.NoSuchSession"
    )
}

/// `systemctl show`'s `Key=Value` lines.
pub fn parse_unit_state(text: &str) -> UnitState {
    let field = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let since = field("ActiveEnterTimestampMonotonic")
        .parse::<u64>()
        .ok()
        .filter(|micros| *micros > 0)
        .map(Duration::from_micros);
    UnitState {
        id: field("Id"),
        load_state: field("LoadState"),
        active_state: field("ActiveState"),
        active_since: since,
    }
}

const SYSTEMCTL: &str = "/usr/bin/systemctl";
const JOURNALCTL: &str = "/usr/bin/journalctl";

impl WatchHost for SystemWatchHost {
    fn boot_id(&self) -> Option<String> {
        let raw = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        let id = raw.trim().replace('-', "");
        is_boot_id(&id).then_some(id)
    }

    fn display_manager(&self) -> Result<UnitState, String> {
        command(
            SYSTEMCTL,
            &[
                "show",
                "--property=Id,LoadState,ActiveState,ActiveEnterTimestampMonotonic",
                "display-manager.service",
            ],
        )
        .map(|text| parse_unit_state(&text))
    }

    fn monotonic(&self) -> Duration {
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `now` is a valid, writable timespec.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
        Duration::new(
            now.tv_sec.max(0) as u64,
            now.tv_nsec.clamp(0, 999_999_999) as u32,
        )
    }

    fn wall_clock(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn sessions(&self) -> Result<Vec<SessionSeen>, String> {
        let bus = self.bus()?;
        let manager = zbus::blocking::Proxy::new(
            bus,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .map_err(|e| e.to_string())?;
        let listed: Vec<(String, u32, String, String, zbus::zvariant::OwnedObjectPath)> = manager
            .call("ListSessions", &())
            .map_err(|e| e.to_string())?;
        let mut sessions = Vec::new();
        for (id, _uid, _user, seat, path) in listed {
            let session = zbus::blocking::Proxy::new(
                bus,
                "org.freedesktop.login1",
                path,
                "org.freedesktop.login1.Session",
            )
            .map_err(|e| e.to_string())?;
            // A session that closed between the list and the question is
            // gone, and only that is skipped. Any other failure means logind
            // could not say, and the watch restarts nothing on a guess.
            let read = |name: &str| session.get_property::<String>(name);
            let (class, kind) = match (read("Class"), read("Type")) {
                (Ok(class), Ok(kind)) => (class, kind),
                (Err(error), _) | (_, Err(error)) => {
                    let name = error_name(&error);
                    if session_gone(&name) {
                        continue;
                    }
                    return Err(format!("session {id}: {name}: {error}"));
                }
            };
            sessions.push(SessionSeen {
                id,
                class,
                kind,
                seat,
            });
        }
        Ok(sessions)
    }

    fn evidence(&self, unit: &str) -> Evidence {
        let mut problems = Vec::new();
        let mut section = |result: Result<String, String>| match result {
            Ok(text) => tail(&text, MAX_SECTION_BYTES),
            Err(error) => {
                problems.push(error);
                String::new()
            }
        };
        let journal = |unit: &str| {
            command(
                JOURNALCTL,
                &[
                    "--boot=0",
                    "--no-pager",
                    "--quiet",
                    "--output=short-monotonic",
                    "--lines=400",
                    "--unit",
                    unit,
                ],
            )
        };
        let display_manager_journal = section(journal(unit));
        let logind_journal = section(journal("systemd-logind.service"));
        let kernel_ucsi = section(
            command(
                JOURNALCTL,
                &[
                    "--boot=0",
                    "--no-pager",
                    "--quiet",
                    "--output=short-monotonic",
                    "--dmesg",
                ],
            )
            .map(|text| {
                text.lines()
                    .filter(|line| line.to_ascii_lowercase().contains("ucsi"))
                    .map(|line| format!("{line}\n"))
                    .collect()
            }),
        );
        let failed_units = section(command(
            SYSTEMCTL,
            &["--failed", "--plain", "--no-legend", "--no-pager"],
        ));
        let seat0_can_graphical = match self.can_graphical() {
            Ok(value) => Some(value),
            Err(error) => {
                problems.push(format!("seat0 CanGraphical: {error}"));
                None
            }
        };
        let mut drm_devices: Vec<String> = std::fs::read_dir("/sys/class/drm")
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        drm_devices.sort();
        Evidence {
            display_manager_journal,
            logind_journal,
            kernel_ucsi,
            seat0_can_graphical,
            drm_devices,
            failed_units,
            sessions: Vec::new(),
            problems,
        }
    }

    fn gdm_config(&self) -> Vec<String> {
        GDM_CONFIG_FILES
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .collect()
    }

    fn login_worker_running(&self, unit: &str) -> bool {
        login_worker_in(Path::new("/proc"), unit)
    }

    fn restart(&self, unit: &str) -> Result<(), String> {
        if !GDM_UNITS.contains(&unit) {
            return Err(format!(
                "{unit} is not a display manager this watch restarts"
            ));
        }
        command(SYSTEMCTL, &["restart", "--no-block", unit]).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_ipc::boot_watch::MAX_EVENTS;
    use std::cell::{Cell, RefCell};

    const BOOT: &str = "0123456789abcdef0123456789abcdef";

    fn session(class: &str, kind: &str, seat: &str) -> SessionSeen {
        SessionSeen {
            id: "c1".to_string(),
            class: class.to_string(),
            kind: kind.to_string(),
            seat: seat.to_string(),
        }
    }

    /// A machine whose sessions follow a script on a fake clock that only
    /// `sleep` moves. GDM became active at 6 s.
    struct Fake {
        now: Cell<Duration>,
        boot: String,
        unit: UnitState,
        /// From when (since boot) each session is listed.
        script: Vec<(Duration, SessionSeen)>,
        /// What appears once the restart was asked for, and how long after.
        after_restart: Option<(Duration, SessionSeen)>,
        restarts: RefCell<Vec<(Duration, String)>>,
        restart_error: Option<String>,
        evidence_reads: Cell<u32>,
        config: Vec<String>,
        worker: bool,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                now: Cell::new(Duration::from_secs(5)),
                boot: BOOT.to_string(),
                unit: UnitState {
                    id: "gdm.service".to_string(),
                    load_state: "loaded".to_string(),
                    active_state: "active".to_string(),
                    active_since: Some(Duration::from_secs(6)),
                },
                script: Vec::new(),
                after_restart: None,
                restarts: RefCell::new(Vec::new()),
                restart_error: None,
                evidence_reads: Cell::new(0),
                config: Vec::new(),
                worker: false,
            }
        }
    }

    impl WatchHost for Fake {
        fn boot_id(&self) -> Option<String> {
            Some(self.boot.clone())
        }
        fn display_manager(&self) -> Result<UnitState, String> {
            Ok(self.unit.clone())
        }
        fn monotonic(&self) -> Duration {
            self.now.get()
        }
        fn wall_clock(&self) -> u64 {
            1_763_300_000 + self.now.get().as_secs()
        }
        fn sleep(&self, duration: Duration) {
            self.now.set(self.now.get() + duration);
        }
        fn sessions(&self) -> Result<Vec<SessionSeen>, String> {
            let now = self.now.get();
            let mut listed: Vec<SessionSeen> = self
                .script
                .iter()
                .filter(|(from, _)| *from <= now)
                .map(|(_, s)| s.clone())
                .collect();
            if let (Some((at, _)), Some((delay, appears))) =
                (self.restarts.borrow().first(), &self.after_restart)
                && now >= *at + *delay
            {
                listed.push(appears.clone());
            }
            Ok(listed)
        }
        fn evidence(&self, unit: &str) -> Evidence {
            self.evidence_reads.set(self.evidence_reads.get() + 1);
            Evidence {
                display_manager_journal: format!("{unit}: Starting GNOME Display Manager...\n"),
                seat0_can_graphical: Some(false),
                drm_devices: vec!["card1".to_string(), "renderD128".to_string()],
                ..Evidence::default()
            }
        }
        fn gdm_config(&self) -> Vec<String> {
            self.config.clone()
        }
        fn login_worker_running(&self, _unit: &str) -> bool {
            self.worker
        }
        fn restart(&self, unit: &str) -> Result<(), String> {
            if let Some(error) = &self.restart_error {
                return Err(error.clone());
            }
            self.restarts
                .borrow_mut()
                .push((self.now.get(), unit.to_string()));
            Ok(())
        }
    }

    fn records() -> (tempfile::TempDir, Records) {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path().join("boot-watch"));
        (dir, records)
    }

    #[test]
    fn no_greeter_records_the_evidence_restarts_once_and_records_the_outcome() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.after_restart = Some((
            Duration::from_secs(5),
            session("greeter", "wayland", "seat0"),
        ));
        let outcome = run(&host, &records, Timings::BOOT);
        assert_eq!(outcome, LastBootOutcome::Restarted);
        let restarts = host.restarts.borrow();
        assert_eq!(restarts.len(), 1);
        // Not before the deadline: GDM active at 6 s, plus 30 s.
        assert!(restarts[0].0 >= Duration::from_secs(36), "{restarts:?}");
        assert_eq!(restarts[0].1, "gdm.service");
        let events = records.events();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.boot_id, BOOT);
        assert_eq!(event.waited_seconds, 30);
        assert_eq!(
            event.restart,
            RestartOutcome::Recovered { after_seconds: 5 }
        );
        assert_eq!(event.evidence.seat0_can_graphical, Some(false));
        assert!(
            event
                .evidence
                .display_manager_journal
                .contains("gdm.service")
        );
        assert_eq!(
            records.last_boot().unwrap().outcome,
            LastBootOutcome::Restarted
        );
        // The event for administrators, the last boot for everyone.
        use std::os::unix::fs::PermissionsExt;
        for name in records.event_names() {
            let mode = std::fs::metadata(records.evidence_dir().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o640);
        }
        let mode = std::fs::metadata(records.last_boot_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o644);
    }

    #[test]
    fn a_login_screen_that_never_comes_back_is_recorded_as_still_missing() {
        let (_dir, records) = records();
        let host = Fake::new();
        run(&host, &records, Timings::BOOT);
        assert_eq!(host.restarts.borrow().len(), 1);
        assert_eq!(
            records.events()[0].restart,
            RestartOutcome::StillMissing { waited_seconds: 30 }
        );
    }

    #[test]
    fn a_greeter_in_time_changes_nothing() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.script = vec![(
            Duration::from_secs(10),
            session("greeter", "wayland", "seat0"),
        )];
        let outcome = run(&host, &records, Timings::BOOT);
        assert_eq!(
            outcome,
            LastBootOutcome::LoginScreenAppeared { after_seconds: 4 }
        );
        assert!(host.restarts.borrow().is_empty());
        assert_eq!(host.evidence_reads.get(), 0);
        assert!(records.events().is_empty());
    }

    #[test]
    fn a_person_already_logged_in_is_never_touched() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        // A text console login, and no login screen at all.
        host.script = vec![(Duration::ZERO, session("user", "tty", "seat0"))];
        let outcome = run(&host, &records, Timings::BOOT);
        assert_eq!(outcome, LastBootOutcome::PersonPresent);
        assert!(host.restarts.borrow().is_empty());
        assert!(records.events().is_empty());
    }

    #[test]
    fn a_person_who_logs_in_after_the_deadline_keeps_gdm() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        // Logs in on a text console just after the deadline loop's last look
        // (at 36 s), while the evidence is being read.
        host.script = vec![(
            Duration::from_secs(36) + Duration::from_millis(1),
            session("user", "tty", ""),
        )];
        struct Late<'a>(&'a Fake);
        impl WatchHost for Late<'_> {
            fn boot_id(&self) -> Option<String> {
                self.0.boot_id()
            }
            fn display_manager(&self) -> Result<UnitState, String> {
                self.0.display_manager()
            }
            fn monotonic(&self) -> Duration {
                self.0.monotonic()
            }
            fn wall_clock(&self) -> u64 {
                self.0.wall_clock()
            }
            fn sleep(&self, d: Duration) {
                self.0.sleep(d)
            }
            fn sessions(&self) -> Result<Vec<SessionSeen>, String> {
                self.0.sessions()
            }
            fn evidence(&self, unit: &str) -> Evidence {
                // Reading the journal takes a moment.
                self.0.sleep(Duration::from_millis(10));
                self.0.evidence(unit)
            }
            fn restart(&self, unit: &str) -> Result<(), String> {
                self.0.restart(unit)
            }
            fn gdm_config(&self) -> Vec<String> {
                self.0.gdm_config()
            }
            fn login_worker_running(&self, unit: &str) -> bool {
                self.0.login_worker_running(unit)
            }
        }
        let outcome = run(&Late(&host), &records, Timings::BOOT);
        assert_eq!(outcome, LastBootOutcome::Restarted);
        assert!(host.restarts.borrow().is_empty());
        assert_eq!(records.events()[0].restart, RestartOutcome::PersonAppeared);
    }

    #[test]
    fn a_second_failure_on_the_same_boot_never_restarts_again() {
        let (_dir, records) = records();
        let host = Fake::new();
        run(&host, &records, Timings::BOOT);
        assert_eq!(host.restarts.borrow().len(), 1);
        // The unit run again by hand, the login screen still missing.
        let outcome = run(&host, &records, Timings::BOOT);
        assert_eq!(outcome, LastBootOutcome::AlreadyHandled);
        assert_eq!(host.restarts.borrow().len(), 1);
        assert_eq!(records.events().len(), 1);
    }

    #[test]
    fn a_failed_restart_is_recorded_with_systemds_words() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.restart_error = Some("Job for gdm.service canceled.".to_string());
        run(&host, &records, Timings::BOOT);
        assert_eq!(
            records.events()[0].restart,
            RestartOutcome::RestartFailed {
                details: "Job for gdm.service canceled.".to_string()
            }
        );
    }

    #[test]
    fn only_the_newest_twenty_records_are_kept() {
        let (_dir, records) = records();
        for index in 0..25u64 {
            let mut host = Fake::new();
            host.boot = format!("{index:032x}");
            host.now.set(Duration::from_secs(5 + index * 1000));
            host.unit.active_since = Some(host.now.get() + Duration::from_secs(1));
            run(&host, &records, Timings::BOOT);
        }
        let names = records.event_names();
        assert_eq!(names.len(), MAX_EVENTS);
        let kept: Vec<String> = records.events().iter().map(|e| e.boot_id.clone()).collect();
        assert_eq!(kept.first().unwrap(), &format!("{:032x}", 24));
        assert_eq!(kept.last().unwrap(), &format!("{:032x}", 5));
    }

    #[test]
    fn turned_off_without_gdm_or_with_another_display_manager_nothing_happens() {
        let (_dir, records) = records();
        set_enabled(&records, false).unwrap();
        let host = Fake::new();
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::TurnedOff
        );
        set_enabled(&records, true).unwrap();
        // Each case below is its own boot: a boot already watched is never
        // watched again.
        let boot = |n: u8| format!("{n:032x}");

        let mut host = Fake::new();
        host.boot = boot(1);
        host.unit.load_state = "not-found".to_string();
        host.unit.id = "display-manager.service".to_string();
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::NoGdm {
                display_manager: None
            }
        );
        let mut host = Fake::new();
        host.boot = boot(2);
        host.unit.id = "lightdm.service".to_string();
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::NoGdm {
                display_manager: Some("lightdm.service".to_string())
            }
        );
        assert!(records.events().is_empty());
        assert_eq!(
            records.last_boot().unwrap().outcome,
            LastBootOutcome::NoGdm {
                display_manager: Some("lightdm.service".to_string())
            }
        );
        std::fs::write(records.settings_path(), "garbage").unwrap();
        let mut host = Fake::new();
        host.boot = boot(3);
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::SettingUnreadable
        );
        assert!(host.restarts.borrow().is_empty());
    }

    #[test]
    fn a_gdm_that_never_becomes_active_is_left_to_systemd() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.unit.active_state = "activating".to_string();
        host.unit.active_since = None;
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::GdmNotStarted {
                state: "activating".to_string()
            }
        );
        assert!(host.restarts.borrow().is_empty());
        // Gave up after a minute, well inside the unit's 180 s.
        assert!(host.now.get() <= Duration::from_secs(66));
    }

    #[test]
    fn the_whole_watch_fits_inside_the_units_timeout() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.unit.active_state = "activating".to_string();
        // Becomes active just before the start wait would give up.
        host.unit.active_since = None;
        struct Slow<'a>(&'a Fake);
        impl WatchHost for Slow<'_> {
            fn boot_id(&self) -> Option<String> {
                self.0.boot_id()
            }
            fn display_manager(&self) -> Result<UnitState, String> {
                let mut unit = self.0.unit.clone();
                if self.0.now.get() >= Duration::from_secs(64) {
                    unit.active_state = "active".to_string();
                    unit.active_since = Some(Duration::from_secs(64));
                }
                Ok(unit)
            }
            fn monotonic(&self) -> Duration {
                self.0.monotonic()
            }
            fn wall_clock(&self) -> u64 {
                self.0.wall_clock()
            }
            fn sleep(&self, d: Duration) {
                self.0.sleep(d)
            }
            fn sessions(&self) -> Result<Vec<SessionSeen>, String> {
                self.0.sessions()
            }
            fn evidence(&self, unit: &str) -> Evidence {
                self.0.evidence(unit)
            }
            fn restart(&self, unit: &str) -> Result<(), String> {
                self.0.restart(unit)
            }
            fn gdm_config(&self) -> Vec<String> {
                self.0.gdm_config()
            }
            fn login_worker_running(&self, unit: &str) -> bool {
                self.0.login_worker_running(unit)
            }
        }
        run(&Slow(&host), &records, Timings::BOOT);
        let spent = host.now.get() - Duration::from_secs(5);
        assert!(spent < Duration::from_secs(150), "{spent:?}");
        assert_eq!(host.restarts.borrow().len(), 1);
    }

    #[test]
    fn gdm_configuration_is_read_from_every_daemon_section() {
        let on = "[daemon]\nAutomaticLoginEnable=True\nAutomaticLogin=tim\n".to_string();
        let commented = "[daemon]\n#AutomaticLoginEnable=true\n".to_string();
        let other_section = "[security]\nAutomaticLoginEnable=true\n".to_string();
        let off = "[daemon]\nAutomaticLoginEnable=false\n".to_string();
        assert!(gdm_login(std::slice::from_ref(&on)).automatic);
        assert_eq!(
            gdm_login(&[commented, other_section, off]),
            GdmLogin::default()
        );
        assert_eq!(gdm_login(&[]), GdmLogin::default());
        // A header with spaces, and [daemon] twice with another between.
        let spaced = "[daemon]   \nWaylandEnable=true\n[security]\nx=1\n[ daemon ]\nAutomaticLoginEnable = 1\n"
            .to_string();
        assert!(gdm_login(&[spaced]).automatic);
        let timed =
            "[daemon]\nTimedLoginEnable=true\n[debug]\n[daemon]\nTimedLoginDelay=90\n".to_string();
        assert_eq!(
            gdm_login(&[timed]),
            GdmLogin {
                automatic: false,
                timed_delay: Some(Duration::from_secs(90))
            }
        );
        let timed_default = "[daemon]\nTimedLoginEnable=1\n".to_string();
        assert_eq!(
            gdm_login(&[timed_default]).timed_delay,
            Some(GDM_TIMED_LOGIN_DELAY)
        );
    }

    #[test]
    fn automatic_login_means_never_restart() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.config = vec!["[daemon]\nAutomaticLoginEnable=true\n".to_string()];
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::AutomaticLogin
        );
        assert!(host.restarts.borrow().is_empty());
        assert!(records.events().is_empty());
    }

    #[test]
    fn timed_login_is_watched_after_its_delay_plus_thirty_seconds() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.config = vec!["[daemon]\nTimedLoginEnable=true\nTimedLoginDelay=60\n".to_string()];
        run(&host, &records, Timings::BOOT);
        let restarts = host.restarts.borrow();
        assert_eq!(restarts.len(), 1);
        // GDM active at 6 s, plus 60 s of delay, plus 30 s.
        assert!(restarts[0].0 >= Duration::from_secs(96), "{restarts:?}");
        assert_eq!(records.events()[0].waited_seconds, 90);
    }

    #[test]
    fn a_login_gdm_is_in_the_middle_of_is_not_restarted() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.worker = true;
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::Restarted
        );
        assert!(host.restarts.borrow().is_empty());
        assert_eq!(records.events()[0].restart, RestartOutcome::LoginInProgress);
    }

    #[test]
    fn only_a_person_login_worker_in_gdms_own_cgroup_counts() {
        let proc = tempfile::tempdir().unwrap();
        let process = |pid: &str, comm: &str, title: &str, cgroup: &str| {
            let dir = proc.path().join(pid);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("comm"), format!("{comm}\n")).unwrap();
            // A process title padded with NULs, as setproctitle leaves it.
            std::fs::write(dir.join("cmdline"), format!("{title}\0\0")).unwrap();
            std::fs::write(dir.join("cgroup"), format!("0::{cgroup}\n")).unwrap();
        };
        process("1", "systemd", "/sbin/init", "/init.scope");
        process(
            "900",
            "gdm-session-wor",
            "gdm-session-worker [pam/gdm-password]",
            "/user.slice/user-1000.slice/session-2.scope",
        );
        assert!(!login_worker_in(proc.path(), "gdm.service"));
        // The worker that launches the greeter is not a person logging in.
        process(
            "901",
            "gdm-session-wor",
            "gdm-session-worker [pam/gdm-launch-environment]",
            "/system.slice/gdm.service",
        );
        assert!(!login_worker_in(proc.path(), "gdm.service"));
        process(
            "902",
            "gdm-session-wor",
            "gdm-session-worker [pam/gdm-password]",
            "/system.slice/gdm.service",
        );
        assert!(login_worker_in(proc.path(), "gdm.service"));
        assert!(!login_worker_in(proc.path(), "gdm3.service"));
    }

    #[test]
    fn only_a_closed_session_is_skipped_and_any_other_logind_error_stops_the_watch() {
        assert!(session_gone("org.freedesktop.DBus.Error.UnknownObject"));
        assert!(session_gone("org.freedesktop.login1.NoSuchSession"));
        assert!(!session_gone("org.freedesktop.DBus.Error.AccessDenied"));
        assert!(!session_gone("org.freedesktop.DBus.Error.NoReply"));
        assert!(!session_gone("org.freedesktop.DBus.Error.Timeout"));
    }

    #[test]
    fn a_boot_whose_last_boot_record_names_it_is_never_watched_again() {
        let (_dir, records) = records();
        let mut host = Fake::new();
        host.script = vec![(Duration::ZERO, session("user", "tty", ""))];
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::PersonPresent
        );
        // The unit run again by hand, the person gone and nothing on screen.
        let host = Fake::new();
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::AlreadyHandled
        );
        assert!(host.restarts.borrow().is_empty());
    }

    #[test]
    fn this_boots_record_survives_pruning_even_when_its_clock_named_it_oldest() {
        let (_dir, records) = records();
        for index in 0..MAX_EVENTS as u64 {
            let mut host = Fake::new();
            host.boot = format!("{:032x}", index + 100);
            host.now.set(Duration::from_secs(5 + (index + 1) * 1000));
            host.unit.active_since = Some(host.now.get() + Duration::from_secs(1));
            run(&host, &records, Timings::BOOT);
        }
        // A boot whose clock was years behind: its name sorts first.
        struct Behind<'a>(&'a Fake);
        impl WatchHost for Behind<'_> {
            fn boot_id(&self) -> Option<String> {
                self.0.boot_id()
            }
            fn display_manager(&self) -> Result<UnitState, String> {
                self.0.display_manager()
            }
            fn monotonic(&self) -> Duration {
                self.0.monotonic()
            }
            fn wall_clock(&self) -> u64 {
                10
            }
            fn sleep(&self, d: Duration) {
                self.0.sleep(d)
            }
            fn sessions(&self) -> Result<Vec<SessionSeen>, String> {
                self.0.sessions()
            }
            fn evidence(&self, unit: &str) -> Evidence {
                self.0.evidence(unit)
            }
            fn restart(&self, unit: &str) -> Result<(), String> {
                self.0.restart(unit)
            }
            fn gdm_config(&self) -> Vec<String> {
                Vec::new()
            }
            fn login_worker_running(&self, _unit: &str) -> bool {
                false
            }
        }
        let host = Fake::new();
        run(&Behind(&host), &records, Timings::BOOT);
        assert_eq!(records.event_names().len(), MAX_EVENTS);
        assert!(records.has_event_for(BOOT));
        assert_eq!(host.restarts.borrow().len(), 1);
    }

    #[test]
    fn evidence_is_for_administrators_and_the_summary_carries_no_journal() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, records) = records();
        run(&Fake::new(), &records, Timings::BOOT);
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&records.evidence_dir()), 0o750);
        let event = records.evidence_dir().join(&records.event_names()[0]);
        assert_eq!(mode(&event), 0o640);
        assert_eq!(mode(&records.summary_path()), 0o644);
        let summary = std::fs::read_to_string(records.summary_path()).unwrap();
        assert!(
            !summary.contains("Starting GNOME Display Manager"),
            "{summary}"
        );
        assert!(
            std::fs::read_to_string(&event)
                .unwrap()
                .contains("Starting GNOME Display Manager")
        );
    }

    #[test]
    fn a_record_is_never_written_through_a_planted_link() {
        let (dir, records) = records();
        std::fs::create_dir_all(records.dir()).unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, "keep").unwrap();
        // A link where the temporary file goes, and one where the record goes.
        std::os::unix::fs::symlink(&target, records.dir().join(".last-boot.json.tmp")).unwrap();
        std::os::unix::fs::symlink(&target, records.last_boot_path()).unwrap();
        run(&Fake::new(), &records, Timings::BOOT);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
        assert!(
            !std::fs::symlink_metadata(records.last_boot_path())
                .unwrap()
                .is_symlink()
        );
    }

    #[test]
    fn systemctl_show_is_read_field_by_field() {
        let state = parse_unit_state(
            "Id=gdm.service\nLoadState=loaded\nActiveState=active\nActiveEnterTimestampMonotonic=6123456\n",
        );
        assert_eq!(state.id, "gdm.service");
        assert_eq!(state.active_since, Some(Duration::from_micros(6_123_456)));
        let missing = parse_unit_state(
            "Id=display-manager.service\nLoadState=not-found\nActiveState=inactive\nActiveEnterTimestampMonotonic=0\n",
        );
        assert_eq!(missing.load_state, "not-found");
        assert_eq!(missing.active_since, None);
    }

    #[test]
    fn the_real_host_restarts_nothing_but_gdm() {
        let host = SystemWatchHost { bus: None };
        assert!(host.restart("ssh.service").is_err());
        assert!(host.restart("better-manager-daemon.service").is_err());
    }
}
