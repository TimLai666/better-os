//! Versioned persistence for the automatic trigger rules.
//!
//! Rules are the one thing in Better Awake a user builds by hand, so losing
//! them is worse than failing to start: this store never overwrites a file it
//! cannot understand. It follows the same discipline as the service state
//! store — the schema stamp is read before the document is deserialized, a
//! newer schema is preserved and refused, and a malformed current-schema file
//! is moved aside rather than silently reset.
//!
//! The rules themselves stay validated by `awake-core`. Nothing here can widen
//! what a condition may contain, because every operand type refuses an illegal
//! value on the way out of `serde` as well as on the way in.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use awake_core::RuleSet;
use serde::{Deserialize, Serialize};

use crate::StoreError;

/// The only rules schema this crate writes.
///
/// There is no earlier version in the field yet, so there is no migration to
/// perform — only the forward-compatibility rule that a newer file is kept and
/// refused. [`migrate`] is the seam the first real migration lands in.
pub const RULES_SCHEMA_VERSION: u32 = 1;

/// The file name the rules live in, under the same directory as the service
/// state so an uninstall that removes user data has one place to look.
pub const RULES_FILE_NAME: &str = "awake-rules.json";

/// The stored rule set, with its schema stamp as a sibling of the payload.
///
/// The stamp is a top-level field rather than something inside `rule_set` so it
/// can be read from a document this version of the crate cannot deserialize.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesDocument {
    pub schema_version: u32,
    pub rule_set: RuleSet,
}

impl RulesDocument {
    pub fn new(rule_set: RuleSet) -> Self {
        Self {
            schema_version: RULES_SCHEMA_VERSION,
            rule_set,
        }
    }
}

/// What a load produced, and what it had to rescue to produce it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RulesLoad {
    pub rule_set: RuleSet,
    /// A malformed current-schema file is moved here before an empty rule set
    /// is returned, so a user's rules are never destroyed to keep the service
    /// starting. The service reports the path so the file can be recovered.
    pub recovered_corrupt_state: Option<PathBuf>,
}

enum DecodeError {
    Invalid,
    UnsupportedSchema(u32),
}

/// The migration seam.
///
/// Version 1 is the first schema shipped, so there is nothing to raise yet. A
/// future version lands its step here rather than in [`RulesStore::load`],
/// where the forward-compatibility refusal lives.
fn migrate(
    value: serde_json::Value,
    schema_version: u32,
) -> Result<serde_json::Value, DecodeError> {
    match schema_version {
        RULES_SCHEMA_VERSION => Ok(value),
        other => Err(DecodeError::UnsupportedSchema(other)),
    }
}

/// Reads and writes the user's automatic rules.
#[derive(Clone, Debug)]
pub struct RulesStore {
    path: PathBuf,
}

impl RulesStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_STATE_HOME/better-awake/awake-rules.json`, falling back to
    /// `~/.local/state`. Nothing here is privileged or shared between users.
    pub fn from_default_path() -> Self {
        Self::at_path(default_rules_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the stored rules.
    ///
    /// A missing file is a first run with no rules, not an error. A newer
    /// schema is an error, because overwriting a file a newer Better Awake
    /// wrote would destroy rules this version cannot even display.
    pub fn load(&self) -> Result<RulesLoad, StoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(RulesLoad {
                    rule_set: RuleSet::new(),
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
            Ok(document) => Ok(RulesLoad {
                rule_set: document.rule_set,
                recovered_corrupt_state: None,
            }),
            Err(DecodeError::UnsupportedSchema(version)) => Err(StoreError::UnsupportedSchema {
                path: self.path.clone(),
                version,
            }),
            Err(DecodeError::Invalid) => {
                let backup = self.backup_corrupt()?;
                Ok(RulesLoad {
                    rule_set: RuleSet::new(),
                    recovered_corrupt_state: Some(backup),
                })
            }
        }
    }

    /// Writes through a temporary file and renames, so a crash mid-write leaves
    /// the previous rules readable rather than a half-written file.
    pub fn save(&self, rule_set: &RuleSet) -> Result<(), StoreError> {
        let document = RulesDocument::new(rule_set.clone());
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

    fn decode(&self, bytes: &[u8]) -> Result<RulesDocument, DecodeError> {
        // Read the stamp before the document: a newer writer may have added
        // condition variants or required fields this version cannot parse, and
        // that file must be preserved rather than treated as corruption.
        let value =
            serde_json::from_slice::<serde_json::Value>(bytes).map_err(|_| DecodeError::Invalid)?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or(DecodeError::Invalid)?;
        if schema_version > RULES_SCHEMA_VERSION {
            return Err(DecodeError::UnsupportedSchema(schema_version));
        }
        let value = migrate(value, schema_version)?;
        serde_json::from_value::<RulesDocument>(value).map_err(|_| DecodeError::Invalid)
    }

    fn backup_corrupt(&self) -> Result<PathBuf, StoreError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(RULES_FILE_NAME);
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

fn default_rules_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("better-awake").join(RULES_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    use awake_core::{
        Combine, Condition, ConditionGroup, PAUSE_LONG_SECONDS, PauseState, Reason, Rule, RuleId,
    };

    fn rule(name: &str) -> Rule {
        Rule::new(
            RuleId(0),
            Reason::new(name).unwrap(),
            Combine::All,
            [ConditionGroup::one(Condition::AcPower { connected: true }).unwrap()],
        )
        .unwrap()
    }

    fn store() -> (tempfile::TempDir, RulesStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = RulesStore::at_path(directory.path().join(RULES_FILE_NAME));
        (directory, store)
    }

    #[test]
    fn a_first_run_starts_with_no_rules_and_without_an_error() {
        let (_directory, store) = store();
        let loaded = store.load().unwrap();
        assert!(loaded.rule_set.is_empty());
        assert_eq!(loaded.recovered_corrupt_state, None);
    }

    #[test]
    fn a_rule_set_survives_a_save_and_load() {
        let (_directory, store) = store();
        let mut saved = RuleSet::new();
        saved.add(rule("Build is running")).unwrap();
        let second = saved.add(rule("Download is running")).unwrap();
        saved.set_enabled(second, false).unwrap();
        store.save(&saved).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.rule_set, saved);
        assert_eq!(loaded.rule_set.rules().len(), 2);
        assert_eq!(loaded.rule_set.rules()[0].name.as_str(), "Build is running");
        assert!(!loaded.rule_set.rule(second).unwrap().enabled);
    }

    #[test]
    fn a_paused_rule_set_is_still_paused_after_a_round_trip() {
        let (_directory, store) = store();
        let mut saved = RuleSet::new();
        saved.add(rule("Build is running")).unwrap();
        saved.pause_for(PAUSE_LONG_SECONDS, 1_000).unwrap();
        saved.override_all(true).unwrap();
        store.save(&saved).unwrap();

        // A pause that did not survive a restart would quietly turn every rule
        // back on while the user still believed they were suspended.
        let loaded = store.load().unwrap().rule_set;
        assert_eq!(
            loaded.pause_state(),
            PauseState::Until {
                unix_seconds: 1_000 + PAUSE_LONG_SECONDS
            }
        );
        assert!(loaded.is_overridden());
    }

    #[test]
    fn a_newer_schema_is_preserved_and_refused_rather_than_reset() {
        let (_directory, store) = store();
        std::fs::write(store.path(), br#"{"schema_version":99,"rule_set":{}}"#).unwrap();

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
            .expect("the unreadable rules file must be kept");
        assert!(backup.exists());
        assert!(loaded.rule_set.is_empty());
        assert!(!store.path().exists());
    }

    #[test]
    fn a_file_with_no_schema_stamp_is_treated_as_corrupt_not_as_version_one() {
        let (_directory, store) = store();
        std::fs::write(store.path(), br#"{"rule_set":{"rules":[],"next_id":1}}"#).unwrap();
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
        let path = default_rules_path();
        assert!(path.ends_with(Path::new("better-awake").join(RULES_FILE_NAME)));
    }
}
