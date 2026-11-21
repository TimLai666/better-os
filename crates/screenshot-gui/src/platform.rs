//! What Better Screenshot reads and writes on the machine: the Pictures and
//! Videos folders (through `files_platform::xdg`, the one reader of
//! `xdg-user-dirs`), its state file, the recently used files, and the
//! shutter sound.

use std::path::{Path, PathBuf};

use files_core::location::Location;
use files_platform::{UserDirectories, UserDirectory};
use screenshot_core::files::{RecentEntry, record_recent, xbel_stamp};
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

/// `$XDG_STATE_HOME/better-os/screenshot/state.json`.
pub fn state_path() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/state"))
        })?;
    Some(state.join("better-os/screenshot/state.json"))
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
/// the first of `$XDG_DATA_DIRS/sounds/<theme>/stereo/screen-capture.*`
/// in the freedesktop theme.
pub fn shutter_sound() -> Option<PathBuf> {
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for dir in dirs.split(':').filter(|dir| !dir.is_empty()) {
        for extension in ["oga", "ogg", "wav"] {
            let path = Path::new(dir)
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
    use super::{load_remembered, save_remembered};
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
