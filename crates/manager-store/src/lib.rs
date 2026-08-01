//! Versioned local persistence for Better Manager's mock state.
//!
//! This crate is an adapter only. It does not decide lifecycle transitions;
//! those remain in manager-core. Writes use a process-local lock, revision
//! checks, and atomic rename so a stale CLI or GUI instance cannot silently
//! overwrite newer history.

use manager_core::{ManagerState, STATE_SCHEMA_VERSION, StateValidationError};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

pub const STATE_FILE_NAME: &str = "manager-state.json";

pub trait StateStore {
    fn load(&self) -> Result<LoadOutcome, StoreError>;
    fn save(&self, state: &ManagerState) -> Result<(), StoreError>;
}

#[derive(Clone, Debug)]
pub struct JsonStore {
    path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadOutcome {
    pub state: ManagerState,
    /// A malformed old file is retained under this path before a default state
    /// is returned. A future schema is never reset and returns an error.
    pub recovered_corrupt_state: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("could not access manager state at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("manager state at {path} is malformed: {reason}")]
    InvalidState { path: PathBuf, reason: String },
    #[error("manager state schema version {version} at {path} is unsupported")]
    UnsupportedSchema { path: PathBuf, version: u32 },
    #[error("manager state is busy at {path}")]
    Busy { path: PathBuf },
    #[error("refusing to overwrite unreadable manager state at {path}")]
    RefuseToOverwriteUnreadable { path: PathBuf },
    #[error("manager state changed from revision {stored} to a newer value before this write")]
    StaleWrite { stored: u64, attempted: u64 },
    #[error("could not serialize manager state")]
    Serialize(#[source] serde_json::Error),
}

enum DecodeError {
    Invalid(String),
    UnsupportedSchema(u32),
}

impl JsonStore {
    pub fn from_default_path() -> Self {
        Self::at_path(default_state_path())
    }

    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn decode(&self, bytes: &[u8]) -> Result<ManagerState, DecodeError> {
        // Check the schema before deserializing the whole state. A newer writer
        // can legitimately add required fields that this version cannot parse;
        // that file must be preserved rather than treated as corruption.
        let value = serde_json::from_slice::<serde_json::Value>(bytes)
            .map_err(|error| DecodeError::Invalid(error.to_string()))?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                DecodeError::Invalid("schema_version is missing or invalid".to_string())
            })?;
        let schema_version = u32::try_from(schema_version)
            .map_err(|_| DecodeError::Invalid("schema_version exceeds u32".to_string()))?;
        if schema_version != STATE_SCHEMA_VERSION {
            return Err(DecodeError::UnsupportedSchema(schema_version));
        }
        let state = serde_json::from_slice::<ManagerState>(bytes)
            .map_err(|error| DecodeError::Invalid(error.to_string()))?;
        state.validate().map_err(|error| match error {
            StateValidationError::UnsupportedSchemaVersion(version) => {
                DecodeError::UnsupportedSchema(version)
            }
            error => DecodeError::Invalid(error.to_string()),
        })?;
        Ok(state)
    }

    fn corrupt_backup_path(&self) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(STATE_FILE_NAME);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        parent.join(format!("{name}.corrupt-{}-{nonce}", process::id()))
    }

    fn backup_corrupt(&self) -> Result<PathBuf, StoreError> {
        let backup = self.corrupt_backup_path();
        fs::rename(&self.path, &backup).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        Ok(backup)
    }

    fn lock_path(&self) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(STATE_FILE_NAME);
        parent.join(format!(".{name}.lock"))
    }

    fn acquire_lock(&self) -> Result<WriteLock, StoreError> {
        let path = self.lock_path();
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(process::id().to_string().as_bytes())
                        .map_err(|source| StoreError::Io {
                            path: path.clone(),
                            source,
                        })?;
                    file.sync_all().map_err(|source| StoreError::Io {
                        path: path.clone(),
                        source,
                    })?;
                    return Ok(WriteLock { path, _file: file });
                }
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                    if self.lock_is_stale(&path)? {
                        fs::remove_file(&path).map_err(|source| StoreError::Io {
                            path: path.clone(),
                            source,
                        })?;
                        continue;
                    }
                    return Err(StoreError::Busy { path });
                }
                Err(source) => return Err(StoreError::Io { path, source }),
            }
        }
        Err(StoreError::Busy { path })
    }

    fn lock_is_stale(&self, path: &Path) -> Result<bool, StoreError> {
        let contents = fs::read_to_string(path).map_err(|source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let Ok(pid) = contents.trim().parse::<u32>() else {
            return Ok(true);
        };
        Ok(!Path::new("/proc").join(pid.to_string()).exists())
    }

    fn existing_revision(&self) -> Result<Option<u64>, StoreError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&self.path).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        match self.decode(&bytes) {
            Ok(state) => Ok(Some(state.revision)),
            Err(DecodeError::UnsupportedSchema(version)) => Err(StoreError::UnsupportedSchema {
                path: self.path.clone(),
                version,
            }),
            Err(DecodeError::Invalid(_)) => Err(StoreError::RefuseToOverwriteUnreadable {
                path: self.path.clone(),
            }),
        }
    }

    fn temporary_path(&self) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(STATE_FILE_NAME);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        parent.join(format!(".{name}.{}-{nonce}.tmp", process::id()))
    }
}

impl StateStore for JsonStore {
    fn load(&self) -> Result<LoadOutcome, StoreError> {
        if !self.path.exists() {
            return Ok(LoadOutcome {
                state: ManagerState::default(),
                recovered_corrupt_state: None,
            });
        }
        let bytes = fs::read(&self.path).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        match self.decode(&bytes) {
            Ok(state) => Ok(LoadOutcome {
                state,
                recovered_corrupt_state: None,
            }),
            Err(DecodeError::UnsupportedSchema(version)) => Err(StoreError::UnsupportedSchema {
                path: self.path.clone(),
                version,
            }),
            Err(DecodeError::Invalid(_reason)) => {
                let _lock = self.acquire_lock()?;
                if !self.path.exists() {
                    return Ok(LoadOutcome {
                        state: ManagerState::default(),
                        recovered_corrupt_state: None,
                    });
                }
                let bytes = fs::read(&self.path).map_err(|source| StoreError::Io {
                    path: self.path.clone(),
                    source,
                })?;
                match self.decode(&bytes) {
                    Ok(state) => Ok(LoadOutcome {
                        state,
                        recovered_corrupt_state: None,
                    }),
                    Err(DecodeError::UnsupportedSchema(version)) => {
                        Err(StoreError::UnsupportedSchema {
                            path: self.path.clone(),
                            version,
                        })
                    }
                    Err(DecodeError::Invalid(_)) => {
                        let backup = self.backup_corrupt()?;
                        Ok(LoadOutcome {
                            state: ManagerState::default(),
                            recovered_corrupt_state: Some(backup),
                        })
                    }
                }
            }
        }
    }

    fn save(&self, state: &ManagerState) -> Result<(), StoreError> {
        state.validate().map_err(|error| match error {
            StateValidationError::UnsupportedSchemaVersion(version) => {
                StoreError::UnsupportedSchema {
                    path: self.path.clone(),
                    version,
                }
            }
            error => StoreError::InvalidState {
                path: self.path.clone(),
                reason: error.to_string(),
            },
        })?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|source| StoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let _lock = self.acquire_lock()?;
        if let Some(stored) = self.existing_revision()? {
            if stored >= state.revision {
                return Err(StoreError::StaleWrite {
                    stored,
                    attempted: state.revision,
                });
            }
        }

        let temporary = self.temporary_path();
        let result = (|| -> Result<(), StoreError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|source| StoreError::Io {
                    path: temporary.clone(),
                    source,
                })?;
            serde_json::to_writer_pretty(&mut file, state).map_err(StoreError::Serialize)?;
            file.write_all(b"\n").map_err(|source| StoreError::Io {
                path: temporary.clone(),
                source,
            })?;
            file.sync_all().map_err(|source| StoreError::Io {
                path: temporary.clone(),
                source,
            })?;
            fs::rename(&temporary, &self.path).map_err(|source| StoreError::Io {
                path: self.path.clone(),
                source,
            })?;
            Ok(())
        })();

        if result.is_err() && temporary.exists() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

struct WriteLock {
    path: PathBuf,
    _file: File,
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn default_state_path() -> PathBuf {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("better-os").join(STATE_FILE_NAME);
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("better-os")
            .join(STATE_FILE_NAME);
    }
    PathBuf::from(".better-os").join(STATE_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
    use manager_core::{DesiredOperation, Manager, OperationStage, SystemProfile};

    fn temporary_directory(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "better-os-manager-store-{name}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn manager() -> Manager {
        let manifests = [include_str!(
            "../../../components/manifests/better-monitor.yaml"
        )]
        .into_iter()
        .map(|manifest| ComponentManifest::parse_yaml(manifest).unwrap())
        .collect::<Vec<_>>();
        Manager::new(
            ComponentCatalog::from_manifests(manifests).unwrap(),
            SystemProfile::default(),
        )
    }

    #[test]
    fn reloads_state_settings_and_activity() {
        let directory = temporary_directory("reload");
        let store = JsonStore::at_path(directory.join(STATE_FILE_NAME));
        let mut state = ManagerState::default();
        state.set_installed(ComponentId::new("better-monitor").unwrap(), "0.0.1", true);
        let mut settings = state.settings.clone();
        settings.locale = manager_core::StoredLocale::ZhTw;
        settings.release_channel = manager_core::ReleaseChannel::Preview;
        state.update_settings(settings);
        store.save(&state).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.state, state);
        assert_eq!(loaded.recovered_corrupt_state, None);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn preserves_corrupt_state_before_returning_a_clean_default() {
        let directory = temporary_directory("corrupt");
        let path = directory.join(STATE_FILE_NAME);
        fs::write(&path, b"{not valid json").unwrap();
        let outcome = JsonStore::at_path(&path).load().unwrap();
        let backup = outcome.recovered_corrupt_state.unwrap();
        assert_eq!(outcome.state, ManagerState::default());
        assert!(!path.exists());
        assert_eq!(fs::read(backup).unwrap(), b"{not valid json");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn refuses_to_reset_a_future_schema() {
        let directory = temporary_directory("future");
        let path = directory.join(STATE_FILE_NAME);
        fs::write(
            &path,
            r#"{"schema_version":99,"revision":0,"components":{},"activity":[],"settings":{},"active_operation":null}"#,
        )
        .unwrap();
        let error = JsonStore::at_path(&path).load().unwrap_err();
        assert!(matches!(
            error,
            StoreError::UnsupportedSchema { version: 99, .. }
        ));
        assert!(path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_a_stale_writer() {
        let directory = temporary_directory("stale");
        let store = JsonStore::at_path(directory.join(STATE_FILE_NAME));
        let mut first = ManagerState::default();
        first.set_installed(ComponentId::new("better-manager").unwrap(), "0.0.1", true);
        store.save(&first).unwrap();

        let mut second = first.clone();
        second.set_installed(ComponentId::new("better-monitor").unwrap(), "0.0.1", true);
        store.save(&second).unwrap();
        assert!(matches!(
            store.save(&first),
            Err(StoreError::StaleWrite { .. })
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn replaces_a_lock_left_by_a_dead_process() {
        let directory = temporary_directory("stale-lock");
        let store = JsonStore::at_path(directory.join(STATE_FILE_NAME));
        fs::write(store.lock_path(), u32::MAX.to_string()).unwrap();
        let mut state = ManagerState::default();
        state.set_installed(ComponentId::new("better-manager").unwrap(), "0.0.1", true);
        store.save(&state).unwrap();
        assert!(!store.lock_path().exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resumes_a_valid_active_mock_lifecycle_after_reload() {
        let directory = temporary_directory("resume");
        let store = JsonStore::at_path(directory.join(STATE_FILE_NAME));
        let manager = manager();
        let component = ComponentId::new("better-monitor").unwrap();
        let mut state = ManagerState::default();
        let plan = manager
            .plan(&state, &component, DesiredOperation::Install)
            .unwrap();
        manager.begin(&mut state, plan).unwrap();
        store.save(&state).unwrap();

        let loaded = store.load().unwrap().state;
        manager.validate_state(&loaded).unwrap();
        assert_eq!(
            loaded.active_operation.as_ref().map(|active| active.stage),
            Some(OperationStage::Downloading)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn reloads_an_active_plan_written_before_optional_review_metadata() {
        let directory = temporary_directory("legacy-active-plan");
        let path = directory.join(STATE_FILE_NAME);
        let manager = manager();
        let component = ComponentId::new("better-monitor").unwrap();
        let mut state = ManagerState::default();
        let plan = manager
            .plan(&state, &component, DesiredOperation::Install)
            .unwrap();
        manager.begin(&mut state, plan).unwrap();

        let mut value = serde_json::to_value(&state).unwrap();
        let plan = value
            .get_mut("active_operation")
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|operation| operation.get_mut("plan"))
            .and_then(serde_json::Value::as_object_mut)
            .unwrap();
        plan.remove("disk_space");
        for step in plan
            .get_mut("steps")
            .and_then(serde_json::Value::as_array_mut)
            .unwrap()
        {
            let step = step.as_object_mut().unwrap();
            step.remove("required_disk_bytes");
            step.remove("release_notes");
        }
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let loaded = JsonStore::at_path(&path).load().unwrap().state;
        manager.validate_state(&loaded).unwrap();
        assert_eq!(
            loaded
                .active_operation
                .as_ref()
                .map(|active| active.plan.disk_space()),
            Some(manager_core::DiskSpaceCheck::NotDeclared)
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
