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
use crate::profiles::GestureProfiles;

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
    /// An import naming a file that is not there. Distinct from a damaged one:
    /// there is nothing to say about the contents of a file nobody wrote.
    #[error("gestures.import.no_such_file:{0}")]
    NoSuchFile(String),
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

    /// Every saved gesture profile, or the shipped document when there is no
    /// file. A file that exists and will not parse is an error, not a first
    /// run — the same rule the settings configuration follows, and for the same
    /// reason: starting from defaults would overwrite the only copy.
    ///
    /// A version 1 file — one global profile, which is all there was — is
    /// migrated on the way in.
    pub fn load_profiles(&self) -> Result<GestureProfiles, GestureStoreError> {
        let path = self.config_path();
        match self.store.read_text(&path)? {
            Some(text) => {
                GestureProfiles::from_json(&text).map_err(|source| GestureStoreError::Damaged {
                    path: path.display().to_string(),
                    source,
                })
            }
            None => Ok(GestureProfiles::default()),
        }
    }

    pub fn save_profiles(&self, profiles: &GestureProfiles) -> Result<(), GestureStoreError> {
        self.store
            .write_text(&self.config_path(), &profiles.to_json())?;
        Ok(())
    }

    /// The global profile, which is what a caller with no device in hand means.
    pub fn load_config(&self) -> Result<GestureConfig, GestureStoreError> {
        Ok(self.load_profiles()?.global)
    }

    /// The profile in force: the selected device's own if it has one, the
    /// global one otherwise.
    ///
    /// This is what the resident pipeline recognizes against, because the
    /// window edits the profile of the pad it is showing and a daemon that
    /// performed the global one instead would perform gestures the user is not
    /// looking at.
    pub fn load_active_config(&self) -> Result<GestureConfig, GestureStoreError> {
        let profiles = self.load_profiles()?;
        Ok(profiles.resolve(profiles.active_device.as_deref()).clone())
    }

    /// Replaces the global profile, leaving every device profile alone.
    pub fn save_config(&self, config: &GestureConfig) -> Result<(), GestureStoreError> {
        let mut profiles = self.load_profiles().unwrap_or_default();
        profiles.global = config.clone();
        self.save_profiles(&profiles)
    }

    /// Writes the profile document to a path the user chose.
    ///
    /// The document written is the configuration schema, not an export format:
    /// the file that comes out is one this build would read back as its own
    /// configuration, which is why import needs no second parser.
    pub fn export_to(
        &self,
        path: &std::path::Path,
        profiles: &GestureProfiles,
    ) -> Result<(), GestureStoreError> {
        self.store.write_text(path, &profiles.to_json())?;
        Ok(())
    }

    /// Reads a profile document from a path the user chose, as untrusted input.
    ///
    /// Nothing is applied by reading it. What comes back is a validated
    /// document the caller still has to put through the preview-and-confirm
    /// gate, which is the only thing that can change a binding.
    pub fn import_from(
        &self,
        path: &std::path::Path,
    ) -> Result<GestureProfiles, GestureStoreError> {
        match self.store.read_text(path)? {
            Some(text) => {
                GestureProfiles::from_json(&text).map_err(|source| GestureStoreError::Damaged {
                    path: path.display().to_string(),
                    source,
                })
            }
            None => Err(GestureStoreError::NoSuchFile(path.display().to_string())),
        }
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
    use crate::profiles::PROFILE_SCHEMA_VERSION;
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
    fn a_version_one_file_on_disk_is_migrated_rather_than_refused() {
        let (_guard, store) = store();
        // What a build before per-device profiles wrote: a bare configuration.
        std::fs::create_dir_all(store.settings().directory()).unwrap();
        std::fs::write(store.config_path(), mac_style().to_json()).unwrap();

        let profiles = store.load_profiles().unwrap();
        assert_eq!(profiles.schema_version, PROFILE_SCHEMA_VERSION);
        assert_eq!(profiles.global, mac_style());
        assert!(profiles.devices.is_empty());
        // Reading does not rewrite the file; saving is what moves it forward.
        store.save_profiles(&profiles).unwrap();
        assert_eq!(store.load_profiles().unwrap(), profiles);
    }

    #[test]
    fn saving_the_global_profile_leaves_every_device_profile_alone() {
        let (_guard, store) = store();
        let mut profiles = GestureProfiles::default();
        profiles
            .devices
            .insert("uniq:LEN-0001".to_string(), mac_style());
        store.save_profiles(&profiles).unwrap();

        store.save_config(&GestureConfig::default()).unwrap();
        let reopened = store.load_profiles().unwrap();
        assert_eq!(reopened.devices.get("uniq:LEN-0001"), Some(&mac_style()));
        assert_eq!(reopened.global, GestureConfig::default());
    }

    #[test]
    fn an_exported_document_is_read_back_as_the_same_bytes_and_the_same_profiles() {
        let (guard, store) = store();
        let mut profiles = GestureProfiles::default();
        profiles
            .devices
            .insert("uniq:LEN-0001".to_string(), mac_style());
        profiles.active_device = Some("uniq:LEN-0001".to_string());

        let path = guard.path().join("gestures-export.json");
        store.export_to(&path, &profiles).unwrap();
        let imported = store.import_from(&path).unwrap();
        assert_eq!(imported, profiles);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            imported.to_json(),
            "a re-export of an imported document is not the same bytes"
        );
    }

    #[test]
    fn an_import_of_a_file_that_is_not_there_or_will_not_parse_says_which() {
        let (guard, store) = store();
        let missing = guard.path().join("nothing.json");
        assert!(matches!(
            store.import_from(&missing),
            Err(GestureStoreError::NoSuchFile(_))
        ));

        let hostile = guard.path().join("hostile.json");
        std::fs::write(&hostile, r#"{"schema_version":2,"global":{"schema_version":1,"enabled":true,"preset":"custom","gestures":[]},"devices":{"../etc":{"schema_version":1,"enabled":true,"preset":"custom","gestures":[]}}}"#).unwrap();
        assert!(matches!(
            store.import_from(&hostile),
            Err(GestureStoreError::Damaged { .. })
        ));
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

    /// The resident pipeline recognizes against the profile the window is
    /// editing. A pad that has diverged from the global profile must therefore
    /// read back as itself, and a pad that follows the global one must not
    /// read back as an empty profile of its own.
    #[test]
    fn the_active_configuration_is_the_selected_devices_own_profile() {
        let (_guard, store) = store();
        let mut profiles = GestureProfiles::default();
        profiles.global.enabled = false;
        profiles.detach("uniq:e3:2a:6d:11:04:9f").enabled = true;
        profiles.active_device = Some("uniq:e3:2a:6d:11:04:9f".to_string());
        store.save_profiles(&profiles).unwrap();

        assert!(store.load_active_config().unwrap().enabled);
        // The global profile is still what a caller with no device means.
        assert!(!store.load_config().unwrap().enabled);

        // A pad following the global profile gets the global profile.
        profiles.active_device = None;
        store.save_profiles(&profiles).unwrap();
        assert!(!store.load_active_config().unwrap().enabled);
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
