//! Versioned persistence for `awake-service`.
//!
//! The crate owns three files, each with its own schema stamp so one can be
//! migrated without touching the others: `awake-service-state.json` here, the
//! user's automatic rules in [`rules`], and the session history in [`history`].
//! All three live in the same per-user state directory, so an uninstall that
//! offers to remove user data has one place to look.
//!
//! The service is the only writer, so this store carries no revision fencing.
//! What it does carry is the reason it exists at all: after a crash, the next
//! run must be able to say that a session was interrupted rather than pretend
//! nothing happened. A run is written open on startup and closed on clean
//! shutdown, so an open run in a file that is being read at startup is, by
//! itself, the evidence of an interrupted session.
//!
//! Reading follows `manager-store`'s discipline: the schema stamp is read
//! before the document is deserialized, a newer schema is preserved and
//! refused, and a malformed current-schema file is moved aside rather than
//! silently reset.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use awake_core::{EndCondition, SessionOrigin, SessionPolicy};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod history;
pub mod rules;

pub use history::{
    HISTORY_FILE_NAME, HISTORY_SCHEMA_VERSION, History, HistoryDocument, HistoryEntry, HistoryLoad,
    HistoryStore, MAX_HISTORY_ENTRIES, MAX_HISTORY_REASON_CHARS, REDACTION_MARKER, StartedSession,
    redact_reason,
};
pub use rules::{RULES_FILE_NAME, RULES_SCHEMA_VERSION, RulesDocument, RulesLoad, RulesStore};

/// The only schema this crate writes.
///
/// There is no earlier version in the field yet, so there is no migration to
/// perform — only the forward-compatibility rule that a newer file is kept and
/// refused. `migrate` is the seam the first real migration lands in.
pub const STATE_SCHEMA_VERSION: u32 = 1;

pub const STATE_FILE_NAME: &str = "awake-service-state.json";

/// A session as it was recorded, including how it ended if it did.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedSession {
    pub session_id: u64,
    pub reason: String,
    pub origin: SessionOrigin,
    pub policy: SessionPolicy,
    #[serde(default)]
    pub battery_stop_percent: Option<u8>,
    pub end: EndCondition,
    pub started_at_unix_seconds: u64,
    /// Absent while the session is still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_unix_seconds: Option<u64>,
    /// A stable `EndCause` key, absent while the session is still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_cause: Option<String>,
}

impl PersistedSession {
    pub fn is_running(&self) -> bool {
        self.ended_at_unix_seconds.is_none()
    }
}

/// One lifetime of the service process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub started_at_unix_seconds: u64,
    /// Refreshed while the service is alive, so an interrupted session can say
    /// when it was last known to be holding an inhibitor.
    pub last_seen_unix_seconds: u64,
    /// Set only by a clean shutdown. Its absence in a file being loaded at
    /// startup is what "the previous run was interrupted" means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shut_down_at_unix_seconds: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceState {
    pub schema_version: u32,
    pub run: RunRecord,
    /// Active sessions, plus the recently ended ones kept for the interrupted
    /// report. Bounded so a long-lived service does not grow a history it was
    /// never asked to keep; the full history is ticket 26's.
    #[serde(default)]
    pub sessions: Vec<PersistedSession>,
    /// Whether the user has accepted the security consequence of a session that
    /// keeps the display on or stops automatic locking.
    #[serde(default)]
    pub reduced_security_confirmed: bool,
}

/// How many ended sessions are kept alongside the running ones.
pub const MAX_RETAINED_SESSIONS: usize = 16;

impl ServiceState {
    pub fn new(now_unix_seconds: u64) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            run: RunRecord {
                started_at_unix_seconds: now_unix_seconds,
                last_seen_unix_seconds: now_unix_seconds,
                shut_down_at_unix_seconds: None,
            },
            sessions: Vec::new(),
            reduced_security_confirmed: false,
        }
    }

    /// Drops the oldest ended sessions once the record grows past its bound.
    pub fn trim(&mut self) {
        while self.sessions.len() > MAX_RETAINED_SESSIONS {
            let Some(oldest_ended) = self
                .sessions
                .iter()
                .position(|session| !session.is_running())
            else {
                break;
            };
            self.sessions.remove(oldest_ended);
        }
    }
}

/// A session the previous run never closed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterruptedSession {
    pub session_id: u64,
    pub reason: String,
    pub started_at_unix_seconds: u64,
    /// The last moment the crashed run recorded itself as alive.
    pub last_seen_unix_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadOutcome {
    pub state: ServiceState,
    /// A malformed current-schema file is moved here before a default state is
    /// returned, so nothing is destroyed to keep the service starting.
    pub recovered_corrupt_state: Option<PathBuf>,
    /// Sessions the previous run left open. Empty after a clean shutdown.
    pub interrupted: Vec<InterruptedSession>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("awake.store.error.io:{path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("awake.store.error.unsupported_schema:{version}")]
    UnsupportedSchema { path: PathBuf, version: u32 },
    #[error("awake.store.error.serialize")]
    Serialize(#[source] serde_json::Error),
}

enum DecodeError {
    Invalid,
    UnsupportedSchema(u32),
}

/// The migration seam.
///
/// Version 1 is the first schema shipped, so there is nothing to raise yet. A
/// future version lands its step here rather than in `load`, where the
/// forward-compatibility refusal lives.
fn migrate(
    value: serde_json::Value,
    schema_version: u32,
) -> Result<serde_json::Value, DecodeError> {
    match schema_version {
        STATE_SCHEMA_VERSION => Ok(value),
        other => Err(DecodeError::UnsupportedSchema(other)),
    }
}

#[derive(Clone, Debug)]
pub struct JsonStore {
    path: PathBuf,
}

impl JsonStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_STATE_HOME/better-awake/awake-service-state.json`, falling back to
    /// `~/.local/state`. Nothing here is privileged or shared between users.
    pub fn from_default_path() -> Self {
        Self::at_path(default_state_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the previous state and reports what the previous run left behind.
    ///
    /// A missing file is a first run, not an error. A newer schema is an error,
    /// because overwriting a file a newer Better Awake wrote would destroy it.
    pub fn load(&self, now_unix_seconds: u64) -> Result<LoadOutcome, StoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadOutcome {
                    state: ServiceState::new(now_unix_seconds),
                    recovered_corrupt_state: None,
                    interrupted: Vec::new(),
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
            Ok(previous) => {
                let interrupted = interrupted_sessions(&previous);
                let mut state = previous;
                state.run = RunRecord {
                    started_at_unix_seconds: now_unix_seconds,
                    last_seen_unix_seconds: now_unix_seconds,
                    shut_down_at_unix_seconds: None,
                };
                // The previous run's sessions are not resumed: whatever
                // inhibitor they held died with the process, so re-listing them
                // as active would claim protection that is not there. They stay
                // in the record marked ended, which is what the report explains.
                let ended_at = interrupted
                    .iter()
                    .map(|session| session.last_seen_unix_seconds)
                    .max();
                for session in &mut state.sessions {
                    if session.is_running() {
                        session.ended_at_unix_seconds = ended_at.or(Some(now_unix_seconds));
                        session.end_cause = Some("interrupted".to_string());
                    }
                }
                state.trim();
                Ok(LoadOutcome {
                    state,
                    recovered_corrupt_state: None,
                    interrupted,
                })
            }
            Err(DecodeError::UnsupportedSchema(version)) => Err(StoreError::UnsupportedSchema {
                path: self.path.clone(),
                version,
            }),
            Err(DecodeError::Invalid) => {
                let backup = self.backup_corrupt()?;
                Ok(LoadOutcome {
                    state: ServiceState::new(now_unix_seconds),
                    recovered_corrupt_state: Some(backup),
                    interrupted: Vec::new(),
                })
            }
        }
    }

    /// Writes through a temporary file and renames, so a crash mid-write leaves
    /// the previous state readable rather than a half-written one.
    pub fn save(&self, state: &ServiceState) -> Result<(), StoreError> {
        let mut state = state.clone();
        state.schema_version = STATE_SCHEMA_VERSION;
        let document = serde_json::to_vec_pretty(&state).map_err(StoreError::Serialize)?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, &document).map_err(|source| StoreError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, &self.path).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })
    }

    fn decode(&self, bytes: &[u8]) -> Result<ServiceState, DecodeError> {
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
        if schema_version > STATE_SCHEMA_VERSION {
            return Err(DecodeError::UnsupportedSchema(schema_version));
        }
        let value = migrate(value, schema_version)?;
        serde_json::from_value::<ServiceState>(value).map_err(|_| DecodeError::Invalid)
    }

    fn backup_corrupt(&self) -> Result<PathBuf, StoreError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(STATE_FILE_NAME);
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

fn interrupted_sessions(state: &ServiceState) -> Vec<InterruptedSession> {
    if state.run.shut_down_at_unix_seconds.is_some() {
        // The previous run said goodbye, so it released what it held.
        return Vec::new();
    }
    state
        .sessions
        .iter()
        .filter(|session| session.is_running())
        .map(|session| InterruptedSession {
            session_id: session.session_id,
            reason: session.reason.clone(),
            started_at_unix_seconds: session.started_at_unix_seconds,
            last_seen_unix_seconds: state.run.last_seen_unix_seconds,
        })
        .collect()
}

fn default_state_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("better-awake").join(STATE_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(id: u64, started: u64) -> PersistedSession {
        PersistedSession {
            session_id: id,
            reason: "Android Studio build is running".to_string(),
            origin: SessionOrigin::Manual,
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
            end: EndCondition::Indefinite,
            started_at_unix_seconds: started,
            ended_at_unix_seconds: None,
            end_cause: None,
        }
    }

    fn store() -> (tempfile::TempDir, JsonStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonStore::at_path(directory.path().join(STATE_FILE_NAME));
        (directory, store)
    }

    #[test]
    fn a_first_run_starts_from_nothing_without_an_error() {
        let (_directory, store) = store();
        let outcome = store.load(1_000).unwrap();
        assert!(outcome.state.sessions.is_empty());
        assert!(outcome.interrupted.is_empty());
        assert_eq!(outcome.recovered_corrupt_state, None);
        assert_eq!(outcome.state.schema_version, STATE_SCHEMA_VERSION);
    }

    #[test]
    fn state_survives_a_save_and_load() {
        let (_directory, store) = store();
        let mut state = ServiceState::new(1_000);
        state.sessions.push(running(1, 1_000));
        state.reduced_security_confirmed = true;
        state.run.shut_down_at_unix_seconds = Some(2_000);
        store.save(&state).unwrap();

        let outcome = store.load(3_000).unwrap();
        assert_eq!(outcome.state.sessions.len(), 1);
        assert!(outcome.state.reduced_security_confirmed);
        assert_eq!(outcome.state.run.started_at_unix_seconds, 3_000);
        assert_eq!(outcome.state.run.shut_down_at_unix_seconds, None);
    }

    #[test]
    fn a_clean_shutdown_leaves_no_interrupted_session_to_explain() {
        let (_directory, store) = store();
        let mut state = ServiceState::new(1_000);
        let mut session = running(1, 1_000);
        session.ended_at_unix_seconds = Some(1_500);
        session.end_cause = Some("service_shutdown".to_string());
        state.sessions.push(session);
        state.run.shut_down_at_unix_seconds = Some(1_500);
        store.save(&state).unwrap();

        assert!(store.load(2_000).unwrap().interrupted.is_empty());
    }

    #[test]
    fn a_crashed_run_is_reported_with_the_session_it_left_open() {
        let (_directory, store) = store();
        let mut state = ServiceState::new(1_000);
        state.run.last_seen_unix_seconds = 1_800;
        state.sessions.push(running(7, 1_000));
        store.save(&state).unwrap();

        let outcome = store.load(9_000).unwrap();
        assert_eq!(
            outcome.interrupted,
            vec![InterruptedSession {
                session_id: 7,
                reason: "Android Studio build is running".to_string(),
                started_at_unix_seconds: 1_000,
                last_seen_unix_seconds: 1_800,
            }]
        );
    }

    #[test]
    fn an_interrupted_session_is_not_resumed_as_active() {
        let (_directory, store) = store();
        let mut state = ServiceState::new(1_000);
        state.run.last_seen_unix_seconds = 1_800;
        state.sessions.push(running(7, 1_000));
        store.save(&state).unwrap();

        // The inhibitor died with the process, so the record must not come back
        // claiming the machine is still protected.
        let outcome = store.load(9_000).unwrap();
        assert!(outcome.state.sessions.iter().all(|s| !s.is_running()));
        assert_eq!(
            outcome.state.sessions[0].end_cause.as_deref(),
            Some("interrupted")
        );
        assert_eq!(outcome.state.sessions[0].ended_at_unix_seconds, Some(1_800));
    }

    #[test]
    fn a_newer_schema_is_preserved_and_refused_rather_than_reset() {
        let (_directory, store) = store();
        std::fs::write(
            store.path(),
            br#"{"schema_version":99,"run":{"started_at_unix_seconds":1}}"#,
        )
        .unwrap();

        let error = store.load(1_000).unwrap_err();
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

        let outcome = store.load(1_000).unwrap();
        let backup = outcome
            .recovered_corrupt_state
            .expect("the unreadable file must be kept");
        assert!(backup.exists());
        assert!(outcome.state.sessions.is_empty());
        assert!(!store.path().exists());
    }

    #[test]
    fn a_file_with_no_schema_stamp_is_treated_as_corrupt_not_as_version_one() {
        let (_directory, store) = store();
        std::fs::write(store.path(), br#"{"sessions":[]}"#).unwrap();
        assert!(store.load(1_000).unwrap().recovered_corrupt_state.is_some());
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
    fn ended_sessions_are_trimmed_but_running_ones_are_kept() {
        let mut state = ServiceState::new(1_000);
        for id in 0..(MAX_RETAINED_SESSIONS as u64 + 4) {
            let mut session = running(id, 1_000 + id);
            session.ended_at_unix_seconds = Some(2_000);
            state.sessions.push(session);
        }
        state.sessions.push(running(999, 3_000));
        state.trim();

        assert_eq!(state.sessions.len(), MAX_RETAINED_SESSIONS);
        assert!(
            state
                .sessions
                .iter()
                .any(|session| session.session_id == 999)
        );
        assert!(
            !state.sessions.iter().any(|session| session.session_id == 0),
            "the oldest ended session is the one that goes"
        );
    }

    #[test]
    fn a_default_path_stays_inside_the_users_own_state_directory() {
        let path = default_state_path();
        assert!(path.ends_with(Path::new("better-awake").join(STATE_FILE_NAME)));
    }
}
