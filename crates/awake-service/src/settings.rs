//! The service's own settings: today, only how long the lid may stay closed on
//! battery before the sessions holding it end (ticket 109).
//!
//! A service setting rather than a window preference, because the tray starts
//! sessions too and must be held to the same limit without reading the
//! window's file. It lives beside the service's other files in
//! `$XDG_STATE_HOME/better-awake/`, under its own schema stamp, with the same
//! rules `awake-store` follows: a missing file is the defaults, a newer schema
//! is kept and refused rather than overwritten, and a file that cannot be read
//! is moved aside rather than silently replaced.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use awake_core::LidBatteryLimit;
use serde::{Deserialize, Serialize};

pub const SETTINGS_FILE_NAME: &str = "awake-service-settings.json";

/// The only schema this service writes.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSettings {
    pub schema_version: u32,
    #[serde(default)]
    pub lid_battery_limit: LidBatteryLimit,
}

impl Default for ServiceSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            lid_battery_limit: LidBatteryLimit::default(),
        }
    }
}

/// What loading found, with anything worth saying about it as a stable key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsLoad {
    pub settings: ServiceSettings,
    pub detail: Option<String>,
    /// False when the file on disk was written by a newer Better Awake, so this
    /// one must not overwrite it.
    pub writable: bool,
}

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Next to the state file, wherever that is.
    pub fn beside(state_file: &Path) -> Self {
        let directory = state_file.parent().unwrap_or_else(|| Path::new("."));
        Self::at_path(directory.join(SETTINGS_FILE_NAME))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> SettingsLoad {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return SettingsLoad {
                    settings: ServiceSettings::default(),
                    detail: None,
                    writable: true,
                };
            }
            Err(error) => {
                return SettingsLoad {
                    settings: ServiceSettings::default(),
                    detail: Some(format!("awake.settings.unreadable:{error}")),
                    // Not known to be ours to replace.
                    writable: false,
                };
            }
        };

        #[derive(Deserialize)]
        struct Stamp {
            schema_version: u32,
        }
        match serde_json::from_slice::<Stamp>(&bytes) {
            Ok(stamp) if stamp.schema_version > SETTINGS_SCHEMA_VERSION => {
                return SettingsLoad {
                    settings: ServiceSettings::default(),
                    detail: Some(format!(
                        "awake.settings.newer_schema:{}",
                        stamp.schema_version
                    )),
                    writable: false,
                };
            }
            _ => {}
        }

        match serde_json::from_slice::<ServiceSettings>(&bytes) {
            Ok(settings) if settings.lid_battery_limit.validate().is_ok() => SettingsLoad {
                settings,
                detail: None,
                writable: true,
            },
            _ => {
                let backup = self.path.with_extension("json.corrupt");
                let moved = fs::rename(&self.path, &backup).is_ok();
                SettingsLoad {
                    settings: ServiceSettings::default(),
                    detail: Some("awake.settings.recovered_corrupt".to_string()),
                    writable: moved,
                }
            }
        }
    }

    /// Writes through a temporary file and renames, so a crash mid-write
    /// leaves the previous settings readable.
    pub fn save(&self, settings: &ServiceSettings) -> io::Result<()> {
        let mut settings = *settings;
        settings.schema_version = SETTINGS_SCHEMA_VERSION;
        let document = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, &document)?;
        fs::rename(&temporary, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, SettingsStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = SettingsStore::at_path(directory.path().join(SETTINGS_FILE_NAME));
        (directory, store)
    }

    #[test]
    fn a_missing_file_is_the_two_hour_default() {
        let (_directory, store) = store();
        let load = store.load();
        assert_eq!(load.settings.lid_battery_limit.minutes(), Some(120));
        assert!(load.writable);
        assert_eq!(load.detail, None);
    }

    #[test]
    fn a_saved_limit_is_read_back() {
        let (_directory, store) = store();
        for limit in [
            LidBatteryLimit::Never,
            LidBatteryLimit::Minutes { minutes: 30 },
        ] {
            store
                .save(&ServiceSettings {
                    lid_battery_limit: limit,
                    ..ServiceSettings::default()
                })
                .unwrap();
            assert_eq!(store.load().settings.lid_battery_limit, limit);
        }
    }

    #[test]
    fn a_newer_file_is_kept_and_not_overwritten() {
        let (_directory, store) = store();
        let newer = r#"{"schema_version":9,"lid_battery_limit":{"kind":"never"},"x":1}"#;
        fs::write(store.path(), newer).unwrap();
        let load = store.load();
        assert!(!load.writable);
        assert_eq!(load.settings, ServiceSettings::default());
        assert_eq!(fs::read_to_string(store.path()).unwrap(), newer);
    }

    #[test]
    fn an_unreadable_or_out_of_range_file_is_moved_aside() {
        for document in [
            "not json",
            r#"{"schema_version":1,"lid_battery_limit":{"kind":"minutes","minutes":1}}"#,
        ] {
            let (_directory, store) = store();
            fs::write(store.path(), document).unwrap();
            let load = store.load();
            assert_eq!(load.settings, ServiceSettings::default());
            assert_eq!(
                load.detail.as_deref(),
                Some("awake.settings.recovered_corrupt")
            );
            assert!(load.writable);
            assert_eq!(
                fs::read_to_string(store.path().with_extension("json.corrupt")).unwrap(),
                document
            );
        }
    }

    #[test]
    fn the_file_sits_beside_the_state_file() {
        let store = SettingsStore::beside(Path::new("/s/better-awake/awake-service-state.json"));
        assert_eq!(
            store.path(),
            Path::new("/s/better-awake/awake-service-settings.json")
        );
    }
}
