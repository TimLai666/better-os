//! The login screen watch's records (ADR 0033, ticket 140).
//!
//! `better-greeter-watch.service` runs once per boot as root and writes these
//! files under [`DIR`]; Better Manager's window and command line read them
//! unprivileged, because every file is 0644. The privileged service writes
//! [`SETTINGS_FILE`] when the person turns the watch off or on.
//!
//! Everything here is read as untrusted input: a file larger than its limit is
//! skipped before it is parsed, unknown fields are refused, and a file that
//! does not parse is left out of the list rather than guessed at.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where the records live.
pub const DIR: &str = "/var/lib/better-os/boot-watch";

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
    /// The login screen appeared between the deadline and the restart, so
    /// nothing was restarted.
    AppearedBeforeRestart,
}

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

    /// The file an event of `boot_id` detected at `at` is written to.
    pub fn event_path(&self, at: u64, boot_id: &str) -> PathBuf {
        self.dir.join(format!("{at:020}-{boot_id}{EVENT_SUFFIX}"))
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
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
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

    /// Every readable event, newest first.
    pub fn events(&self) -> Vec<Event> {
        let mut events: Vec<Event> = self
            .event_names()
            .iter()
            .filter_map(|name| read_capped(&self.dir.join(name), MAX_EVENT_BYTES))
            .filter_map(|bytes| serde_json::from_slice::<Event>(&bytes).ok())
            .filter(|event| event.version == RECORD_VERSION)
            .collect();
        events.sort_by_key(|event| std::cmp::Reverse(event.detected_at_unix_seconds));
        events
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = "0123456789abcdef0123456789abcdef";

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
        for at in [100, 300, 200] {
            let path = records.event_path(at, BOOT);
            std::fs::write(path, serde_json::to_vec(&event(at, BOOT)).unwrap()).unwrap();
        }
        std::fs::write(dir.path().join("notes.json"), "{}").unwrap();
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
