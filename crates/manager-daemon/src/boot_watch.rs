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

use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use manager_ipc::boot_watch::{
    Event, Evidence, LastBoot, LastBootOutcome, MAX_EVENTS, MAX_SECTION_BYTES, RECORD_VERSION,
    Records, RestartOutcome, SessionSeen, Setting, is_boot_id, tail,
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
    if let Err(error) = write_record(&records.last_boot_path(), &last) {
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
    if records.has_event_for(boot_id) {
        return LastBootOutcome::AlreadyHandled;
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
    let deadline = active_since + timings.deadline;
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
    if let Err(error) = write_record(&path, &event) {
        // Without the record, "once per boot" would rest on nothing.
        eprintln!(
            "better-greeter-watch: could not write {}: {error}",
            path.display()
        );
        return LastBootOutcome::CouldNotWatch {
            details: format!("record not written: {error}"),
        };
    }
    prune(records);

    // A person who logged in on a text console in the meantime keeps GDM.
    event.restart = match host.sessions().map(|sessions| look(&sessions)) {
        Ok(Seen::Person) => RestartOutcome::PersonAppeared,
        Ok(Seen::LoginScreen) => RestartOutcome::AppearedBeforeRestart,
        Err(details) => RestartOutcome::RestartFailed {
            details: format!("logind: {details}"),
        },
        Ok(Seen::Nothing) => match host.restart(&unit) {
            Err(details) => RestartOutcome::RestartFailed { details },
            Ok(()) => wait_after_restart(host, timings),
        },
    };
    if let Err(error) = write_record(&path, &event) {
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

/// Keeps the newest [`MAX_EVENTS`] event records.
pub fn prune(records: &Records) {
    let names = records.event_names();
    let excess = names.len().saturating_sub(MAX_EVENTS);
    for name in names.iter().take(excess) {
        let _ = std::fs::remove_file(records.dir().join(name));
    }
}

/// Writes `value` as JSON, readable by everyone (0644), replacing the file
/// whole.
pub fn write_record<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("record")
    ));
    let json = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&temporary)?;
    file.write_all(&json)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

/// Turns the watch on or off. Written by the privileged service, never by a
/// window (ADR 0033).
pub fn set_enabled(records: &Records, enabled: bool) -> std::io::Result<()> {
    write_record(
        &records.settings_path(),
        &manager_ipc::boot_watch::Settings { enabled },
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
            // A session that closed between the list and the question is gone,
            // not an error.
            let (Ok(class), Ok(kind)) = (
                session.get_property::<String>("Class"),
                session.get_property::<String>("Type"),
            ) else {
                continue;
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
        // Readable by the person.
        use std::os::unix::fs::PermissionsExt;
        for name in records.event_names() {
            let mode = std::fs::metadata(records.dir().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o644);
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

        let mut host = Fake::new();
        host.unit.load_state = "not-found".to_string();
        host.unit.id = "display-manager.service".to_string();
        assert_eq!(
            run(&host, &records, Timings::BOOT),
            LastBootOutcome::NoGdm {
                display_manager: None
            }
        );
        let mut host = Fake::new();
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
        let host = Fake::new();
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
        }
        run(&Slow(&host), &records, Timings::BOOT);
        let spent = host.now.get() - Duration::from_secs(5);
        assert!(spent < Duration::from_secs(150), "{spent:?}");
        assert_eq!(host.restarts.borrow().len(), 1);
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
