//! Where the gesture configuration and its capture live.
//!
//! Two files, `gestures.json` and `gestures-backup.json`, in the same directory
//! as the pointer, scrolling, and clicking configuration and written through
//! the same atomic-write and write-once machinery `touchpad-core::store`
//! already owns. There is no second implementation of either.
//!
//! Keeping them as separate files is a safety property, not filing. Issue #3
//! requires that a failing gesture adapter must not break pointer movement or
//! two-finger scrolling, and the cheapest way to be sure of that is for the
//! gesture half to have no way to touch the settings half: different files,
//! different capture, different restore. The worst a gesture failure can do is
//! leave the machine with no gestures.

use thiserror::Error;
use touchpad_core::{StoreError, TouchpadStore};

use crate::config::{ConfigError, GestureConfig};

#[derive(Debug, Error)]
pub enum GestureStoreError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{path} could not be read as a gesture configuration: {source}")]
    Damaged {
        path: String,
        #[source]
        source: ConfigError,
    },
}

/// The gesture half of Better Touchpad's stored state.
pub struct GestureStore {
    store: TouchpadStore,
}

impl GestureStore {
    pub fn new(store: TouchpadStore) -> Self {
        Self { store }
    }

    pub fn at(directory: impl Into<std::path::PathBuf>) -> Self {
        Self::new(TouchpadStore::new(directory))
    }

    pub fn for_user() -> Self {
        Self::new(TouchpadStore::for_user())
    }

    pub fn config_path(&self) -> std::path::PathBuf {
        self.store.directory().join("gestures.json")
    }

    pub fn capture_path(&self) -> std::path::PathBuf {
        self.store.directory().join("gestures-backup.json")
    }

    /// The saved gesture configuration, or the shipped one when there is no
    /// file. A file that exists and will not parse is an error, not a first
    /// run — the same rule the settings configuration follows, and for the same
    /// reason: starting from defaults would overwrite the only copy.
    pub fn load_config(&self) -> Result<GestureConfig, GestureStoreError> {
        let path = self.config_path();
        match self.store.read_text(&path)? {
            Some(text) => {
                GestureConfig::from_json(&text).map_err(|source| GestureStoreError::Damaged {
                    path: path.display().to_string(),
                    source,
                })
            }
            None => Ok(GestureConfig::default()),
        }
    }

    pub fn save_config(&self, config: &GestureConfig) -> Result<(), GestureStoreError> {
        self.store
            .write_text(&self.config_path(), &config.to_json())?;
        Ok(())
    }

    pub fn load_capture(&self) -> Result<Option<GestureConfig>, GestureStoreError> {
        let path = self.capture_path();
        match self.store.read_text(&path)? {
            Some(text) => GestureConfig::from_json(&text).map(Some).map_err(|source| {
                GestureStoreError::Damaged {
                    path: path.display().to_string(),
                    source,
                }
            }),
            None => Ok(None),
        }
    }

    /// Writes what the gestures were before Better Touchpad first changed them.
    ///
    /// Called before every apply and refuses to replace an existing capture, so
    /// the first capture is the only one that ever counts. A second capture
    /// taken after a change would record Better OS's own configuration as the
    /// thing to go back to.
    pub fn capture_once(&self, config: &GestureConfig) -> Result<(), GestureStoreError> {
        match self
            .store
            .write_once(&self.capture_path(), &config.to_json())
        {
            Ok(()) => Ok(()),
            // A capture that already exists is the expected case on every apply
            // after the first, and is not a failure.
            Err(StoreError::CaptureExists(_)) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn has_capture(&self) -> bool {
        self.capture_path().exists()
    }

    /// The settings store this one sits beside, for a caller that needs both.
    pub fn settings(&self) -> &TouchpadStore {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::{ConflictResolution, GNOME_46_GESTURES};
    use crate::plan::PresetPlan;
    use crate::preset::mac_style;
    use better_actions::DesktopAction;
    use touchpad_session::MockSessionAdapter;

    fn store() -> (tempfile::TempDir, GestureStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = GestureStore::at(directory.path().join("touchpad"));
        (directory, store)
    }

    #[test]
    fn a_first_run_reads_the_shipped_configuration_without_writing_anything() {
        let (_guard, store) = store();
        assert_eq!(store.load_config().unwrap(), GestureConfig::default());
        assert!(!store.config_path().exists());
        assert!(!store.has_capture());
    }

    #[test]
    fn a_saved_configuration_survives_a_restart() {
        let (_guard, store) = store();
        store.save_config(&mac_style()).unwrap();
        let reopened = GestureStore::at(store.settings().directory());
        assert_eq!(reopened.load_config().unwrap(), mac_style());
    }

    #[test]
    fn the_capture_written_before_the_first_change_is_never_replaced() {
        let (_guard, store) = store();
        let before = GestureConfig::default();
        store.capture_once(&before).unwrap();
        // A second apply captures again, and must not overwrite.
        store.capture_once(&mac_style()).unwrap();
        assert_eq!(store.load_capture().unwrap(), Some(before));
    }

    #[test]
    fn a_damaged_gesture_configuration_is_reported_rather_than_replaced_by_defaults() {
        let (_guard, store) = store();
        store.save_config(&mac_style()).unwrap();
        std::fs::write(store.config_path(), "{ not json").unwrap();
        assert!(matches!(
            store.load_config(),
            Err(GestureStoreError::Damaged { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(store.config_path()).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn a_failing_gesture_adapter_leaves_the_pointer_and_scrolling_state_untouched() {
        // Issue #3's rule, asserted rather than reasoned about: a gesture
        // failure must not reach pointer movement or two-finger scrolling.
        let (_guard, store) = store();
        let settings = store.settings();
        settings
            .save_config(&touchpad_core::TouchpadConfig::default())
            .unwrap();
        let settings_before = std::fs::read_to_string(settings.config_path()).unwrap();

        let mut adapter = MockSessionAdapter::new().failing(&DesktopAction::LauncherOpen);
        let plan = PresetPlan::build(
            &GestureConfig::default(),
            &mac_style(),
            GNOME_46_GESTURES,
            &adapter,
        );
        let resolutions = plan
            .conflicts
            .iter()
            .map(|conflict| (conflict.gesture.clone(), ConflictResolution::KeepBuiltIn))
            .collect();
        let approved = plan.approve(&resolutions, true).unwrap();
        store.capture_once(approved.previous()).unwrap();
        let (config, report) = approved.apply(&mut adapter);
        store.save_config(&config).unwrap();

        assert_eq!(report.state(), crate::plan::RunState::Failed);
        assert_eq!(
            std::fs::read_to_string(settings.config_path()).unwrap(),
            settings_before,
            "a gesture failure rewrote the pointer and scrolling configuration"
        );
        assert!(!settings.backup_path().exists());
        // And the gesture state is where it should be: its own two files.
        assert!(store.config_path().exists());
        assert!(store.has_capture());
    }

    #[test]
    fn a_damaged_gesture_configuration_does_not_stop_the_settings_being_read() {
        let (_guard, store) = store();
        let settings = store.settings();
        settings
            .save_config(&touchpad_core::TouchpadConfig::default())
            .unwrap();
        std::fs::write(store.config_path(), "{ not json").unwrap();

        assert!(store.load_config().is_err());
        assert_eq!(
            settings.load_config().unwrap(),
            touchpad_core::TouchpadConfig::default()
        );
    }
}
