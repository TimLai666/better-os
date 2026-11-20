//! What removing Better Touchpad puts back (ticket 88).
//!
//! Every touchpad setting Better Touchpad changed was captured before its
//! first change. Removing the component writes those captured values back, so
//! nothing it set outlives it (AGENTS.md: "keybindings and defaults it set,
//! restored to what they were"). Better Manager runs this as the person, after
//! the component's services have stopped and right before the package goes,
//! through the `before-removal` program the package ships
//! (`better-touchpad-before-removal`).
//!
//! Nothing here can make removal impossible:
//!
//! - a capture that cannot be read, or a write the desktop refuses, is
//!   reported as "not restored" and the removal goes on;
//! - with no session to write in (a removal over SSH), the capture is copied
//!   to a pending restore outside the component's own data, which Better
//!   Manager finishes the next time it starts in the person's session
//!   ([`finish_pending`]);
//! - a removal the privileged service then refuses runs `--undo`, which writes
//!   back what the restore replaced (saved first, in [`UNDO_FILE`]) or drops
//!   the pending restore, so the component stays exactly as it was.
//!
//! Gestures need nothing here: GNOME's own swipe gestures come back when the
//! adapter extension is turned off, which Better Manager also does, and no
//! keyboard shortcut is ever written by Better Touchpad.

use std::path::{Path, PathBuf};

use touchpad_core::{Backup, RestorePlan, RestoreScope, RunState, SettingId, TouchpadStore};

use crate::TouchpadBackend;

/// What the restore replaced, kept beside the capture until the removal
/// either happens (and the directory goes, or stays as the person's data) or
/// is refused (and `--undo` writes it back).
pub const UNDO_FILE: &str = "removal-undo.json";

/// The package this module belongs to, as Better Manager's record names it.
pub const PACKAGE: &str = "better-touchpad";

/// What one run did, which the program turns into its exit status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PutBack {
    /// No capture: Better Touchpad never changed a setting.
    NothingChanged,
    /// The captured values were written back, with this result.
    Restored(RunState),
    /// The capture could not be read, so nothing was written.
    Unreadable(String),
    /// No session to write in; the capture waits in the pending restore.
    Deferred,
}

/// The pending restore for a session-less removal: the capture itself,
/// under the person's state directory, which "Also delete my settings and
/// history" does not remove (it is not the component's data).
pub fn pending_path(state_home: &Path) -> PathBuf {
    state_home
        .join("better-os")
        .join("pending-restore")
        .join("better-touchpad.json")
}

/// `$XDG_STATE_HOME`, or `~/.local/state`.
pub fn user_state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
}

fn write_json(path: &Path, backup: &Backup) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(backup).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn read_json(path: &Path) -> Result<Option<Backup>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| format!("{} could not be read: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{} could not be read: {error}", path.display())),
    }
}

fn restore(backup: &Backup, backend: &mut dyn TouchpadBackend) -> RunState {
    let capabilities = backend.capabilities().clone();
    let plan = RestorePlan::from_backup(backup, RestoreScope::All, |setting| {
        capabilities.support(setting)
    });
    backend.restore(&plan).state()
}

/// Why nothing can be put back in this session although a capture exists:
/// the session's dconf database could not be decided. The sentence is the
/// person's (Better Manager shows the last line in Activity with the exit 3
/// warning); with no capture there is nothing to put back and no warning.
pub fn refused_before_removal(
    store: &TouchpadStore,
    backend: &crate::GnomeBackend,
) -> Option<String> {
    let reason = backend.profile_refusal()?;
    if matches!(store.load_backup(), Ok(None)) {
        return None;
    }
    Some(
        crate::dconf_profile::sentence_for_environment(reason)
            .unwrap_or(reason)
            .to_string(),
    )
}

/// The run before removal. `reachable` is whether the desktop's settings
/// can be written in this session.
pub fn put_back_before_removal(
    store: &TouchpadStore,
    backend: &mut dyn TouchpadBackend,
    reachable: bool,
    state_home: &Path,
) -> PutBack {
    // An undo file is only ever about this run. One an earlier removal left
    // (kept with the person's data) would make a refused removal's `--undo`
    // write that older run's values back.
    match std::fs::remove_file(store.directory().join(UNDO_FILE)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return PutBack::Unreadable(format!("an old undo file could not be removed: {error}"));
        }
    }
    let backup = match store.load_backup() {
        Ok(Some(backup)) => backup,
        Ok(None) => return PutBack::NothingChanged,
        Err(error) => return PutBack::Unreadable(error.to_string()),
    };
    if !reachable {
        return match write_json(&pending_path(state_home), &backup) {
            Ok(()) => PutBack::Deferred,
            Err(error) => {
                PutBack::Unreadable(format!("the pending restore could not be kept: {error}"))
            }
        };
    }
    // What is there now goes aside first, so a refused removal can have it
    // back. Only the settings the capture covers are read.
    let settings: Vec<SettingId> = backup.readings().map(|(setting, _)| setting).collect();
    let now = Backup::capture(backend.name(), None, backend.read(&settings), 0);
    if let Err(error) = write_json(&store.directory().join(UNDO_FILE), &now) {
        return PutBack::Unreadable(format!("what is there now could not be kept: {error}"));
    }
    PutBack::Restored(restore(&backup, backend))
}

/// `--undo`: the removal did not happen, so what the run before replaced is
/// written back, and a pending restore is dropped. Returns whether anything
/// failed.
pub fn undo_before_removal(
    store: &TouchpadStore,
    backend: &mut dyn TouchpadBackend,
    state_home: &Path,
) -> Result<(), String> {
    match std::fs::remove_file(pending_path(state_home)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let undo = store.directory().join(UNDO_FILE);
    let Some(replaced) = read_json(&undo)? else {
        return Ok(());
    };
    if restore(&replaced, backend) == RunState::Failed {
        return Err("the touchpad settings could not be written again".to_string());
    }
    std::fs::remove_file(&undo).map_err(|error| error.to_string())
}

/// Finishes a restore a session-less removal left, now that there is a
/// session. Returns whether one was finished; a failed write leaves it for
/// the next start.
///
/// When Better Touchpad is installed again before this ran, the pending
/// restore is dropped unapplied: the component is back and its settings are
/// its own again, and writing the old capture would undo them.
pub fn finish_pending(
    backend: &mut dyn TouchpadBackend,
    reachable: bool,
    state_home: &Path,
    installed_again: bool,
) -> Result<bool, String> {
    let path = pending_path(state_home);
    if installed_again {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.to_string()),
        };
    }
    let Some(backup) = read_json(&path)? else {
        return Ok(false);
    };
    if !reachable {
        return Ok(false);
    }
    if restore(&backup, backend) == RunState::Failed {
        return Err("the touchpad settings could not be put back yet".to_string());
    }
    std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    Ok(true)
}

/// What Better Manager's start runs, in the window and the command line: in
/// this session's real desktop settings. The caller keeps tests away from it.
#[cfg(feature = "dconf-write")]
pub fn finish_pending_in_session(installed_again: bool) -> Result<bool, String> {
    let state_home = user_state_home();
    if !pending_path(&state_home).exists() {
        return Ok(false);
    }
    let mut backend = crate::GnomeBackend::connect(None);
    let reachable = backend.status().reachable;
    finish_pending(&mut backend, reachable, &state_home, installed_again)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockBackend;
    use touchpad_core::{Reading, SettingValue};

    fn captured_tap_off(store: &TouchpadStore) {
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
    }

    // Review finding 4: a session whose dconf database could not be decided
    // gets the person's sentence and the exit 3 warning, not a pending restore
    // that waits for a session forever; with nothing captured, nothing at all.
    #[test]
    fn a_refused_session_profile_is_reported_only_when_something_was_captured() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path().to_path_buf());
        let backend = crate::GnomeBackend::without_database(
            None,
            &crate::dconf_profile::ProfileError::NotTheSessionEnvironment {
                desktop: "betteros:cosmic".to_string(),
            },
        );
        assert_eq!(refused_before_removal(&store, &backend), None);
        captured_tap_off(&store);
        let said = refused_before_removal(&store, &backend).unwrap();
        let (en, zh) =
            crate::dconf_profile::sentence("dconf.session_profile_not_in_environment").unwrap();
        assert!(said == en || said == zh, "{said}");
        let reachable = crate::GnomeBackend::read_only("/nonexistent/dconf/user", None);
        assert_eq!(refused_before_removal(&store, &reachable), None);
    }

    fn tap(backend: &MockBackend) -> Option<Reading> {
        backend
            .read(&[SettingId::TapToClick])
            .into_iter()
            .next()
            .map(|(_, reading)| reading)
    }

    #[test]
    fn no_capture_means_nothing_is_written() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path());
        let mut backend = MockBackend::new();
        assert_eq!(
            put_back_before_removal(&store, &mut backend, true, directory.path()),
            PutBack::NothingChanged
        );
        assert!(backend.writes.is_empty());
    }

    #[test]
    fn the_captured_values_go_back_and_undo_puts_ours_back_again() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path().join("touchpad"));
        std::fs::create_dir_all(store.directory()).unwrap();
        captured_tap_off(&store);
        let mut backend =
            MockBackend::new().holding(SettingId::TapToClick, SettingValue::toggle(true));

        let result = put_back_before_removal(&store, &mut backend, true, directory.path());
        assert_eq!(result, PutBack::Restored(RunState::Applied));
        assert_eq!(
            tap(&backend),
            Some(Reading::value(SettingValue::toggle(false)))
        );

        undo_before_removal(&store, &mut backend, directory.path()).unwrap();
        assert_eq!(
            tap(&backend),
            Some(Reading::value(SettingValue::toggle(true)))
        );
        assert!(!store.directory().join(UNDO_FILE).exists());
    }

    #[test]
    fn a_damaged_capture_is_reported_not_written() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path());
        std::fs::write(store.backup_path(), "{ not json").unwrap();
        let mut backend = MockBackend::new();
        assert!(matches!(
            put_back_before_removal(&store, &mut backend, true, directory.path()),
            PutBack::Unreadable(_)
        ));
        assert!(backend.writes.is_empty());
    }

    #[test]
    fn with_no_session_the_restore_waits_and_the_next_start_finishes_it() {
        let directory = tempfile::tempdir().unwrap();
        let state_home = directory.path().join("state");
        let store = TouchpadStore::new(directory.path().join("touchpad"));
        std::fs::create_dir_all(store.directory()).unwrap();
        captured_tap_off(&store);
        let mut backend =
            MockBackend::new().holding(SettingId::TapToClick, SettingValue::toggle(true));

        let result = put_back_before_removal(&store, &mut backend, false, &state_home);
        assert_eq!(result, PutBack::Deferred);
        assert!(backend.writes.is_empty());
        // "Also delete my settings and history" removes the component's data.
        std::fs::remove_dir_all(store.directory()).unwrap();

        // Still no session: it keeps waiting.
        assert!(!finish_pending(&mut backend, false, &state_home, false).unwrap());
        assert!(pending_path(&state_home).exists());
        // In the person's session, it is finished and forgotten.
        assert!(finish_pending(&mut backend, true, &state_home, false).unwrap());
        assert_eq!(
            tap(&backend),
            Some(Reading::value(SettingValue::toggle(false)))
        );
        assert!(!pending_path(&state_home).exists());
        assert!(!finish_pending(&mut backend, true, &state_home, false).unwrap());
    }

    #[test]
    fn undo_of_a_deferred_run_drops_the_pending_restore() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path().join("touchpad"));
        std::fs::create_dir_all(store.directory()).unwrap();
        captured_tap_off(&store);
        let mut backend = MockBackend::new();
        assert_eq!(
            put_back_before_removal(&store, &mut backend, false, directory.path()),
            PutBack::Deferred
        );
        undo_before_removal(&store, &mut backend, directory.path()).unwrap();
        assert!(!pending_path(directory.path()).exists());
        assert!(backend.writes.is_empty());
    }

    #[test]
    fn a_stale_undo_file_is_never_written_back() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path().join("touchpad"));
        std::fs::create_dir_all(store.directory()).unwrap();
        captured_tap_off(&store);
        // An earlier removal's undo file, kept with the person's data.
        let stale = Backup::capture(
            "gnome",
            None,
            vec![(
                SettingId::TapToClick,
                Reading::value(SettingValue::toggle(true)),
            )],
            0,
        );
        write_json(&store.directory().join(UNDO_FILE), &stale).unwrap();
        let mut backend = MockBackend::new();

        // This removal has no session, so it is deferred; the daemon then
        // refuses the removal and `--undo` runs.
        assert_eq!(
            put_back_before_removal(&store, &mut backend, false, directory.path()),
            PutBack::Deferred
        );
        undo_before_removal(&store, &mut backend, directory.path()).unwrap();
        assert!(backend.writes.is_empty(), "{:?}", backend.writes);

        // The same with a capture that cannot be read.
        write_json(&store.directory().join(UNDO_FILE), &stale).unwrap();
        std::fs::write(store.backup_path(), "{ not json").unwrap();
        assert!(matches!(
            put_back_before_removal(&store, &mut backend, true, directory.path()),
            PutBack::Unreadable(_)
        ));
        undo_before_removal(&store, &mut backend, directory.path()).unwrap();
        assert!(backend.writes.is_empty(), "{:?}", backend.writes);
    }

    #[test]
    fn a_reinstall_drops_the_pending_restore_unapplied() {
        let directory = tempfile::tempdir().unwrap();
        let store = TouchpadStore::new(directory.path().join("touchpad"));
        std::fs::create_dir_all(store.directory()).unwrap();
        captured_tap_off(&store);
        let mut backend = MockBackend::new();
        assert_eq!(
            put_back_before_removal(&store, &mut backend, false, directory.path()),
            PutBack::Deferred
        );

        assert!(!finish_pending(&mut backend, true, directory.path(), true).unwrap());
        assert!(backend.writes.is_empty());
        assert!(!pending_path(directory.path()).exists());
    }
}
