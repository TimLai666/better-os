//! The navigation chrome, decided from the history rather than from clicks.
//!
//! Back, Forward, and Up are enabled by asking `files_core::History` what it
//! can do. Nothing here tracks its own idea of where the user has been, so a
//! button cannot be enabled while the navigation that would follow it is
//! refused — the two answers come from the same object.
//!
//! The path field is the other half: a typed string becomes a
//! `files_core::Location` or a stated reason, and navigation is attempted only
//! for the first case.

use std::path::{Path, PathBuf};

use files_core::{History, LocalPath, Location};

use crate::i18n::Copy;

/// What the toolbar draws for one tab.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolbarState {
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub can_go_to_parent: bool,
    /// The text the path field shows when it is not being edited.
    pub path_text: String,
    /// The current location, for the breadcrumb and for copy-path.
    pub location: Location,
}

/// Reads the toolbar's state off a tab's history.
pub fn toolbar_state(history: &History) -> ToolbarState {
    let location = history.current().clone();
    ToolbarState {
        can_go_back: history.can_go_back(),
        can_go_forward: history.can_go_forward(),
        can_go_to_parent: location.parent().is_some(),
        path_text: display_path(&location),
        location,
    }
}

/// The text the path field shows for a location.
///
/// A local path is shown as a path, because that is what a user types. A typed
/// location that has no path is shown as its URI, which is what `to_uri`
/// already round-trips, rather than as an invented path like `/Trash`.
pub fn display_path(location: &Location) -> String {
    match location.as_local_path() {
        Some(path) => path.as_path().to_string_lossy().into_owned(),
        None => location.to_uri(),
    }
}

/// Why a typed location was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathRejection {
    Empty,
    /// A relative path. Better Files has no working directory to resolve one
    /// against, and resolving against the current tab would silently open a
    /// different folder than the one that was typed.
    NotAbsolute,
    NotFound,
    NotADirectory,
    /// A scheme this build recognizes but cannot open, or does not recognize.
    Unsupported,
}

impl PathRejection {
    pub fn message(self, c: &'static Copy) -> &'static str {
        match self {
            PathRejection::Empty => c.path_empty,
            PathRejection::NotAbsolute => c.path_not_absolute,
            PathRejection::NotFound => c.path_not_found,
            PathRejection::NotADirectory => c.path_not_a_directory,
            PathRejection::Unsupported => c.path_unsupported,
        }
    }
}

/// What the filesystem says about a typed path. Injected so the validation
/// tests do not need real directories, and so a future build can move the
/// check off the render thread without changing the parser.
pub trait PathValidator {
    fn exists(&self, path: &Path) -> bool;
    fn is_directory(&self, path: &Path) -> bool;
}

/// The real answer.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemValidator;

impl PathValidator for FilesystemValidator {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_directory(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// A validator built from a fixed set of directories and files.
#[derive(Clone, Debug, Default)]
pub struct FixedValidator {
    directories: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl FixedValidator {
    pub fn new(
        directories: impl IntoIterator<Item = PathBuf>,
        files: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        Self {
            directories: directories.into_iter().collect(),
            files: files.into_iter().collect(),
        }
    }
}

impl PathValidator for FixedValidator {
    fn exists(&self, path: &Path) -> bool {
        self.directories.iter().any(|entry| entry == path) || self.files.iter().any(|f| f == path)
    }

    fn is_directory(&self, path: &Path) -> bool {
        self.directories.iter().any(|entry| entry == path)
    }
}

/// Turns what the user typed into a location, or into a reason it was refused.
///
/// Accepted forms, in the order they are tried: a `scheme://` URI, `~` and
/// `~/…` against the session's home, and an absolute path. Everything else is
/// refused with a reason rather than guessed at.
pub fn resolve_path_input(
    input: &str,
    home: Option<&Path>,
    validator: &dyn PathValidator,
) -> Result<Location, PathRejection> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(PathRejection::Empty);
    }

    if trimmed.contains("://") {
        let location = Location::parse_uri(trimmed);
        return match &location {
            Location::Local(path) => validate_local(path, validator),
            other if other.is_listable() => Ok(location),
            _ => Err(PathRejection::Unsupported),
        };
    }

    let expanded = expand_home(trimmed, home)?;
    if !expanded.is_absolute() {
        return Err(PathRejection::NotAbsolute);
    }
    let path = LocalPath::new(expanded).map_err(|_| PathRejection::NotAbsolute)?;
    validate_local(&path, validator)
}

fn validate_local(
    path: &LocalPath,
    validator: &dyn PathValidator,
) -> Result<Location, PathRejection> {
    if !validator.exists(path.as_path()) {
        return Err(PathRejection::NotFound);
    }
    if !validator.is_directory(path.as_path()) {
        return Err(PathRejection::NotADirectory);
    }
    Ok(Location::Local(path.clone()))
}

fn expand_home(input: &str, home: Option<&Path>) -> Result<PathBuf, PathRejection> {
    if input == "~" {
        return home
            .map(Path::to_path_buf)
            .ok_or(PathRejection::NotAbsolute);
    }
    if let Some(rest) = input.strip_prefix("~/") {
        let home = home.ok_or(PathRejection::NotAbsolute)?;
        return Ok(home.join(rest));
    }
    Ok(PathBuf::from(input))
}

/// The breadcrumb for a local path: each ancestor with the location it opens.
///
/// Built from the path's own components rather than from the history, so it
/// stays correct after a jump that skipped the intermediate folders.
pub fn breadcrumb(location: &Location) -> Vec<(String, Location)> {
    let Some(path) = location.as_local_path() else {
        return vec![(location.display_name(), location.clone())];
    };
    let mut crumbs = Vec::new();
    let mut current = PathBuf::from("/");
    crumbs.push(("/".to_string(), Location::Local(LocalPath::root())));
    for component in path.as_path().components().skip(1) {
        current.push(component.as_os_str());
        if let Ok(step) = LocalPath::new(current.clone()) {
            crumbs.push((
                component.as_os_str().to_string_lossy().into_owned(),
                Location::Local(step),
            ));
        }
    }
    crumbs
}
