//! What Better Screenshot reads and writes on the machine: the Pictures and
//! Videos folders (through `files_platform::xdg`, the one reader of
//! `xdg-user-dirs`), its state file, the recently used files, the lockdown
//! key, and the shutter sound.

use std::path::{Path, PathBuf};

use defaults_platform::dconf_profile::ProfileError;
use defaults_platform::dconf_stack::DatabaseStack;
use files_core::location::Location;
use files_platform::{UserDirectories, UserDirectory};
use screenshot_core::files::{RecentEntry, record_recent, xbel_stamp};
use screenshot_core::lockdown::Saving;
use screenshot_core::session::Remembered;

/// The Pictures or Videos folder, or the home folder when there is neither.
pub fn user_folder(directory: UserDirectory) -> Option<PathBuf> {
    let directories = UserDirectories::from_env();
    let local = |location: &Location| match location {
        Location::Local(path) => Some(path.as_path().to_path_buf()),
        _ => None,
    };
    directories
        .location(directory)
        .and_then(local)
        .or_else(|| directories.home().and_then(local))
}

/// `$XDG_RUNTIME_DIR/better-os/screenshot/state.json`: what is remembered
/// lasts as long as the login, as GNOME keeps it in the shell's memory for
/// the session (spec C9, P1). The runtime directory goes at logout, so the
/// next login starts with GNOME's defaults. Without a private runtime
/// directory nothing is remembered beyond the process.
pub fn state_path() -> Option<PathBuf> {
    match better_core::private_dir::private_session_dir(&["better-os", "screenshot"]) {
        Ok(dir) => Some(dir.join("state.json")),
        Err(error) => {
            eprintln!("better-screenshot: nothing is remembered: {error}");
            None
        }
    }
}

pub fn load_remembered(path: Option<&Path>) -> Remembered {
    path.and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| Remembered::parse(&text))
        .unwrap_or_default()
}

pub fn save_remembered(path: Option<&Path>, remembered: &Remembered) {
    let Some(path) = path else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temporary = path.with_extension("json.tmp");
    if std::fs::write(&temporary, remembered.to_text()).is_ok() {
        let _ = std::fs::rename(&temporary, path);
    }
}

/// Whether saving to disk is allowed (`org.gnome.desktop.lockdown
/// disable-save-to-disk`), read through the profile dconf puts in force with
/// its system databases and locks (`defaults_platform::dconf_stack`). Read
/// at each request, as GNOME reads it.
pub fn saving() -> Saving {
    let stack = defaults_platform::dconf_stack::session_stack();
    if let Ok(stack) = &stack {
        for note in &stack.notes {
            eprintln!("better-screenshot: {note}");
        }
    }
    saving_from(stack)
}

/// The decision over a read stack:
///
/// - The key true is disabled, whatever else was noted.
/// - A database that could not be read, or whose locks could not, leaves
///   the answer unknown: it might have set or locked the key.
/// - `DCONF_PROFILE` naming a profile that is missing or unreadable is
///   dconf's null configuration (`dconf-engine-profile.c`, "using the null
///   configuration"), where the schema's default, false, is what GSettings
///   shows: allowed.
/// - Any other profile dconf could not be followed through is unknown.
///
/// Unknown is never turned into allowed (AGENTS.md); the caller treats it as
/// disabled and says why.
pub fn saving_from(stack: Result<DatabaseStack, ProfileError>) -> Saving {
    match stack {
        Ok(stack) => {
            if stack.boolean(screenshot_core::lockdown::KEY, false) {
                Saving::Disabled
            } else if !stack.notes.is_empty() {
                Saving::Unknown(stack.notes.join("; "))
            } else {
                Saving::Allowed
            }
        }
        Err(ProfileError::NamedProfileMissing { .. } | ProfileError::Unreadable { .. }) => {
            Saving::Allowed
        }
        Err(error) => Saving::Unknown(error.to_string()),
    }
}

/// `$XDG_DATA_HOME/recently-used.xbel`.
pub fn recent_list() -> Option<PathBuf> {
    files_platform::recent::recent_file_from_env()
}

/// Adds a saved file to the recently used files (spec S5, R7).
pub fn add_recent(path: &Path, mime: &'static str) {
    let Some(list) = recent_list() else { return };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let entry = RecentEntry {
        uri: better_core::uri::file_uri(path),
        mime,
        stamp: xbel_stamp(now),
        application: "Better Screenshot",
        exec: "better-screenshot",
    };
    if let Err(error) = record_recent(&list, &entry) {
        eprintln!("better-screenshot: not added to recent files: {error}");
    }
}

/// The sound theme's `screen-capture` sound (the freedesktop sound naming
/// specification's name, which GNOME plays, `js/ui/screenshot.js` 2392):
/// the first `sounds/freedesktop/stereo/screen-capture.*` in the XDG data
/// directories (`files_platform::xdg::data_dirs_from_env`, the one reading
/// of that rule).
pub fn shutter_sound() -> Option<PathBuf> {
    for dir in files_platform::xdg::data_dirs_from_env() {
        for extension in ["oga", "ogg", "wav"] {
            let path = dir
                .join("sounds/freedesktop/stereo")
                .join(format!("screen-capture.{extension}"));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// Plays the shutter sound on a thread of its own, through GStreamer's
/// `playbin`; a machine without the sound or the player is silent.
pub fn play_shutter() {
    let Some(path) = shutter_sound() else { return };
    let _ = std::thread::Builder::new()
        .name("shutter".into())
        .spawn(move || {
            use gstreamer::prelude::*;
            if gstreamer::init().is_err() {
                return;
            }
            let Ok(playbin) = gstreamer::ElementFactory::make("playbin").build() else {
                return;
            };
            playbin.set_property("uri", better_core::uri::file_uri(&path));
            if playbin.set_state(gstreamer::State::Playing).is_err() {
                return;
            }
            if let Some(bus) = playbin.bus() {
                for message in bus.iter_timed(gstreamer::ClockTime::from_seconds(5)) {
                    match message.view() {
                        gstreamer::MessageView::Eos(_) | gstreamer::MessageView::Error(_) => break,
                        _ => {}
                    }
                }
            }
            let _ = playbin.set_state(gstreamer::State::Null);
        });
}

#[cfg(test)]
mod tests {
    use super::{load_remembered, save_remembered, saving_from};
    use defaults_platform::dconf_profile::ProfileError;
    use defaults_platform::dconf_stack::DatabaseStack;
    use screenshot_core::lockdown::Saving;

    #[test]
    fn only_dconfs_null_configuration_reads_as_allowed() {
        let empty = DatabaseStack {
            sources: Vec::new(),
            notes: Vec::new(),
        };
        assert_eq!(saving_from(Ok(empty)), Saving::Allowed);
        let noted = DatabaseStack {
            sources: Vec::new(),
            notes: vec!["system-db:local is not readable".into()],
        };
        assert!(matches!(saving_from(Ok(noted)), Saving::Unknown(_)));
        let missing = ProfileError::NamedProfileMissing {
            name: "betteros".into(),
            searched: Vec::new(),
        };
        assert_eq!(saving_from(Err(missing)), Saving::Allowed);
        let other = ProfileError::InvalidDatabaseName { name: "a/b".into() };
        assert!(matches!(saving_from(Err(other)), Saving::Unknown(_)));
    }

    use screenshot_core::geometry::Rect;
    use screenshot_core::session::Remembered;

    #[test]
    fn the_state_file_round_trips_and_a_missing_one_is_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("better-os/screenshot/state.json");
        assert_eq!(load_remembered(Some(&path)), Remembered::default());
        let remembered = Remembered {
            area: Some(Rect::new(5, 6, 7, 8)),
            ..Remembered::default()
        };
        save_remembered(Some(&path), &remembered);
        assert_eq!(load_remembered(Some(&path)), remembered);
        assert_eq!(load_remembered(None), Remembered::default());
    }
}
