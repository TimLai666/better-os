//! The columns view (Ctrl+3), as Finder's and folio's (`src/FolderColumns.qml`):
//! the folders above this one side by side, each with the next one along
//! the way selected; this folder; and what the selected folder holds.
//!
//! The columns other than this folder's are read here, directly and
//! briefly (at most [`COLUMN_LIMIT`] names each), because they are small
//! listings of folders the person just walked through.

use std::path::{Path, PathBuf};

/// Names past this are not read into a side column.
pub const COLUMN_LIMIT: usize = 1_000;

/// How many folders above this one are shown.
pub const ANCESTORS: usize = 3;

/// The folders whose columns stand to the left of `folder`, oldest first:
/// from the home folder when `folder` is under it, from `/` otherwise, at
/// most [`ANCESTORS`] of them.
pub fn ancestor_columns(folder: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let stop = home.filter(|home| folder.starts_with(home) && folder != *home);
    let mut chain: Vec<PathBuf> = Vec::new();
    let mut at = folder.parent();
    while let Some(parent) = at {
        chain.push(parent.to_path_buf());
        if Some(parent) == stop {
            break;
        }
        at = parent.parent();
    }
    chain.reverse();
    let skip = chain.len().saturating_sub(ANCESTORS);
    chain.split_off(skip)
}

/// One name in a side column.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_folder: bool,
}

/// What a folder holds, folders first then by name ignoring case, hidden
/// names only when `show_hidden`, at most [`COLUMN_LIMIT`].
pub fn column_entries(folder: &Path, show_hidden: bool) -> Vec<ColumnEntry> {
    let Ok(read) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut entries: Vec<ColumnEntry> = read
        .flatten()
        .take(COLUMN_LIMIT * 4)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            Some(ColumnEntry {
                is_folder: path.is_dir(),
                name,
                path,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_folder
            .cmp(&a.is_folder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries.truncate(COLUMN_LIMIT);
    entries
}
