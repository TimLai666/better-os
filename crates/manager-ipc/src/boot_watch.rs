//! The login screen watch's records (ADR 0033, ticket 140).
//!
//! `better-greeter-watch.service` runs once per boot as root and writes these
//! files under [`DIR`]. What anyone may read is 0644 there: the setting, what
//! the latest boot's watch did, and [`SUMMARY_FILE`], every record without
//! its evidence. The evidence (journal excerpts, failed units) is in
//! [`EVIDENCE_DIR`], 0750 root:adm with 0640 files, so only an administrator
//! reads it (ADR 0033). The privileged service writes [`SETTINGS_FILE`] when
//! the person turns the watch off or on.
//!
//! Everything here is read as untrusted input: a file larger than its limit is
//! skipped before it is parsed, unknown fields are refused, and a file that
//! does not parse is left out of the list rather than guessed at.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where the records live.
pub const DIR: &str = "/var/lib/better-os/boot-watch";

/// The full records, journal excerpts included: 0750 root:adm.
pub const EVIDENCE_DIR: &str = "evidence";

/// Every record without its evidence, readable by everyone.
pub const SUMMARY_FILE: &str = "summary.json";

/// The group that may read the evidence: the first user of an Ubuntu install
/// is in it, as everyone who may read the system journal is.
pub const EVIDENCE_GROUP: &str = "adm";

/// The person's choice, written by the privileged service.
pub const SETTINGS_FILE: &str = "settings.json";

/// What the watch found on the most recent boot it ran on, overwritten every
/// boot.
pub const LAST_BOOT_FILE: &str = "last-boot.json";

/// Event files are `<detected unix seconds, 20 digits>-<boot id>.json`, so a
/// name sorts by time and says which boot it belongs to.
pub const EVENT_SUFFIX: &str = ".json";

/// How many event records are kept; the oldest go first.
pub const MAX_EVENTS: usize = 20;

/// The largest event file read back. A record is capped well below this when
/// it is written ([`MAX_SECTION_BYTES`] per journal section).
pub const MAX_EVENT_BYTES: u64 = 512 * 1024;

/// The most kept from one evidence section (a journal excerpt, the failed
/// units). The tail is kept: the last lines before the hang are the ones that
/// say what GDM was waiting for.
pub const MAX_SECTION_BYTES: usize = 64 * 1024;

/// The record format. A reader refuses any other.
pub const RECORD_VERSION: u32 = 1;

/// Whether the watch is on.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
}

/// What a reader can say about the setting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting {
    /// No settings file: the watch is on, as installed.
    OnByDefault,
    On,
    Off,
    /// The file is there and could not be read. The watch treats this as off
    /// and says so; a window shows it as unknown, never as on.
    Unreadable,
}

impl Setting {
    /// Whether the watch acts on this boot.
    pub fn watches(self) -> bool {
        matches!(self, Setting::OnByDefault | Setting::On)
    }
}

/// One login session logind listed, as the watch saw it. No user name is
/// kept: the class says whether it is a person's.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSeen {
    pub id: String,
    pub class: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub seat: String,
}

impl SessionSeen {
    /// A person's own session, of any kind (graphical, text, remote) or a
    /// screen locked over one. The watch never touches a boot that has one.
    pub fn is_person(&self) -> bool {
        self.class.starts_with("user") || self.class == "lock-screen"
    }

    /// What the watch waits for: GDM's greeter, or any graphical session on
    /// the first seat.
    pub fn is_login_screen(&self) -> bool {
        self.seat == "seat0"
            && (self.class == "greeter" || matches!(self.kind.as_str(), "wayland" | "x11" | "mir"))
    }
}

/// What was recorded before the restart.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// GDM's journal for this boot, the tail.
    pub display_manager_journal: String,
    /// systemd-logind's journal for this boot, the tail.
    pub logind_journal: String,
    /// Kernel lines naming `ucsi` (ticket 101), the tail.
    pub kernel_ucsi: String,
    /// seat0's `CanGraphical`; `None` when logind did not answer.
    pub seat0_can_graphical: Option<bool>,
    /// The entries of `/sys/class/drm`.
    pub drm_devices: Vec<String>,
    /// `systemctl --failed`, the tail.
    pub failed_units: String,
    /// Every session logind listed at the deadline.
    pub sessions: Vec<SessionSeen>,
    /// What could not be read, one line each.
    pub problems: Vec<String>,
}

/// How the restart ended.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result", deny_unknown_fields)]
pub enum RestartOutcome {
    /// Written before the restart is asked for, so a watch that is killed in
    /// the middle still leaves a record of having restarted.
    Pending,
    /// The login screen appeared this many seconds after the restart.
    Recovered { after_seconds: u32 },
    /// It still had not appeared when the watch stopped waiting.
    StillMissing { waited_seconds: u32 },
    /// systemd refused the restart.
    RestartFailed { details: String },
    /// A person's session appeared between the deadline and the restart, so
    /// nothing was restarted.
    PersonAppeared,
    /// GDM was running a login (a `gdm-session-worker`) at the deadline, so
    /// nothing was restarted.
    LoginInProgress,
    /// The login screen appeared between the deadline and the restart, so
    /// nothing was restarted.
    AppearedBeforeRestart,
    /// The Better OS login supervisor (ticket 267) had to restart the login
    /// screen `restarts` times (one or two) before it accepted input;
    /// `stage` is the stage the last miss was at.
    LoginScreenRestarted {
        stage: LoginStage,
        restarts: u32,
        after_seconds: u32,
    },
    /// The Better OS login supervisor gave up on the login screen at
    /// `stage` and started the text login on `tty` (`tty1` or `tty2`).
    TextLoginUsed {
        stage: LoginStage,
        tty: String,
        after_seconds: u32,
    },
}

/// The stages of the Better OS login path a record can name
/// (design/login-screen-spec.md section 5.10, its table of stage names):
/// the ones the fall back or a retry can be at. The supervisor's journal
/// lines name more (1a, 2, 4, 9), which never end an appearance.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStage {
    /// 1: no graphics device.
    GraphicsDevice,
    /// 3: logind did not answer.
    LoginManager,
    /// 5: greetd or the greeter's session did not start.
    SignInService,
    /// 6: the compositor did not start.
    DisplayServer,
    /// 7: the greeter did not become ready, or stopped answering.
    LoginScreenProgram,
    /// The supervisor itself (its state could not be read; the backstop's
    /// own message names this row too).
    Supervisor,
}

impl LoginStage {
    pub const ALL: [LoginStage; 6] = [
        LoginStage::GraphicsDevice,
        LoginStage::LoginManager,
        LoginStage::SignInService,
        LoginStage::DisplayServer,
        LoginStage::LoginScreenProgram,
        LoginStage::Supervisor,
    ];

    /// The stage's number in the spec's table, as the journal lines carry it.
    pub fn code(self) -> &'static str {
        match self {
            LoginStage::GraphicsDevice => "1",
            LoginStage::LoginManager => "3",
            LoginStage::SignInService => "5",
            LoginStage::DisplayServer => "6",
            LoginStage::LoginScreenProgram => "7",
            LoginStage::Supervisor => "supervisor",
        }
    }

    /// The stage's name in English: what the text console shows.
    pub fn english(self) -> &'static str {
        match self {
            LoginStage::GraphicsDevice => "no graphics device",
            LoginStage::LoginManager => "the login manager didn't respond",
            LoginStage::SignInService => "the sign-in service didn't start",
            LoginStage::DisplayServer => "the display server didn't start",
            LoginStage::LoginScreenProgram => "the login screen program didn't respond",
            LoginStage::Supervisor => "the login supervisor couldn't run",
        }
    }

    /// The stage's name in zh-TW: what Better Manager and a terminal that
    /// can draw it show.
    pub fn zh_tw(self) -> &'static str {
        match self {
            LoginStage::GraphicsDevice => "找不到顯示卡",
            LoginStage::LoginManager => "登入管理服務沒有回應",
            LoginStage::SignInService => "登入服務無法啟動",
            LoginStage::DisplayServer => "顯示伺服器無法啟動",
            LoginStage::LoginScreenProgram => "登入畫面程式沒有回應",
            LoginStage::Supervisor => "登入監督程式無法執行",
        }
    }
}

/// The unit the Better OS login supervisor's records name as their display
/// manager.
pub const LOGIN_SUPERVISOR_UNIT: &str = "better-os-login-supervisor.service";

/// One boot on which the login screen did not appear in time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub version: u32,
    pub boot_id: String,
    /// When the deadline passed, Unix seconds.
    pub detected_at_unix_seconds: u64,
    /// The display manager's unit, `gdm.service` or `gdm3.service`.
    pub display_manager: String,
    /// Seconds between GDM starting and the deadline.
    pub waited_seconds: u32,
    pub evidence: Evidence,
    pub restart: RestartOutcome,
}

/// What the watch did on one boot, every boot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", deny_unknown_fields)]
pub enum LastBootOutcome {
    /// The login screen appeared this many seconds after GDM started.
    LoginScreenAppeared { after_seconds: u32 },
    /// A person's session already existed; nothing was checked further.
    PersonPresent,
    /// It did not appear in time; the event record says what happened next.
    Restarted,
    /// This boot already has an event record (the unit was run again by
    /// hand); a second restart is never made.
    AlreadyHandled,
    /// The person turned the watch off.
    TurnedOff,
    /// GDM logs someone in automatically (`AutomaticLoginEnable` or
    /// `TimedLoginEnable`), so a missing login screen is expected and GDM is
    /// never restarted.
    AutomaticLogin,
    /// The settings file could not be read, so the watch did nothing.
    SettingUnreadable,
    /// No GDM: `display_manager` names what is there instead, if anything.
    NoGdm { display_manager: Option<String> },
    /// GDM did not reach active in time. systemd handles a display manager
    /// that fails to start; this watch only handles one that starts and then
    /// never opens the login screen.
    GdmNotStarted { state: String },
    /// The watch could not do its job: logind did not answer, there was no
    /// boot id, or the record could not be written. Nothing was restarted.
    CouldNotWatch { details: String },
}

/// Not `deny_unknown_fields`: serde cannot combine it with the flattened
/// outcome. The outcome's own tag is still a closed set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LastBoot {
    pub version: u32,
    pub boot_id: String,
    pub checked_at_unix_seconds: u64,
    #[serde(flatten)]
    pub outcome: LastBootOutcome,
}

/// The watch's files, at [`DIR`] or a test's directory.
#[derive(Clone, Debug)]
pub struct Records {
    dir: PathBuf,
}

impl Default for Records {
    fn default() -> Self {
        Self::system()
    }
}

impl Records {
    pub fn system() -> Self {
        Self::at(DIR)
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn settings_path(&self) -> PathBuf {
        self.dir.join(SETTINGS_FILE)
    }

    pub fn last_boot_path(&self) -> PathBuf {
        self.dir.join(LAST_BOOT_FILE)
    }

    pub fn evidence_dir(&self) -> PathBuf {
        self.dir.join(EVIDENCE_DIR)
    }

    pub fn summary_path(&self) -> PathBuf {
        self.dir.join(SUMMARY_FILE)
    }

    /// The file an event of `boot_id` detected at `at` is written to.
    pub fn event_path(&self, at: u64, boot_id: &str) -> PathBuf {
        self.evidence_dir()
            .join(format!("{at:020}-{boot_id}{EVENT_SUFFIX}"))
    }

    pub fn setting(&self) -> Setting {
        match std::fs::read(self.settings_path()) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Setting::OnByDefault,
            Err(_) => Setting::Unreadable,
            Ok(bytes) if bytes.len() > 4096 => Setting::Unreadable,
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(Settings { enabled: true }) => Setting::On,
                Ok(Settings { enabled: false }) => Setting::Off,
                Err(_) => Setting::Unreadable,
            },
        }
    }

    pub fn last_boot(&self) -> Option<LastBoot> {
        let bytes = read_capped(&self.last_boot_path(), 4096)?;
        serde_json::from_slice::<LastBoot>(&bytes)
            .ok()
            .filter(|record| record.version == RECORD_VERSION)
    }

    /// The event files' names, oldest first.
    pub fn event_names(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.evidence_dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| is_event_name(name))
            .collect();
        names.sort();
        names
    }

    /// Whether this boot already has an event record.
    pub fn has_event_for(&self, boot_id: &str) -> bool {
        let suffix = format!("-{boot_id}{EVENT_SUFFIX}");
        self.event_names()
            .iter()
            .any(|name| name.ends_with(&suffix))
    }

    /// Whether this boot was already watched: an event record, or
    /// `last-boot.json` naming it. Either way GDM is never restarted again.
    pub fn handled(&self, boot_id: &str) -> bool {
        self.has_event_for(boot_id) || self.last_boot().is_some_and(|last| last.boot_id == boot_id)
    }

    /// Every event with its evidence, newest first. Needs the evidence
    /// directory to be readable.
    pub fn events(&self) -> Vec<Event> {
        let evidence = self.evidence_dir();
        let mut events: Vec<Event> = self
            .event_names()
            .iter()
            .filter_map(|name| read_capped(&evidence.join(name), MAX_EVENT_BYTES))
            .filter_map(|bytes| serde_json::from_slice::<Event>(&bytes).ok())
            .filter(|event| event.version == RECORD_VERSION)
            .collect();
        events.sort_by_key(|event| std::cmp::Reverse(event.detected_at_unix_seconds));
        events
    }
}

impl Records {
    /// What this reader may see: every record with its evidence when the
    /// evidence directory is readable, otherwise the public summary, every
    /// record with its evidence left empty.
    pub fn read_events(&self) -> Events {
        if std::fs::read_dir(self.evidence_dir()).is_ok() {
            return Events {
                events: self.events(),
                evidence_readable: true,
            };
        }
        let mut events: Vec<Event> = read_capped(&self.summary_path(), MAX_EVENT_BYTES)
            .and_then(|bytes| serde_json::from_slice::<Summary>(&bytes).ok())
            .filter(|summary| summary.version == RECORD_VERSION)
            .map(|summary| summary.events)
            .unwrap_or_default();
        events.retain(|event| event.version == RECORD_VERSION);
        events.sort_by_key(|event| std::cmp::Reverse(event.detected_at_unix_seconds));
        Events {
            events,
            evidence_readable: false,
        }
    }
}

/// The records a reader got.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Events {
    /// Newest first.
    pub events: Vec<Event>,
    /// False when only the summary could be read: each event's evidence is
    /// then empty, and the details need an administrator.
    pub evidence_readable: bool,
}

/// [`SUMMARY_FILE`]: every record with its evidence left empty.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub version: u32,
    pub events: Vec<Event>,
}

impl Summary {
    /// The public summary of `events`: nothing from the journal.
    pub fn of(events: &[Event]) -> Self {
        Self {
            version: RECORD_VERSION,
            events: events
                .iter()
                .map(|event| Event {
                    evidence: Evidence::default(),
                    ..event.clone()
                })
                .collect(),
        }
    }
}

/// Whether a record's `pending` restart belongs to an earlier boot: the
/// watch was cut off and never wrote how it ended.
pub fn interrupted(event: &Event, current_boot: Option<&str>) -> bool {
    event.restart == RestartOutcome::Pending
        && current_boot.is_some_and(|boot| boot != event.boot_id)
}

/// This boot's id, dashes removed, as the watch records it.
pub fn current_boot_id() -> Option<String> {
    let raw = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let id = raw.trim().replace('-', "");
    is_boot_id(&id).then_some(id)
}

/// `<20 digits>-<boot id>.json`, and nothing else in the directory.
fn is_event_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(EVENT_SUFFIX) else {
        return false;
    };
    let Some((time, boot)) = stem.split_once('-') else {
        return false;
    };
    time.len() == 20 && time.bytes().all(|b| b.is_ascii_digit()) && is_boot_id(boot)
}

/// A boot id as the kernel writes it, without dashes (the watch strips them):
/// 32 lowercase hexadecimal digits.
pub fn is_boot_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn read_capped(path: &Path, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

/// The last `limit` bytes of `text`, cut at a line start, with a first line
/// saying how much was dropped.
pub fn tail(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if let Some(newline) = text[start..].find('\n') {
        start += newline + 1;
    }
    format!("[{} earlier bytes not kept]\n{}", start, &text[start..])
}

/// Keeps the newest [`MAX_EVENTS`] event records.
/// Never this boot's own records, whatever their names sort as (a clock that
/// was wrong at boot names one as the oldest).
pub fn prune(records: &Records, current_boot: &str) {
    let own = format!("-{current_boot}{EVENT_SUFFIX}");
    let names: Vec<String> = records
        .event_names()
        .into_iter()
        .filter(|name| !name.ends_with(&own))
        .collect();
    let others_kept = MAX_EVENTS.saturating_sub(1);
    let excess = names.len().saturating_sub(others_kept);
    for name in names.iter().take(excess) {
        let _ = std::fs::remove_file(records.evidence_dir().join(name));
    }
}

/// Who may read a record file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Readers {
    /// 0644: the setting, the last boot, the summary.
    Everyone,
    /// 0640 root:adm: an event with its journal excerpts.
    Administrators,
}

/// The `adm` group's id, when it exists.
fn evidence_gid() -> Option<libc::gid_t> {
    let name = std::ffi::CString::new(EVIDENCE_GROUP).ok()?;
    // SAFETY: a valid C string; the returned pointer is read at once and
    // not kept.
    let group = unsafe { libc::getgrnam(name.as_ptr()) };
    // SAFETY: non-null pointers from getgrnam point at a valid group.
    (!group.is_null()).then(|| unsafe { (*group).gr_gid })
}

/// Hands `path` to root:adm when this runs as root. Elsewhere (a test) the
/// mode alone is set.
fn give_to_administrators(path: &Path) -> std::io::Result<()> {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        return Ok(());
    }
    let gid = evidence_gid().ok_or_else(|| std::io::Error::other("there is no adm group"))?;
    std::os::unix::fs::lchown(path, Some(0), Some(gid))
}

/// Writes `value` as JSON for `readers`, replacing the file whole. The
/// temporary file is created new and never through a symbolic link, and the
/// rename replaces a link rather than following it. Both writers of the
/// records directory, the login screen watch (ADR 0033) and the Better OS
/// login supervisor (ticket 267), write through here.
pub fn write_record<T: serde::Serialize>(
    path: &Path,
    value: &T,
    readers: Readers,
) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mode = match readers {
        Readers::Everyone => 0o644,
        Readers::Administrators => 0o640,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("record")
    ));
    match std::fs::remove_file(&temporary) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let json = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(mode)
        .open(&temporary)?;
    file.write_all(&json)?;
    file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    if readers == Readers::Administrators {
        give_to_administrators(&temporary)?;
    }
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

/// Writes an event into the evidence directory (0750 root:adm, the file
/// 0640), then the public summary of every kept event.
pub fn write_event(records: &Records, path: &Path, event: &Event) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let evidence = records.evidence_dir();
    std::fs::create_dir_all(&evidence)?;
    let meta = std::fs::symlink_metadata(&evidence)?;
    if !meta.is_dir() {
        return Err(std::io::Error::other(
            "the evidence directory is not a directory",
        ));
    }
    std::fs::set_permissions(&evidence, std::fs::Permissions::from_mode(0o750))?;
    give_to_administrators(&evidence)?;
    write_record(path, event, Readers::Administrators)?;
    write_summary(records)
}

/// Rewrites the public summary from the evidence directory.
pub fn write_summary(records: &Records) -> std::io::Result<()> {
    write_record(
        &records.summary_path(),
        &Summary::of(&records.events()),
        Readers::Everyone,
    )
}

/// The newest event the Better OS login supervisor wrote, for
/// `better-os-login-status`; the summary when the evidence cannot be read.
pub fn latest_supervisor_event(records: &Records) -> Option<Event> {
    records
        .read_events()
        .events
        .into_iter()
        .find(|event| event.display_manager == LOGIN_SUPERVISOR_UNIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn the_supervisor_outcomes_round_trip_and_name_their_stage() {
        for restart in [
            RestartOutcome::LoginScreenRestarted {
                stage: LoginStage::DisplayServer,
                restarts: 2,
                after_seconds: 40,
            },
            RestartOutcome::TextLoginUsed {
                stage: LoginStage::LoginManager,
                tty: "tty2".to_string(),
                after_seconds: 75,
            },
        ] {
            let mut record = event(5, BOOT);
            record.display_manager = LOGIN_SUPERVISOR_UNIT.to_string();
            record.restart = restart;
            let json = serde_json::to_string(&record).unwrap();
            assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), record);
        }
        let json = serde_json::to_string(&RestartOutcome::TextLoginUsed {
            stage: LoginStage::SignInService,
            tty: "tty1".to_string(),
            after_seconds: 1,
        })
        .unwrap();
        assert!(json.contains(r#""result":"text_login_used""#), "{json}");
        assert!(json.contains(r#""stage":"sign_in_service""#), "{json}");
        // A stage outside the closed set is refused, not guessed at.
        assert!(
            serde_json::from_str::<RestartOutcome>(
                r#"{"result":"text_login_used","stage":"bios","tty":"tty1","after_seconds":1}"#
            )
            .is_err()
        );
        for stage in LoginStage::ALL {
            assert!(!stage.english().is_empty() && !stage.zh_tw().is_empty());
        }
    }

    #[test]
    fn the_newest_supervisor_event_is_found_beside_the_watchs_own() {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path());
        let mut watch = event(300, BOOT);
        watch.display_manager = "gdm.service".to_string();
        let mut older = event(100, BOOT);
        older.display_manager = LOGIN_SUPERVISOR_UNIT.to_string();
        let mut newer = event(200, BOOT);
        newer.display_manager = LOGIN_SUPERVISOR_UNIT.to_string();
        for record in [&watch, &older, &newer] {
            write_event(
                &records,
                &records.event_path(record.detected_at_unix_seconds, BOOT),
                record,
            )
            .unwrap();
        }
        assert_eq!(latest_supervisor_event(&records), Some(newer));
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&records.evidence_dir()), 0o750);
        assert_eq!(mode(&records.event_path(100, BOOT)), 0o640);
        assert_eq!(mode(&records.summary_path()), 0o644);
    }

    fn event(at: u64, boot: &str) -> Event {
        Event {
            version: RECORD_VERSION,
            boot_id: boot.to_string(),
            detected_at_unix_seconds: at,
            display_manager: "gdm.service".to_string(),
            waited_seconds: 30,
            evidence: Evidence::default(),
            restart: RestartOutcome::Recovered { after_seconds: 5 },
        }
    }

    #[test]
    fn the_setting_is_on_until_the_file_says_otherwise_and_unreadable_is_not_on() {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path());
        assert_eq!(records.setting(), Setting::OnByDefault);
        std::fs::write(records.settings_path(), r#"{"enabled":false}"#).unwrap();
        assert_eq!(records.setting(), Setting::Off);
        std::fs::write(records.settings_path(), r#"{"enabled":true}"#).unwrap();
        assert_eq!(records.setting(), Setting::On);
        std::fs::write(records.settings_path(), r#"{"enabled":true,"x":1}"#).unwrap();
        assert_eq!(records.setting(), Setting::Unreadable);
        assert!(!Setting::Unreadable.watches());
        assert!(Setting::OnByDefault.watches());
    }

    #[test]
    fn events_are_read_newest_first_and_stray_or_oversized_files_are_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path());
        std::fs::create_dir_all(records.evidence_dir()).unwrap();
        for at in [100, 300, 200] {
            let path = records.event_path(at, BOOT);
            std::fs::write(path, serde_json::to_vec(&event(at, BOOT)).unwrap()).unwrap();
        }
        std::fs::write(records.evidence_dir().join("notes.json"), "{}").unwrap();
        std::fs::write(records.event_path(400, BOOT), "not json").unwrap();
        let big = records.event_path(500, BOOT);
        std::fs::write(&big, vec![b' '; MAX_EVENT_BYTES as usize + 1]).unwrap();
        let shown: Vec<u64> = records
            .events()
            .iter()
            .map(|e| e.detected_at_unix_seconds)
            .collect();
        assert_eq!(shown, [300, 200, 100]);
        assert!(records.has_event_for(BOOT));
        assert!(!records.has_event_for("ffffffffffffffffffffffffffffffff"));
    }

    #[test]
    fn without_the_evidence_directory_a_reader_gets_the_summary_and_no_journal() {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path());
        let mut full = event(100, BOOT);
        full.evidence.display_manager_journal = "gdm: secret line".to_string();
        let summary = Summary::of(std::slice::from_ref(&full));
        assert!(
            summary.events[0]
                .evidence
                .display_manager_journal
                .is_empty()
        );
        std::fs::write(
            records.summary_path(),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        // No evidence directory at all stands for one this reader may not open.
        let read = records.read_events();
        assert!(!read.evidence_readable);
        assert_eq!(read.events.len(), 1);
        assert_eq!(read.events[0].restart, full.restart);
        assert!(read.events[0].evidence.display_manager_journal.is_empty());

        std::fs::create_dir_all(records.evidence_dir()).unwrap();
        std::fs::write(
            records.event_path(100, BOOT),
            serde_json::to_vec(&full).unwrap(),
        )
        .unwrap();
        let read = records.read_events();
        assert!(read.evidence_readable);
        assert_eq!(
            read.events[0].evidence.display_manager_journal,
            "gdm: secret line"
        );
    }

    #[test]
    fn a_pending_record_from_an_earlier_boot_is_interrupted() {
        let mut pending = event(100, BOOT);
        pending.restart = RestartOutcome::Pending;
        assert!(interrupted(
            &pending,
            Some("ffffffffffffffffffffffffffffffff")
        ));
        // This boot's own pending record is still running, not interrupted.
        assert!(!interrupted(&pending, Some(BOOT)));
        assert!(!interrupted(&pending, None));
        assert!(!interrupted(
            &event(100, BOOT),
            Some("ffffffffffffffffffffffffffffffff")
        ));
    }

    #[test]
    fn a_boot_named_by_the_last_boot_record_is_handled() {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path());
        assert!(!records.handled(BOOT));
        let last = LastBoot {
            version: RECORD_VERSION,
            boot_id: BOOT.to_string(),
            checked_at_unix_seconds: 1,
            outcome: LastBootOutcome::PersonPresent,
        };
        std::fs::write(records.last_boot_path(), serde_json::to_vec(&last).unwrap()).unwrap();
        assert!(records.handled(BOOT));
        assert!(!records.handled("ffffffffffffffffffffffffffffffff"));
    }

    #[test]
    fn a_tail_keeps_whole_lines_and_says_what_it_dropped() {
        assert_eq!(tail("short", 10), "short");
        let text = "one\ntwo\nthree\nfour\n";
        assert_eq!(tail(text, 9), "[14 earlier bytes not kept]\nfour\n");
        // Never cut inside a character.
        let wide = "前前前前\n後\n";
        assert!(tail(wide, 5).ends_with("後\n"));
    }

    #[test]
    fn which_sessions_count_as_a_person_and_as_the_login_screen() {
        let session = |class: &str, kind: &str, seat: &str| SessionSeen {
            id: "c1".to_string(),
            class: class.to_string(),
            kind: kind.to_string(),
            seat: seat.to_string(),
        };
        assert!(session("greeter", "wayland", "seat0").is_login_screen());
        assert!(!session("greeter", "wayland", "seat0").is_person());
        assert!(session("user", "tty", "").is_person());
        assert!(session("user-early", "wayland", "seat0").is_person());
        assert!(session("lock-screen", "x11", "seat0").is_person());
        assert!(!session("background", "unspecified", "").is_person());
        assert!(!session("greeter", "wayland", "seat1").is_login_screen());
        assert!(session("user", "x11", "seat0").is_login_screen());
        assert!(!session("manager", "unspecified", "").is_login_screen());
    }

    #[test]
    fn the_last_boot_record_round_trips_with_its_outcome_inline() {
        let record = LastBoot {
            version: RECORD_VERSION,
            boot_id: BOOT.to_string(),
            checked_at_unix_seconds: 7,
            outcome: LastBootOutcome::LoginScreenAppeared { after_seconds: 4 },
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(
            json.contains(r#""outcome":"login_screen_appeared""#),
            "{json}"
        );
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path());
        std::fs::write(records.last_boot_path(), json).unwrap();
        assert_eq!(records.last_boot(), Some(record));
    }
}
