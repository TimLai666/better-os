//! Where the configuration, the capture, and the safe-mode marker live.
//!
//! Three properties are load-bearing:
//!
//! - **Writes are atomic.** A configuration is written to a temporary file in
//!   the same directory and renamed over the old one, so a crash mid-write
//!   leaves the previous file intact rather than a half-written one.
//! - **The capture is written once.** [`TouchpadStore::capture_once`] refuses
//!   to replace an existing backup file. Overwriting it would replace the only
//!   record of what the touchpad did before Better OS.
//! - **Damage is reported, not repaired.** A configuration that will not parse
//!   comes back as an error naming the file. Silently starting from defaults
//!   would look identical to a first run and would then overwrite the file that
//!   might still be recoverable.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::backup::Backup;
use crate::config::{ConfigError, TouchpadConfig};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} could not be read as a Better Touchpad configuration: {source}")]
    Damaged {
        path: PathBuf,
        #[source]
        source: ConfigError,
    },
    #[error("{0} already holds a capture, and a capture is never replaced")]
    CaptureExists(PathBuf),
}

fn io(path: &Path) -> impl Fn(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

pub struct TouchpadStore {
    directory: PathBuf,
}

impl TouchpadStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// `$XDG_CONFIG_HOME/better-os/touchpad`, or the same under `$HOME/.config`.
    pub fn user_directory() -> PathBuf {
        let config = match std::env::var("XDG_CONFIG_HOME") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
        };
        config.join("better-os").join("touchpad")
    }

    pub fn for_user() -> Self {
        Self::new(Self::user_directory())
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn config_path(&self) -> PathBuf {
        self.directory.join("config.json")
    }

    pub fn backup_path(&self) -> PathBuf {
        self.directory.join("backup.json")
    }

    pub fn safe_mode_path(&self) -> PathBuf {
        self.directory.join("safe-mode")
    }

    /// The saved configuration, or the shipped defaults when there is no file
    /// yet. A file that exists and cannot be read is an error, not a first run.
    pub fn load_config(&self) -> Result<TouchpadConfig, StoreError> {
        let path = self.config_path();
        match fs::read_to_string(&path) {
            Ok(text) => TouchpadConfig::from_json(&text).map_err(|source| StoreError::Damaged {
                path: path.clone(),
                source,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(TouchpadConfig::default()),
            Err(error) => Err(io(&path)(error)),
        }
    }

    pub fn save_config(&self, config: &TouchpadConfig) -> Result<(), StoreError> {
        self.write_atomically(&self.config_path(), &config.to_json())
    }

    pub fn load_backup(&self) -> Result<Option<Backup>, StoreError> {
        let path = self.backup_path();
        match fs::read_to_string(&path) {
            Ok(text) => {
                serde_json::from_str(&text)
                    .map(Some)
                    .map_err(|error| StoreError::Damaged {
                        path: path.clone(),
                        source: ConfigError::NotJson(error.to_string()),
                    })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io(&path)(error)),
        }
    }

    /// Writes the first capture. Refuses to replace one that already exists.
    pub fn capture_once(&self, backup: &Backup) -> Result<(), StoreError> {
        let path = self.backup_path();
        if path.exists() {
            return Err(StoreError::CaptureExists(path));
        }
        self.write_atomically(
            &path,
            &serde_json::to_string_pretty(backup).expect("a capture always serializes"),
        )
    }

    /// Adds newly captured readings to the stored capture, leaving every
    /// reading it already holds alone.
    pub fn extend_capture(&self, backup: &Backup) -> Result<(), StoreError> {
        let merged = match self.load_backup()? {
            Some(mut existing) => {
                existing.extend_untouched(
                    backup
                        .readings()
                        .map(|(setting, reading)| (setting, reading.clone()))
                        .collect(),
                );
                existing
            }
            None => backup.clone(),
        };
        self.write_atomically(
            &self.backup_path(),
            &serde_json::to_string_pretty(&merged).expect("a capture always serializes"),
        )
    }

    pub fn safe_mode_enabled(&self) -> bool {
        self.safe_mode_path().exists()
    }

    /// The safe-mode entry point. It writes a marker file and nothing else, so
    /// it works when the configuration itself is the thing that is broken.
    pub fn enable_safe_mode(&self) -> Result<(), StoreError> {
        self.write_atomically(
            &self.safe_mode_path(),
            "Better Touchpad integration is disabled.\nDelete this file to enable it again.\n",
        )
    }

    pub fn clear_safe_mode(&self) -> Result<(), StoreError> {
        let path = self.safe_mode_path();
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io(&path)(error)),
        }
    }

    fn write_atomically(&self, path: &Path, contents: &str) -> Result<(), StoreError> {
        fs::create_dir_all(&self.directory).map_err(io(&self.directory))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&temporary, contents).map_err(io(&temporary))?;
        fs::rename(&temporary, path).map_err(io(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Reading, SettingId, SettingValue};
    use crate::value::Sensitivity;

    fn store() -> (tempfile::TempDir, TouchpadStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path().join("touchpad"));
        (directory, store)
    }

    #[test]
    fn a_first_run_reads_the_shipped_defaults_without_writing_anything() {
        let (_guard, store) = store();
        assert_eq!(store.load_config().unwrap(), TouchpadConfig::default());
        assert!(!store.config_path().exists());
    }

    #[test]
    fn a_saved_configuration_survives_a_restart() {
        let (_guard, store) = store();
        let mut config = TouchpadConfig::default();
        config
            .set(
                SettingId::PointerSensitivity,
                SettingValue::sensitivity(Sensitivity::new(0.75).unwrap()),
            )
            .unwrap();
        store.save_config(&config).unwrap();

        let reopened = TouchpadStore::new(store.directory());
        assert_eq!(reopened.load_config().unwrap(), config);
    }

    #[test]
    fn a_damaged_configuration_is_reported_rather_than_replaced_by_defaults() {
        let (_guard, store) = store();
        fs::create_dir_all(store.directory()).unwrap();
        fs::write(store.config_path(), "{ not json").unwrap();
        assert!(matches!(
            store.load_config(),
            Err(StoreError::Damaged { .. })
        ));
        // The unreadable file is still there to look at.
        assert_eq!(
            fs::read_to_string(store.config_path()).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn a_capture_is_written_once_and_never_replaced() {
        let (_guard, store) = store();
        let first = Backup::capture(
            "gnome",
            None,
            vec![(
                SettingId::TapToClick,
                Reading::value(SettingValue::toggle(false)),
            )],
            1,
        );
        store.capture_once(&first).unwrap();

        let second = Backup::capture(
            "gnome",
            None,
            vec![(
                SettingId::TapToClick,
                Reading::value(SettingValue::toggle(true)),
            )],
            2,
        );
        assert!(matches!(
            store.capture_once(&second),
            Err(StoreError::CaptureExists(_))
        ));
        assert_eq!(store.load_backup().unwrap().unwrap(), first);
    }

    #[test]
    fn extending_a_capture_adds_new_settings_and_keeps_the_old_readings() {
        let (_guard, store) = store();
        store
            .capture_once(&Backup::capture(
                "gnome",
                None,
                vec![(
                    SettingId::TapToClick,
                    Reading::value(SettingValue::toggle(false)),
                )],
                1,
            ))
            .unwrap();
        store
            .extend_capture(&Backup::capture(
                "gnome",
                None,
                vec![
                    (
                        SettingId::TapToClick,
                        Reading::value(SettingValue::toggle(true)),
                    ),
                    (SettingId::DragLock, Reading::session_default("unset")),
                ],
                2,
            ))
            .unwrap();

        let stored = store.load_backup().unwrap().unwrap();
        assert_eq!(
            stored.reading(SettingId::TapToClick),
            Some(&Reading::value(SettingValue::toggle(false)))
        );
        assert!(stored.covers(SettingId::DragLock));
        assert_eq!(stored.captured_at, 1);
    }

    #[test]
    fn safe_mode_is_a_marker_file_that_can_be_set_and_cleared() {
        let (_guard, store) = store();
        assert!(!store.safe_mode_enabled());
        store.enable_safe_mode().unwrap();
        assert!(store.safe_mode_enabled());
        store.clear_safe_mode().unwrap();
        assert!(!store.safe_mode_enabled());
        // Clearing a safe mode that is not on is not an error.
        store.clear_safe_mode().unwrap();
    }

    #[test]
    fn no_temporary_file_is_left_behind_by_a_write() {
        let (_guard, store) = store();
        store.save_config(&TouchpadConfig::default()).unwrap();
        let leftovers: Vec<_> = fs::read_dir(store.directory())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "left behind {leftovers:?}");
    }
}
