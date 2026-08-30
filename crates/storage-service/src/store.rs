//! Where per-device Performance-mode overrides live.
//!
//! A user-session file under `$XDG_CONFIG_HOME/better-os/storage`, written
//! atomically through a temporary file and a rename so an interrupted write
//! cannot leave a half-parsed preference behind. A file from a newer schema is
//! preserved and refused rather than reset, and a corrupt current-schema file
//! is moved aside so the default takes over without losing the evidence — the
//! same three rules `manager-store` follows.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use storage_core::PreferenceSet;
use thiserror::Error;

pub const PREFERENCES_FILE_NAME: &str = "storage-preferences.json";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("could not access storage preferences at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("storage preferences at {path} use unsupported schema version {version}")]
    UnsupportedSchema { path: PathBuf, version: u32 },
    #[error("could not serialize storage preferences: {0}")]
    Serialize(#[source] serde_json::Error),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadOutcome {
    pub preferences: PreferenceSet,
    /// Set when a malformed file was moved aside before defaults were used.
    pub recovered_corrupt_file: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct PreferenceStore {
    path: PathBuf,
}

/// `$XDG_CONFIG_HOME/better-os/storage`, or `~/.config/better-os/storage`.
pub fn default_preferences_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("better-os")
        .join("storage")
        .join(PREFERENCES_FILE_NAME)
}

impl PreferenceStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn from_default_path() -> Self {
        Self::at_path(default_preferences_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<LoadOutcome, StoreError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            // No file yet is the normal first run: every device defaults to
            // Direct Removal, which is what an empty set means.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadOutcome {
                    preferences: PreferenceSet::new(),
                    recovered_corrupt_file: None,
                });
            }
            Err(source) => {
                return Err(StoreError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        };

        match PreferenceSet::from_json(&contents) {
            Ok(preferences) => Ok(LoadOutcome {
                preferences,
                recovered_corrupt_file: None,
            }),
            Err(storage_core::preferences::PreferenceError::UnsupportedSchema(version)) => {
                Err(StoreError::UnsupportedSchema {
                    path: self.path.clone(),
                    version,
                })
            }
            Err(_) => {
                let recovered = self.path.with_extension("corrupt");
                let _ = fs::rename(&self.path, &recovered);
                Ok(LoadOutcome {
                    preferences: PreferenceSet::new(),
                    recovered_corrupt_file: Some(recovered),
                })
            }
        }
    }

    pub fn save(&self, preferences: &PreferenceSet) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let document = preferences.to_json().map_err(StoreError::Serialize)?;

        let temporary = self
            .path
            .with_extension(format!("tmp{}", std::process::id()));
        let mut file = fs::File::create(&temporary).map_err(|source| StoreError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.write_all(document.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|source| StoreError::Io {
                path: temporary.clone(),
                source,
            })?;
        fs::rename(&temporary, &self.path).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage_core::{
        DeviceIdentity, IdentityEvidence, PerformanceOptIn, RemovalPolicy, Transport,
    };

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "better-os-storage-store-{label}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn store(&self) -> PreferenceStore {
            PreferenceStore::at_path(self.root.join(PREFERENCES_FILE_NAME))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn device() -> DeviceIdentity {
        DeviceIdentity::from_evidence(IdentityEvidence {
            filesystem_uuid: Some("A1B2-C3D4".to_string()),
            device_path: "/dev/sdb1".to_string(),
            transport: Transport::Usb,
            ..IdentityEvidence::default()
        })
    }

    #[test]
    fn a_first_run_with_no_file_starts_every_device_at_direct_removal() {
        let fixture = Fixture::new("first-run");
        let outcome = fixture.store().load().unwrap();
        assert!(outcome.preferences.is_empty());
        assert!(outcome.recovered_corrupt_file.is_none());
        assert_eq!(
            outcome.preferences.policy_for(&device()),
            RemovalPolicy::DirectRemoval
        );
    }

    #[test]
    fn a_performance_override_survives_a_service_restart() {
        let fixture = Fixture::new("restart");
        let store = fixture.store();
        let mut preferences = PreferenceSet::new();
        preferences
            .set_performance(&device(), PerformanceOptIn::acknowledging_all_risks())
            .unwrap();
        store.save(&preferences).unwrap();

        let reloaded = store.load().unwrap().preferences;
        assert_eq!(reloaded.policy_for(&device()), RemovalPolicy::Performance);
    }

    #[test]
    fn a_file_from_a_newer_schema_is_preserved_and_refused() {
        let fixture = Fixture::new("newer");
        let store = fixture.store();
        fs::write(store.path(), r#"{"schema_version":99,"records":{}}"#).unwrap();
        assert!(matches!(
            store.load(),
            Err(StoreError::UnsupportedSchema { version: 99, .. })
        ));
        // And the file is still there.
        assert!(store.path().exists());
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_rather_than_deleted_or_trusted() {
        let fixture = Fixture::new("corrupt");
        let store = fixture.store();
        fs::write(store.path(), "{not json at all").unwrap();
        let outcome = store.load().unwrap();
        assert!(outcome.preferences.is_empty());
        let recovered = outcome
            .recovered_corrupt_file
            .expect("the bad file was kept");
        assert!(recovered.exists());
    }

    #[test]
    fn a_write_replaces_the_file_atomically_and_leaves_no_temporary_behind() {
        let fixture = Fixture::new("atomic");
        let store = fixture.store();
        store.save(&PreferenceSet::new()).unwrap();
        let leftovers: Vec<_> = fs::read_dir(&fixture.root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
    }
}
