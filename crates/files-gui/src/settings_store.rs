//! How each folder is shown, remembered between launches.
//!
//! One file of Better Files' own, `$XDG_STATE_HOME/better-os/files/views.json`,
//! holds one default view for every folder, an override for each folder the
//! person changed, and the last folder that was open. Nothing is ever written
//! into the folders themselves: no `.DS_Store`, no `.directory`, so a shared
//! or read-only folder behaves like any other and a project directory stays
//! clean.
//!
//! A view is the four things the owner asked to be remembered: how entries
//! are drawn (grid or list), the zoom, the sort order, and whether hidden
//! files are shown. Changing one of them changes it for the folder in front
//! of the person; "Use this view for every folder" makes it the default and
//! forgets the overrides.
//!
//! The state directory rather than the config directory, because this is
//! remembered state, not a setting a person edits: losing it costs a sort
//! order, never a decision. A missing or unreadable file is the defaults.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use files_core::{HiddenPreference, SortDirection, SortKey, SortOrder, ViewPreferences};
use serde::{Deserialize, Serialize};

use crate::prefs::{ItemScale, ViewMode};

/// How many folder overrides are kept. The oldest-touched go first, so a
/// person who browses thousands of folders over a year does not grow the file
/// without bound.
pub const MAX_OVERRIDES: usize = 2_000;

/// One folder's view.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderView {
    pub mode: ViewMode,
    pub scale: ItemScale,
    pub show_hidden: bool,
    pub folders_first: bool,
    sort_key: StoredKey,
    descending: bool,
}

impl Default for FolderView {
    fn default() -> Self {
        let order = SortOrder::default();
        Self {
            mode: ViewMode::default(),
            scale: ItemScale::default(),
            show_hidden: false,
            folders_first: order.folders_first,
            sort_key: StoredKey::from(order.key),
            descending: order.direction == SortDirection::Descending,
        }
    }
}

impl FolderView {
    pub fn new(mode: ViewMode, scale: ItemScale, show_hidden: bool, order: SortOrder) -> Self {
        let mut view = Self {
            mode,
            scale,
            show_hidden,
            ..Self::default()
        };
        view.set_order(order);
        view
    }

    pub fn order(&self) -> SortOrder {
        SortOrder::new(
            self.sort_key.into(),
            if self.descending {
                SortDirection::Descending
            } else {
                SortDirection::Ascending
            },
        )
        .with_folders_first(self.folders_first)
    }

    pub fn set_order(&mut self, order: SortOrder) {
        self.sort_key = order.key.into();
        self.descending = order.direction == SortDirection::Descending;
        self.folders_first = order.folders_first;
    }

    pub fn hidden(&self) -> HiddenPreference {
        if self.show_hidden {
            HiddenPreference::showing_hidden()
        } else {
            HiddenPreference::default()
        }
    }

    pub fn view_preferences(&self) -> ViewPreferences {
        ViewPreferences {
            order: self.order(),
            hidden: self.hidden(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredKey {
    Name,
    Modified,
    Size,
    Type,
    Extension,
}

impl From<SortKey> for StoredKey {
    fn from(key: SortKey) -> Self {
        match key {
            SortKey::Name => StoredKey::Name,
            SortKey::Modified => StoredKey::Modified,
            SortKey::Size => StoredKey::Size,
            SortKey::Type => StoredKey::Type,
            SortKey::Extension => StoredKey::Extension,
        }
    }
}

impl From<StoredKey> for SortKey {
    fn from(key: StoredKey) -> Self {
        match key {
            StoredKey::Name => SortKey::Name,
            StoredKey::Modified => SortKey::Modified,
            StoredKey::Size => SortKey::Size,
            StoredKey::Type => SortKey::Type,
            StoredKey::Extension => SortKey::Extension,
        }
    }
}

/// One override and when it was last touched, for the size bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Override {
    view: FolderView,
    touched: u64,
}

/// Everything the file holds.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewState {
    default: FolderView,
    /// Keyed by the folder's absolute path, as text.
    folders: BTreeMap<String, Override>,
    last_folder: Option<PathBuf>,
    /// A counter rather than a clock, so the order is exact and tests need no
    /// time.
    tick: u64,
}

impl ViewState {
    pub fn default_view(&self) -> FolderView {
        self.default
    }

    /// The view for `folder`: its override, or the default.
    pub fn view_for(&self, folder: Option<&Path>) -> FolderView {
        folder
            .and_then(|path| self.folders.get(&key(path)))
            .map(|entry| entry.view)
            .unwrap_or(self.default)
    }

    pub fn has_override(&self, folder: &Path) -> bool {
        self.folders.contains_key(&key(folder))
    }

    /// Records what the person changed in `folder`. A change that makes the
    /// folder look exactly like the default drops its override instead, so
    /// the file only names folders that really differ.
    ///
    /// With no folder (Trash, Applications, a search), the change is the
    /// default: those places have no path to key an override by.
    pub fn set_view(&mut self, folder: Option<&Path>, view: FolderView) {
        let Some(folder) = folder else {
            self.default = view;
            return;
        };
        let name = key(folder);
        if view == self.default {
            self.folders.remove(&name);
            return;
        }
        self.tick += 1;
        self.folders.insert(
            name,
            Override {
                view,
                touched: self.tick,
            },
        );
        if self.folders.len() > MAX_OVERRIDES {
            let oldest = self
                .folders
                .iter()
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(name, _)| name.clone());
            if let Some(oldest) = oldest {
                self.folders.remove(&oldest);
            }
        }
    }

    /// "Use this view for every folder": the view becomes the default and
    /// every override is forgotten.
    pub fn apply_everywhere(&mut self, view: FolderView) {
        self.default = view;
        self.folders.clear();
    }

    pub fn last_folder(&self) -> Option<&Path> {
        self.last_folder.as_deref()
    }

    pub fn set_last_folder(&mut self, folder: Option<&Path>) {
        if let Some(folder) = folder {
            self.last_folder = Some(folder.to_path_buf());
        }
    }

    /// Drops every override for a folder that no longer exists, and the last
    /// folder when it is gone. Returns how many overrides went.
    pub fn prune(&mut self, exists: impl Fn(&Path) -> bool) -> usize {
        let before = self.folders.len();
        self.folders.retain(|name, _| exists(Path::new(name)));
        if self
            .last_folder
            .as_deref()
            .is_some_and(|path| !exists(path))
        {
            self.last_folder = None;
        }
        before - self.folders.len()
    }

    pub fn override_count(&self) -> usize {
        self.folders.len()
    }
}

fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Where the file is, and how it is read and written.
#[derive(Clone, Debug)]
pub struct ViewStateStore {
    path: PathBuf,
}

impl ViewStateStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_STATE_HOME/better-os/files/views.json`, falling back to
    /// `~/.local/state` as the specification defines.
    pub fn from_env() -> Self {
        Self::at_path(state_home().join("better-os/files/views.json"))
    }

    /// A store in the same directory as another file, for a session built
    /// around a temporary preferences file.
    pub fn beside(other: &Path) -> Self {
        Self::at_path(
            other
                .parent()
                .map(|dir| dir.join("views.json"))
                .unwrap_or_else(|| PathBuf::from("views.json")),
        )
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the file and drops what points at folders that are gone. A
    /// missing or unreadable file is the defaults; nothing here refuses to
    /// open a window.
    pub fn load(&self) -> ViewState {
        let mut state = fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str::<ViewState>(&text).ok())
            .unwrap_or_default();
        state.prune(|path| path.is_dir());
        state
    }

    /// Writes through a temporary file and a rename, so an interrupted save
    /// leaves the previous state rather than half a file.
    pub fn save(&self, state: &ViewState) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(state)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, text.as_bytes())?;
        fs::rename(&temporary, &self.path)
    }
}

/// `$XDG_STATE_HOME`, or `~/.local/state`.
pub fn state_home() -> PathBuf {
    if let Some(value) = std::env::var_os("XDG_STATE_HOME") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return path;
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home);
        if path.is_absolute() {
            return path.join(".local/state");
        }
    }
    PathBuf::from(".local/state")
}
