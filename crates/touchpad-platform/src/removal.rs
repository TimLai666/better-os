//! What removing Better Touchpad puts back (ticket 88).
//!
//! Every touchpad setting Better Touchpad changed was captured before its
//! first change. Removing the component writes those captured values back, so
//! nothing it set outlives it (AGENTS.md: "keybindings and defaults it set,
//! restored to what they were"). Better Manager runs this as the person,
//! before the package goes, through the `before-removal` program the package
//! ships (`better-touchpad-before-removal`).
//!
//! Gestures need nothing here: GNOME's own swipe gestures come back when the
//! adapter extension is turned off, which Better Manager also does, and no
//! keyboard shortcut is ever written by Better Touchpad.

use touchpad_core::{RestorePlan, RestoreScope, RunState, StoreError, TouchpadStore};

use crate::TouchpadBackend;

/// What removal found to do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PutBack {
    /// No capture: Better Touchpad never changed a setting.
    NothingChanged,
    /// The captured values were written back, with this result.
    Restored(RunState),
}

/// Writes every captured setting back through `backend`.
pub fn put_back_before_removal(
    store: &TouchpadStore,
    backend: &mut dyn TouchpadBackend,
) -> Result<PutBack, StoreError> {
    let Some(backup) = store.load_backup()? else {
        return Ok(PutBack::NothingChanged);
    };
    let capabilities = backend.capabilities().clone();
    let plan = RestorePlan::from_backup(&backup, RestoreScope::All, |setting| {
        capabilities.support(setting)
    });
    Ok(PutBack::Restored(backend.restore(&plan).state()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockBackend;
    use touchpad_core::{Backup, Reading, SettingId, SettingValue};

    #[test]
    fn no_capture_means_nothing_is_written() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path());
        let mut backend = MockBackend::new();
        assert_eq!(
            put_back_before_removal(&store, &mut backend).unwrap(),
            PutBack::NothingChanged
        );
        assert!(backend.writes.is_empty());
    }

    #[test]
    fn the_captured_values_are_written_back() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path());
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
        let mut backend =
            MockBackend::new().holding(SettingId::TapToClick, SettingValue::toggle(true));

        let result = put_back_before_removal(&store, &mut backend).unwrap();

        assert_eq!(result, PutBack::Restored(RunState::Applied));
        assert_eq!(
            backend.writes,
            vec![(SettingId::TapToClick, Some(SettingValue::toggle(false)))]
        );
    }

    #[test]
    fn a_damaged_capture_is_an_error_not_a_silent_success() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path());
        std::fs::write(store.backup_path(), "{ not json").unwrap();
        assert!(put_back_before_removal(&store, &mut MockBackend::new()).is_err());
    }
}
