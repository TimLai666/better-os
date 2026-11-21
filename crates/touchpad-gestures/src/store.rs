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
    #[error("{source} ({path})")]
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

    /// Where the lock every writer of `gestures.json` takes lives.
    pub fn lock_path(&self) -> std::path::PathBuf {
        self.store.directory().join("gestures.lock")
    }

    /// Reads the profiles, lets `change` alter them, and writes them back, with
    /// every other writer of the file kept out from the read to the write
    /// (ticket 131).
    ///
    /// The window and the gesture service both write through this, each
    /// merging only its own changes onto what it reads here
    /// ([`crate::merge::merge`]). The lock is `flock(2)` on `gestures.lock`
    /// beside the file, which the kernel releases when the holder exits, so a
    /// writer that crashes cannot leave the other locked out. A file that will
    /// not parse is never written over: it is the only copy. Returns what the
    /// file holds afterwards.
    pub fn update_profiles(
        &self,
        change: impl FnOnce(&mut GestureProfiles),
    ) -> Result<GestureProfiles, GestureStoreError> {
        std::fs::create_dir_all(self.store.directory()).map_err(|source| StoreError::Io {
            path: self.store.directory().to_path_buf(),
            source,
        })?;
        let lock_path = self.lock_path();
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|source| StoreError::Io {
                path: lock_path.clone(),
                source,
            })?;
        lock.lock().map_err(|source| StoreError::Io {
            path: lock_path.clone(),
            source,
        })?;
        let before = self.load_profiles()?;
        let mut after = before.clone();
        change(&mut after);
        if after != before {
            self.save_profiles(&after)?;
        }
        // Dropping the file releases the lock.
        drop(lock);
        Ok(after)
    }

    /// Writes the changes `ours` made to `base` onto the file as it is now,
    /// under the lock, with [`crate::merge::merge`]: the one way both the
    /// window and the gesture service save (ticket 131). A file that does not
    /// exist yet is taken to hold `base`, so a first save writes `ours` whole.
    /// Returns what the file holds afterwards.
    pub fn save_changes(
        &self,
        base: &GestureProfiles,
        ours: &GestureProfiles,
    ) -> Result<GestureProfiles, GestureStoreError> {
        let path = self.config_path();
        self.update_profiles(|theirs| {
            if !path.exists() {
                *theirs = base.clone();
            }
            *theirs = crate::merge::merge(base, ours, theirs);
        })
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

    /// Puts the shipped Mac-style preset in force the first time the gesture
    /// service runs for this person, so installing Better Touchpad is enough
    /// for gestures to work (owner, 2026-11-13, ticket 88). Returns whether it
    /// wrote anything.
    ///
    /// "First time" is "there is no gesture file at all". A file that exists
    /// is the person's (or an earlier run's) decision, including one with
    /// every gesture turned off, and is never replaced. The capture records
    /// the empty shipped configuration before the preset is written, so
    /// restoring Gestures goes back to what the desktop had before Better OS:
    /// no gesture of ours.
    pub fn apply_first_run_default(&self) -> Result<bool, GestureStoreError> {
        if self.store.read_text(&self.config_path())?.is_some() {
            return Ok(false);
        }
        self.capture_once(&GestureConfig::default())?;
        let profiles = GestureProfiles {
            global: crate::preset::mac_style(),
            ..GestureProfiles::default()
        };
        self.save_profiles(&profiles)?;
        Ok(true)
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

    /// The marker the gesture pipeline leaves when it turned gestures off by
    /// itself after the adapter failed several times in a row. It is what
    /// lets Better Manager's Home say so after the fact (ticket 88): the
    /// profiles only record that gestures are off, not why.
    pub fn turned_off_path(&self) -> std::path::PathBuf {
        self.store.directory().join("gestures-turned-off")
    }

    /// Records that gestures turned themselves off after `failures` failed
    /// runs in a row.
    pub fn record_turned_off(&self, failures: u32) -> Result<(), GestureStoreError> {
        self.store
            .write_text(
                &self.turned_off_path(),
                &format!(
                    "Gestures were turned off after {failures} failed attempts in a row.\n\
                     Turning them on again in Better Manager removes this file.\n"
                ),
            )
            .map_err(Into::into)
    }

    pub fn turned_off_by_itself(&self) -> bool {
        self.turned_off_path().exists()
    }

    /// Forgets the marker: gestures are on again, or the person turned them
    /// off themselves, which is their own choice and not a fault.
    pub fn clear_turned_off(&self) -> Result<(), GestureStoreError> {
        let path = self.turned_off_path();
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StoreError::Io {
                path,
                source: error,
            }
            .into()),
        }
    }

    /// The settings store this one sits beside, for a caller that needs both.
    pub fn settings(&self) -> &TouchpadStore {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_turned_off_marker_is_written_read_and_cleared() {
        let directory = tempfile::tempdir().unwrap();
        let store = GestureStore::at(directory.path().join("touchpad"));
        assert!(!store.turned_off_by_itself());
        // Clearing what is not there is not an error.
        store.clear_turned_off().unwrap();
        store.record_turned_off(3).unwrap();
        assert!(store.turned_off_by_itself());
        store.clear_turned_off().unwrap();
        assert!(!store.turned_off_by_itself());
    }
    use crate::conflict::{ConflictResolution, GNOME_46_GESTURES};
    use crate::plan::PresetPlan;
    use crate::preset::mac_style;
    use crate::profiles::PROFILE_SCHEMA_VERSION;
    use better_actions::DesktopAction;
    use touchpad_session::MockSessionAdapter;

    /// Ticket 131: the window and the service write the same file. Each read,
    /// merge, and write happens under the one lock, so two writers interleaving
    /// as tightly as threads can never lose each other's change.
    #[test]
    fn two_writers_interleaving_never_lose_a_change() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("touchpad");
        let writer = |prefix: &'static str| {
            let root = root.clone();
            std::thread::spawn(move || {
                let store = GestureStore::at(&root);
                for index in 0..30 {
                    store
                        .update_profiles(|profiles| {
                            // Widen the window between the read and the write.
                            std::thread::sleep(std::time::Duration::from_millis(1));
                            profiles.global.gestures.push(
                                crate::definition::GestureDefinition::new(
                                    &format!("{prefix}-{index}"),
                                    crate::definition::GestureShape::Swipe,
                                    5,
                                    false,
                                    Some(crate::definition::Direction::Up),
                                    DesktopAction::ShowOverview,
                                )
                                .unwrap(),
                            );
                        })
                        .unwrap();
                }
            })
        };
        let first = writer("window");
        let second = writer("service");
        first.join().unwrap();
        second.join().unwrap();
        let stored = GestureStore::at(&root).load_profiles().unwrap();
        assert_eq!(stored.global.gestures.len(), 60);
    }

    #[test]
    fn an_update_never_writes_over_a_file_that_will_not_parse() {
        let directory = tempfile::tempdir().unwrap();
        let store = GestureStore::at(directory.path());
        std::fs::write(store.config_path(), "{ not json").unwrap();
        assert!(
            store
                .update_profiles(|profiles| profiles.global.enabled = false)
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(store.config_path()).unwrap(),
            "{ not json"
        );
    }

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
    fn the_first_run_default_is_the_mac_style_preset_with_an_empty_capture() {
        let (_guard, store) = store();
        assert!(store.apply_first_run_default().unwrap());
        assert_eq!(store.load_config().unwrap(), mac_style());
        assert_eq!(
            store.load_capture().unwrap(),
            Some(GestureConfig::default())
        );
        // A second run changes nothing.
        assert!(!store.apply_first_run_default().unwrap());
    }

    #[test]
    fn the_first_run_default_never_replaces_a_saved_file() {
        let (_guard, store) = store();
        let mut off = mac_style();
        off.enabled = false;
        store.save_config(&off).unwrap();
        assert!(!store.apply_first_run_default().unwrap());
        assert_eq!(store.load_config().unwrap(), off);
        assert!(!store.has_capture());
    }

    #[test]
    fn the_first_run_default_refuses_a_damaged_file_rather_than_replacing_it() {
        let (_guard, store) = store();
        std::fs::create_dir_all(store.settings().directory()).unwrap();
        std::fs::write(store.config_path(), "{ not json").unwrap();
        assert!(!store.apply_first_run_default().unwrap());
        assert_eq!(
            std::fs::read_to_string(store.config_path()).unwrap(),
            "{ not json"
        );
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
