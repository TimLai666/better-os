//! The repositories opened before, kept in Better Git's own file under the
//! XDG state directory: `$XDG_STATE_HOME/better-os/git-recent.json`, or
//! `~/.local/state/better-os/git-recent.json`.
//!
//! The file is a convenience. A missing or unreadable one is an empty list,
//! never an error that stops the window. Every function takes the file's path
//! so tests write only to a temporary directory.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many repositories the list keeps.
pub const LIMIT: usize = 10;

#[derive(Debug, Default, Deserialize, Serialize)]
struct RecentFile {
    repositories: Vec<String>,
}

/// Where the list lives for this user, or `None` when neither
/// `XDG_STATE_HOME` nor `HOME` says. A relative `XDG_STATE_HOME` is ignored,
/// as the XDG specification asks.
pub fn default_path() -> Option<PathBuf> {
    state_path(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// [`default_path`] with the two variables passed in.
pub fn state_path(xdg_state_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let state = xdg_state_home
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|path| path.is_absolute())
                .map(|home| home.join(".local/state"))
        })?;
    Some(state.join("better-os/git-recent.json"))
}

/// Reads the list. A file that is missing, unreadable, or not the expected
/// JSON is an empty list. Entries that are not absolute paths are dropped,
/// and the list is cut to [`LIMIT`].
pub fn load(file: &Path) -> Vec<PathBuf> {
    let Ok(bytes) = std::fs::read(file) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_slice::<RecentFile>(&bytes) else {
        return Vec::new();
    };
    let mut list = Vec::new();
    for entry in parsed.repositories {
        let path = PathBuf::from(entry);
        if path.is_absolute() && !list.contains(&path) {
            list.push(path);
        }
    }
    list.truncate(LIMIT);
    list
}

/// Writes the list, creating the directory when it is missing. The file is
/// written beside its final name and then renamed, so a crash part-way leaves
/// the old list rather than half a new one. A path that is not valid UTF-8
/// cannot be written as a JSON string and is left out.
pub fn save(file: &Path, list: &[PathBuf]) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let document = RecentFile {
        repositories: list
            .iter()
            .filter_map(|path| path.to_str().map(str::to_string))
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(io::Error::other)?;
    let mut partial = file.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, file)
}

/// The list with `repository` moved to the front, without duplicates, cut to
/// [`LIMIT`].
pub fn remember(list: &[PathBuf], repository: PathBuf) -> Vec<PathBuf> {
    let mut next: Vec<PathBuf> = list
        .iter()
        .filter(|path| **path != repository)
        .cloned()
        .collect();
    next.insert(0, repository);
    next.truncate(LIMIT);
    next
}
