//! Versioned persistence for the session history.
//!
//! Ticket 26 asks history to answer one question honestly: why was this machine
//! awake, and what stopped it. That means start and end time, manual or trigger
//! origin, every active reason, the effective policy, the stop cause, backend
//! failures, and battery safety stops — and nothing else. In particular it
//! records "no sensitive command-line arguments or arbitrary process data by
//! default", which is why every reason on its way into an entry passes through
//! [`redact_reason`] rather than being trusted because of where it came from.
//!
//! Reading follows the same discipline as the rest of the crate: the schema
//! stamp is read before the document is deserialized, a newer schema is
//! preserved and refused, and a malformed current-schema file is moved aside
//! rather than silently reset.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use awake_core::{SessionOrigin, SessionPolicy};
use serde::{Deserialize, Serialize};

use crate::StoreError;

/// The only history schema this crate writes.
///
/// There is no earlier version in the field yet, so there is no migration to
/// perform — only the forward-compatibility rule that a newer file is kept and
/// refused. [`migrate`] is the seam the first real migration lands in.
pub const HISTORY_SCHEMA_VERSION: u32 = 1;

/// The file name the history lives in, under the same directory as the service
/// state so an uninstall that removes user data has one place to look.
pub const HISTORY_FILE_NAME: &str = "awake-history.json";

/// How many sessions the history keeps.
///
/// Issue #13 defers the history *retention duration* to an ADR, and ticket 26
/// repeats that deferral. A bounded count is what ships in its place, because a
/// count is the only honest bound available while no duration has been decided:
/// it needs no policy answer, it holds whatever the ADR eventually chooses
/// without a migration, and it is the one guarantee that can be made without
/// pretending to know how long a user wants their history kept. An unbounded
/// file was not an option — the service writes an entry per session for as long
/// as it is installed, so "no limit" means a file that grows forever.
///
/// Five hundred entries is months of ordinary use and a few hundred kilobytes.
/// When the ADR lands, a duration is applied on top of this cap rather than
/// instead of it.
pub const MAX_HISTORY_ENTRIES: usize = 500;

/// The longest a stored reason may be, matching `awake_core::MAX_REASON_CHARS`.
///
/// A reason that reached a session was already bounded at this length, so a
/// longer one arriving here came from somewhere that did not validate it and is
/// truncated rather than trusted.
pub const MAX_HISTORY_REASON_CHARS: usize = awake_core::MAX_REASON_CHARS;

/// What replaces the part of a reason that was cut away. A fixed marker, so a
/// reader can tell redaction from a reason that simply reads that way.
pub const REDACTION_MARKER: &str = "[…]";

/// One recorded session.
///
/// Every field is either a bounded value the user chose or a stable machine
/// key. There is no field capable of carrying a command line, an environment,
/// or arbitrary process data, which is how ticket 26's privacy requirement is
/// enforced rather than merely intended.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub session_id: u64,
    pub started_at_unix_seconds: u64,
    /// Absent while the session is still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_unix_seconds: Option<u64>,
    pub origin: SessionOrigin,
    /// The rule that held it, when a rule did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<u64>,
    /// Every reason that was active while this session ran.
    ///
    /// Invariant: every string here has been through [`redact_reason`]. Nothing
    /// writes this field directly — [`HistoryEntry::record`] takes raw strings
    /// and redacts them, and [`History::record_start`] redacts again on the way
    /// in, so a reason cannot reach the file unredacted by a forgotten call.
    pub reasons: Vec<String>,
    pub effective_policy: SessionPolicy,
    #[serde(default)]
    pub battery_stop_percent: Option<u8>,
    /// A stable `EndCause` key such as `battery_threshold`, absent while the
    /// session is still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_cause: Option<String>,
    /// A stable key for a backend failure seen during this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_failure: Option<String>,
    /// The battery percentage a low-battery stop happened at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_stop_percent_at_stop: Option<u8>,
}

/// A session that has just started, in raw form.
///
/// This is the only way to build a [`HistoryEntry`], and its `reasons` are
/// deliberately raw: the caller does not decide whether to redact, because the
/// one caller who forgets is the one who leaks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartedSession {
    pub session_id: u64,
    pub started_at_unix_seconds: u64,
    pub origin: SessionOrigin,
    pub rule_id: Option<u64>,
    /// Raw, unredacted reasons. They are redacted by [`HistoryEntry::record`].
    pub reasons: Vec<String>,
    pub effective_policy: SessionPolicy,
    pub battery_stop_percent: Option<u8>,
}

impl HistoryEntry {
    /// Records a started session, redacting every reason on the way in.
    pub fn record(started: StartedSession) -> Self {
        Self {
            session_id: started.session_id,
            started_at_unix_seconds: started.started_at_unix_seconds,
            ended_at_unix_seconds: None,
            origin: started.origin,
            rule_id: started.rule_id,
            reasons: started
                .reasons
                .iter()
                .map(|reason| redact_reason(reason))
                .collect(),
            effective_policy: started.effective_policy,
            battery_stop_percent: started.battery_stop_percent,
            end_cause: None,
            backend_failure: None,
            battery_stop_percent_at_stop: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.ended_at_unix_seconds.is_none()
    }
}

/// Removes everything from a reason that could carry data the user never asked
/// to store.
///
/// A reason arrives from a rule name, a tray preset, or a client, and any of
/// those can end up holding whatever a person pasted into a text field. Four
/// rules apply, in order:
///
/// 1. Control characters become spaces and runs of whitespace collapse to one,
///    so a reason cannot smuggle a second line into a log or a menu.
/// 2. The reason is cut at the first whitespace-separated token that starts with
///    `-`. That token is the beginning of a command line, and the argument after
///    a flag is exactly where a password lands: `myapp --password hunter2`.
/// 3. It is cut at the first token containing `/`. A path names the document
///    someone is working on, and `Uploading
///    /home/tim/Documents/tax-return-2024.pdf` says far more about the user than
///    "Uploading" does.
/// 4. It is cut at the first token containing `=`, which is the shape of an
///    environment pair: `build AWS_SECRET_KEY=abc123` must not keep the value.
///
/// Whatever is cut is replaced with [`REDACTION_MARKER`], so the entry says that
/// something was removed instead of pretending the reason ended there. Finally
/// the result is truncated to [`MAX_HISTORY_REASON_CHARS`] with a trailing `…`.
///
/// A reason with none of those shapes — "Android Studio build is running" — is
/// returned unchanged. Over-redaction that mangles every ordinary reason would
/// make the history useless, which is a failure of its own.
///
/// Rule 3 has no exception for a reason that is a single token. An earlier
/// version kept one, on the grounds that a lone slashed token is usually a name
/// like `24/7` or `N/A` and cutting it leaves an entry explaining nothing. That
/// exception also kept a reason consisting of nothing but a bare path — exactly
/// the leak the rule exists to stop, and the likelier of the two by far, since
/// nobody names a keep-awake session `N/A`. The cosmetic case loses.
pub fn redact_reason(raw: &str) -> String {
    let normalized: String = raw
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();

    let tokens: Vec<&str> = normalized.split_whitespace().collect();

    let mut kept: Vec<&str> = Vec::with_capacity(tokens.len());
    let mut cut = false;
    for token in tokens {
        if is_sensitive_token(token) {
            cut = true;
            break;
        }
        kept.push(token);
    }

    let mut redacted = kept.join(" ");
    if cut {
        if !redacted.is_empty() {
            redacted.push(' ');
        }
        redacted.push_str(REDACTION_MARKER);
    }

    if redacted.chars().count() > MAX_HISTORY_REASON_CHARS {
        // One character is spent on the ellipsis so the whole string, marker
        // included, still fits the documented bound.
        redacted = redacted
            .chars()
            .take(MAX_HISTORY_REASON_CHARS - 1)
            .collect::<String>();
        redacted.push('…');
    }
    redacted
}

fn is_sensitive_token(token: &str) -> bool {
    token.starts_with('-') || token.contains('=') || token.contains('/')
}

/// Every recorded session, oldest first.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    #[serde(default)]
    entries: Vec<HistoryEntry>,
}

impl History {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Every entry, oldest first.
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// The newest `limit` entries, still oldest first. Asking for more than
    /// there are returns everything rather than failing.
    pub fn recent(&self, limit: usize) -> &[HistoryEntry] {
        let start = self.entries.len().saturating_sub(limit);
        &self.entries[start..]
    }

    /// Appends a started session and enforces the retention cap immediately.
    ///
    /// Reasons are redacted again here rather than trusted: [`HistoryEntry`] has
    /// public fields, so this is the last point at which a reason that was set
    /// by hand can still be caught. [`redact_reason`] leaves an already-redacted
    /// string alone, so the second pass costs nothing.
    pub fn record_start(&mut self, mut entry: HistoryEntry) {
        for reason in &mut entry.reasons {
            *reason = redact_reason(reason);
        }
        self.entries.push(entry);
        self.trim();
    }

    /// Closes the running entry for this session.
    ///
    /// Only a running entry is closed, and only the one naming this session, so
    /// a reused session id cannot rewrite the end of an older record. Returns
    /// whether an entry was closed.
    pub fn record_end(
        &mut self,
        session_id: u64,
        ended_at_unix_seconds: u64,
        end_cause: &str,
    ) -> bool {
        let Some(entry) = self.running_entry_mut(session_id) else {
            return false;
        };
        entry.ended_at_unix_seconds = Some(ended_at_unix_seconds);
        entry.end_cause = Some(end_cause.to_string());
        true
    }

    /// Records that the inhibitor backend failed during this session.
    ///
    /// Kept apart from the end cause because a session can survive a backend
    /// failure and end for an entirely different reason, and a history that
    /// conflated the two would misexplain the machine.
    pub fn record_backend_failure(&mut self, session_id: u64, failure_key: &str) -> bool {
        let Some(entry) = self.running_entry_mut(session_id) else {
            return false;
        };
        entry.backend_failure = Some(failure_key.to_string());
        true
    }

    /// Records the battery percentage a low-battery stop happened at.
    pub fn record_battery_stop(&mut self, session_id: u64, percent: u8) -> bool {
        let Some(entry) = self.running_entry_mut(session_id) else {
            return false;
        };
        entry.battery_stop_percent_at_stop = Some(percent);
        true
    }

    /// Drops the oldest entries once the history grows past its bound.
    ///
    /// The oldest go first because the recent sessions are the ones a user is
    /// asking about when they open History at all.
    pub fn trim(&mut self) {
        if self.entries.len() > MAX_HISTORY_ENTRIES {
            let excess = self.entries.len() - MAX_HISTORY_ENTRIES;
            self.entries.drain(..excess);
        }
    }

    fn running_entry_mut(&mut self, session_id: u64) -> Option<&mut HistoryEntry> {
        self.entries
            .iter_mut()
            .rev()
            .find(|entry| entry.session_id == session_id && entry.is_running())
    }
}

/// The stored history, with its schema stamp as a sibling of the payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryDocument {
    pub schema_version: u32,
    pub history: History,
}

impl HistoryDocument {
    pub fn new(history: History) -> Self {
        Self {
            schema_version: HISTORY_SCHEMA_VERSION,
            history,
        }
    }
}

/// What a load produced, and what it had to rescue to produce it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryLoad {
    pub history: History,
    /// A malformed current-schema file is moved here before an empty history is
    /// returned, so nothing is destroyed to keep the service starting.
    pub recovered_corrupt_state: Option<PathBuf>,
}

enum DecodeError {
    Invalid,
    UnsupportedSchema(u32),
}

/// The migration seam.
///
/// Version 1 is the first schema shipped, so there is nothing to raise yet. A
/// future version lands its step here rather than in [`HistoryStore::load`],
/// where the forward-compatibility refusal lives.
fn migrate(
    value: serde_json::Value,
    schema_version: u32,
) -> Result<serde_json::Value, DecodeError> {
    match schema_version {
        HISTORY_SCHEMA_VERSION => Ok(value),
        other => Err(DecodeError::UnsupportedSchema(other)),
    }
}

/// Reads and writes the session history.
#[derive(Clone, Debug)]
pub struct HistoryStore {
    path: PathBuf,
}

impl HistoryStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_STATE_HOME/better-awake/awake-history.json`, falling back to
    /// `~/.local/state`. Nothing here is privileged or shared between users.
    pub fn from_default_path() -> Self {
        Self::at_path(default_history_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the stored history.
    ///
    /// A missing file is a first run with no history, not an error. A newer
    /// schema is an error, because overwriting a file a newer Better Awake
    /// wrote would destroy records this version cannot even display.
    pub fn load(&self) -> Result<HistoryLoad, StoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(HistoryLoad {
                    history: History::new(),
                    recovered_corrupt_state: None,
                });
            }
            Err(source) => {
                return Err(StoreError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        };

        match self.decode(&bytes) {
            Ok(document) => Ok(HistoryLoad {
                history: document.history,
                recovered_corrupt_state: None,
            }),
            Err(DecodeError::UnsupportedSchema(version)) => Err(StoreError::UnsupportedSchema {
                path: self.path.clone(),
                version,
            }),
            Err(DecodeError::Invalid) => {
                let backup = self.backup_corrupt()?;
                Ok(HistoryLoad {
                    history: History::new(),
                    recovered_corrupt_state: Some(backup),
                })
            }
        }
    }

    /// Writes through a temporary file and renames, so a crash mid-write leaves
    /// the previous history readable rather than a half-written file.
    ///
    /// The retention cap is applied to the copy being written, so a caller that
    /// built a history by hand cannot save an unbounded file.
    pub fn save(&self, history: &History) -> Result<(), StoreError> {
        let mut history = history.clone();
        history.trim();
        let document = HistoryDocument::new(history);
        let bytes = serde_json::to_vec_pretty(&document).map_err(StoreError::Serialize)?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, &bytes).map_err(|source| StoreError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, &self.path).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })
    }

    fn decode(&self, bytes: &[u8]) -> Result<HistoryDocument, DecodeError> {
        // Read the stamp before the document: a newer writer may have added
        // required fields this version cannot parse, and that file must be
        // preserved rather than treated as corruption.
        let value =
            serde_json::from_slice::<serde_json::Value>(bytes).map_err(|_| DecodeError::Invalid)?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or(DecodeError::Invalid)?;
        if schema_version > HISTORY_SCHEMA_VERSION {
            return Err(DecodeError::UnsupportedSchema(schema_version));
        }
        let value = migrate(value, schema_version)?;
        serde_json::from_value::<HistoryDocument>(value).map_err(|_| DecodeError::Invalid)
    }

    fn backup_corrupt(&self) -> Result<PathBuf, StoreError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(HISTORY_FILE_NAME);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let backup = parent.join(format!("{name}.corrupt-{}-{nonce}", std::process::id()));
        fs::rename(&self.path, &backup).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        Ok(backup)
    }
}

fn default_history_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("better-awake").join(HISTORY_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(session_id: u64, started_at: u64) -> HistoryEntry {
        HistoryEntry::record(StartedSession {
            session_id,
            started_at_unix_seconds: started_at,
            origin: SessionOrigin::Manual,
            rule_id: None,
            reasons: vec!["Android Studio build is running".to_string()],
            effective_policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
        })
    }

    fn store() -> (tempfile::TempDir, HistoryStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = HistoryStore::at_path(directory.path().join(HISTORY_FILE_NAME));
        (directory, store)
    }

    // ---- The store --------------------------------------------------------

    #[test]
    fn a_first_run_starts_with_no_history_and_without_an_error() {
        let (_directory, store) = store();
        let loaded = store.load().unwrap();
        assert!(loaded.history.entries().is_empty());
        assert_eq!(loaded.recovered_corrupt_state, None);
    }

    #[test]
    fn history_survives_a_save_and_load() {
        let (_directory, store) = store();
        let mut history = History::new();
        history.record_start(HistoryEntry::record(StartedSession {
            session_id: 7,
            started_at_unix_seconds: 1_000,
            origin: SessionOrigin::Trigger,
            rule_id: Some(3),
            reasons: vec!["Build is running".to_string()],
            effective_policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
        }));
        history.record_backend_failure(7, "lease_missing");
        history.record_battery_stop(7, 19);
        history.record_end(7, 2_000, "battery_threshold");
        store.save(&history).unwrap();

        let loaded = store.load().unwrap().history;
        assert_eq!(loaded, history);
        let entry = &loaded.entries()[0];
        assert_eq!(entry.origin, SessionOrigin::Trigger);
        assert_eq!(entry.rule_id, Some(3));
        assert_eq!(entry.end_cause.as_deref(), Some("battery_threshold"));
        assert_eq!(entry.backend_failure.as_deref(), Some("lease_missing"));
        assert_eq!(entry.battery_stop_percent_at_stop, Some(19));
        assert_eq!(entry.ended_at_unix_seconds, Some(2_000));
    }

    #[test]
    fn a_newer_schema_is_preserved_and_refused_rather_than_reset() {
        let (_directory, store) = store();
        std::fs::write(store.path(), br#"{"schema_version":99,"history":{}}"#).unwrap();

        let error = store.load().unwrap_err();
        assert!(matches!(
            error,
            StoreError::UnsupportedSchema { version: 99, .. }
        ));
        assert!(
            store.path().exists(),
            "a file a newer Better Awake wrote must survive being refused"
        );
    }

    #[test]
    fn a_malformed_file_is_moved_aside_instead_of_blocking_startup() {
        let (_directory, store) = store();
        std::fs::write(store.path(), b"{ not json").unwrap();

        let loaded = store.load().unwrap();
        let backup = loaded
            .recovered_corrupt_state
            .expect("the unreadable history file must be kept");
        assert!(backup.exists());
        assert!(loaded.history.entries().is_empty());
        assert!(!store.path().exists());
    }

    #[test]
    fn a_file_with_no_schema_stamp_is_treated_as_corrupt_not_as_version_one() {
        let (_directory, store) = store();
        std::fs::write(store.path(), br#"{"history":{"entries":[]}}"#).unwrap();
        assert!(store.load().unwrap().recovered_corrupt_state.is_some());
    }

    #[test]
    fn the_migration_seam_refuses_a_version_it_does_not_know() {
        let value = serde_json::json!({"schema_version": 4});
        assert!(matches!(
            migrate(value, 4),
            Err(DecodeError::UnsupportedSchema(4))
        ));
    }

    #[test]
    fn a_default_path_stays_inside_the_users_own_state_directory() {
        let path = default_history_path();
        assert!(path.ends_with(Path::new("better-awake").join(HISTORY_FILE_NAME)));
    }

    // ---- The collection ---------------------------------------------------

    #[test]
    fn recording_an_end_closes_the_right_entry_and_leaves_the_others_open() {
        let mut history = History::new();
        history.record_start(started(1, 1_000));
        history.record_start(started(2, 1_100));
        history.record_start(started(3, 1_200));

        assert!(history.record_end(2, 1_500, "user_request"));

        assert!(history.entries()[0].is_running());
        assert_eq!(history.entries()[1].ended_at_unix_seconds, Some(1_500));
        assert_eq!(
            history.entries()[1].end_cause.as_deref(),
            Some("user_request")
        );
        assert!(history.entries()[2].is_running());
    }

    #[test]
    fn ending_a_session_that_was_never_started_changes_nothing() {
        let mut history = History::new();
        history.record_start(started(1, 1_000));
        assert!(!history.record_end(9, 2_000, "user_request"));
        assert!(history.entries()[0].is_running());
    }

    #[test]
    fn an_already_closed_entry_is_not_reopened_by_a_reused_session_id() {
        let mut history = History::new();
        history.record_start(started(1, 1_000));
        history.record_end(1, 1_500, "user_request");
        history.record_start(started(1, 2_000));
        history.record_end(1, 2_500, "expired");

        assert_eq!(
            history.entries()[0].end_cause.as_deref(),
            Some("user_request")
        );
        assert_eq!(history.entries()[1].end_cause.as_deref(), Some("expired"));
    }

    #[test]
    fn trimming_drops_the_oldest_entries_and_keeps_the_newest() {
        let mut history = History::new();
        for id in 0..(MAX_HISTORY_ENTRIES as u64 + 4) {
            history.record_start(started(id, 1_000 + id));
        }

        assert_eq!(history.entries().len(), MAX_HISTORY_ENTRIES);
        assert_eq!(
            history.entries()[0].session_id,
            4,
            "the four oldest sessions are the ones that go"
        );
        assert_eq!(
            history.entries().last().unwrap().session_id,
            MAX_HISTORY_ENTRIES as u64 + 3
        );
    }

    #[test]
    fn recent_returns_the_newest_entries_and_never_more_than_there_are() {
        let mut history = History::new();
        for id in 0..5 {
            history.record_start(started(id, 1_000 + id));
        }

        let recent: Vec<u64> = history
            .recent(2)
            .iter()
            .map(|entry| entry.session_id)
            .collect();
        assert_eq!(recent, vec![3, 4]);
        assert_eq!(history.recent(99).len(), 5);
    }

    // ---- Redaction --------------------------------------------------------

    #[test]
    fn a_command_line_argument_never_reaches_the_history_file() {
        let redacted = redact_reason("myapp --password hunter2");
        assert!(!redacted.contains("hunter2"), "{redacted}");
        assert!(!redacted.contains("--password"), "{redacted}");
        assert_eq!(redacted, "myapp […]");
    }

    #[test]
    fn a_path_that_names_a_document_is_cut_away() {
        let redacted = redact_reason("Uploading /home/tim/Documents/tax-return-2024.pdf");
        assert!(!redacted.contains("tax-return-2024"), "{redacted}");
        assert_eq!(redacted, "Uploading […]");
    }

    #[test]
    fn a_reason_that_is_nothing_but_a_path_is_still_cut() {
        // The rule has no exception for a lone token. An earlier version did,
        // and it let exactly this through: the commonest shape a leaked path
        // takes is a reason with nothing else in it.
        for bare in [
            "/home/tim/Documents/tax-return-2024.pdf",
            "~/Downloads/passport-scan.jpg",
            "smb://nas.local/private/salaries.ods",
        ] {
            let redacted = redact_reason(bare);
            assert_eq!(redacted, REDACTION_MARKER, "{bare} leaked as {redacted}");
        }
    }

    #[test]
    fn an_environment_pair_never_keeps_its_value() {
        let redacted = redact_reason("build AWS_SECRET_KEY=abc123");
        assert!(!redacted.contains("abc123"), "{redacted}");
        assert_eq!(redacted, "build […]");
    }

    #[test]
    fn an_ordinary_reason_survives_completely_unchanged() {
        // Over-redaction that mangles every reason is a failure of its own: a
        // history of "[…]" explains nothing.
        assert_eq!(
            redact_reason("Android Studio build is running"),
            "Android Studio build is running"
        );
        assert_eq!(redact_reason("保持清醒"), "保持清醒");
    }

    #[test]
    fn control_characters_and_whitespace_runs_collapse_to_single_spaces() {
        assert_eq!(redact_reason("two\nlines\there"), "two lines here");
        assert_eq!(redact_reason("  spaced    out  "), "spaced out");
    }

    #[test]
    fn a_reason_that_is_only_sensitive_becomes_the_marker_alone() {
        assert_eq!(redact_reason("--token=secret"), REDACTION_MARKER);
    }

    #[test]
    fn a_two_hundred_character_reason_is_truncated_to_the_documented_bound() {
        let redacted = redact_reason(&"a".repeat(200));
        assert!(redacted.chars().count() <= MAX_HISTORY_REASON_CHARS);
        assert_eq!(redacted.chars().count(), MAX_HISTORY_REASON_CHARS);
        assert!(redacted.ends_with('…'));
    }

    #[test]
    fn redacting_an_already_redacted_reason_changes_nothing_further() {
        // The second pass in `record_start` must be a safety net, not a
        // progressive shredder.
        for raw in [
            "Android Studio build is running",
            "myapp --password hunter2",
            "build AWS_SECRET_KEY=abc123",
            &"a".repeat(200),
        ] {
            let once = redact_reason(raw);
            assert_eq!(redact_reason(&once), once, "{raw}");
        }
    }

    #[test]
    fn a_reason_set_by_hand_is_still_redacted_when_it_is_recorded() {
        let mut entry = started(1, 1_000);
        entry.reasons = vec!["leak /home/tim/Secret Plans.txt".to_string()];
        let mut history = History::new();
        history.record_start(entry);

        assert_eq!(history.entries()[0].reasons, vec!["leak […]".to_string()]);
    }
}
