//! The view preferences one user carries between sessions.
//!
//! Issue #6 leaves the per-folder versus global policy open and ticket 34 has
//! to choose one. **This build stores view preferences globally, per user.**
//! One file holds the view mode, the sort order, folders-first, the item
//! scale, the hidden-entry preference, and the language, and every new tab
//! inherits them.
//!
//! The reason is that `files_core::ViewPreferences` already lives on the tab,
//! so a per-folder rule can be layered on later by writing a different value
//! into one tab without touching this file or the model. Shipping per-folder
//! first would have been the irreversible choice: a directory-keyed store has
//! to decide eviction, what an unwritable directory does, and what a moved
//! directory inherits, and none of those is answerable before there is a
//! window to observe. `docs/files-gui-policy.md` records it.
//!
//! Nothing here fails loudly. A missing file is the defaults, and a corrupt
//! file is the defaults plus a note the window can show — losing a sort order
//! is not worth refusing to open a file manager over.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use files_core::{HiddenPreference, SortDirection, SortKey, SortOrder, ViewPreferences};
use serde::{Deserialize, Serialize};

/// How the content area draws its entries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    /// Tiles in a wrapping grid.
    #[default]
    Grid,
    /// One row per entry, with columns.
    List,
}

impl ViewMode {
    pub fn key(self) -> &'static str {
        match self {
            ViewMode::Grid => "files.view.grid",
            ViewMode::List => "files.view.list",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            ViewMode::Grid => ViewMode::List,
            ViewMode::List => ViewMode::Grid,
        }
    }
}

/// Icon size in the grid and row height in the list, as one control.
///
/// A closed set rather than a free pixel value: the row height decides the
/// virtualized viewport arithmetic, and an arbitrary float would make the
/// scroll position depend on a number the user can nudge by one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemScale {
    Small,
    #[default]
    Medium,
    Large,
}

impl ItemScale {
    pub const ALL: [ItemScale; 3] = [ItemScale::Small, ItemScale::Medium, ItemScale::Large];

    pub fn key(self) -> &'static str {
        match self {
            ItemScale::Small => "files.scale.small",
            ItemScale::Medium => "files.scale.medium",
            ItemScale::Large => "files.scale.large",
        }
    }

    /// The tile edge in the grid, in logical pixels.
    pub fn tile_size(self) -> f32 {
        match self {
            ItemScale::Small => 96.0,
            ItemScale::Medium => 128.0,
            ItemScale::Large => 168.0,
        }
    }

    /// The row height in the list, in logical pixels.
    pub fn row_height(self) -> f32 {
        match self {
            ItemScale::Small => 28.0,
            ItemScale::Medium => 34.0,
            ItemScale::Large => 44.0,
        }
    }

    pub fn larger(self) -> Self {
        match self {
            ItemScale::Small => ItemScale::Medium,
            _ => ItemScale::Large,
        }
    }

    pub fn smaller(self) -> Self {
        match self {
            ItemScale::Large => ItemScale::Medium,
            _ => ItemScale::Small,
        }
    }
}

/// The language the window is drawn in.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalePreference {
    /// Follow the session's `LANG`.
    #[default]
    System,
    EnUs,
    ZhTw,
}

/// A sort key, in a form that can be written to a file.
///
/// `files_core::SortKey` deliberately carries no `serde` derive — it is a
/// domain type, not a wire type — so the mapping lives here and a key this
/// build does not recognize falls back to Name rather than failing the load.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredSortKey {
    Name,
    Modified,
    Size,
    Type,
    Extension,
}

impl StoredSortKey {
    fn from_key(key: SortKey) -> Self {
        match key {
            SortKey::Name => StoredSortKey::Name,
            SortKey::Modified => StoredSortKey::Modified,
            SortKey::Size => StoredSortKey::Size,
            SortKey::Type => StoredSortKey::Type,
            SortKey::Extension => StoredSortKey::Extension,
        }
    }

    fn to_key(self) -> SortKey {
        match self {
            StoredSortKey::Name => SortKey::Name,
            StoredSortKey::Modified => SortKey::Modified,
            StoredSortKey::Size => SortKey::Size,
            StoredSortKey::Type => SortKey::Type,
            StoredSortKey::Extension => SortKey::Extension,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredSortDirection {
    Ascending,
    Descending,
}

/// Everything one user's window remembers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesPreferences {
    pub view_mode: ViewMode,
    pub scale: ItemScale,
    pub locale: LocalePreference,
    /// Whether dotfiles and the rest of the platform's hidden set are drawn.
    pub show_hidden: bool,
    pub folders_first: bool,
    sort_key: StoredSortKey,
    sort_direction: StoredSortDirection,
}

impl Default for FilesPreferences {
    fn default() -> Self {
        let order = SortOrder::default();
        Self {
            view_mode: ViewMode::default(),
            scale: ItemScale::default(),
            locale: LocalePreference::default(),
            // Issue #6: hidden by default.
            show_hidden: false,
            folders_first: order.folders_first,
            sort_key: StoredSortKey::from_key(order.key),
            sort_direction: StoredSortDirection::Ascending,
        }
    }
}

impl FilesPreferences {
    pub fn order(&self) -> SortOrder {
        SortOrder::new(
            self.sort_key.to_key(),
            match self.sort_direction {
                StoredSortDirection::Ascending => SortDirection::Ascending,
                StoredSortDirection::Descending => SortDirection::Descending,
            },
        )
        .with_folders_first(self.folders_first)
    }

    pub fn set_order(&mut self, order: SortOrder) {
        self.sort_key = StoredSortKey::from_key(order.key);
        self.sort_direction = match order.direction {
            SortDirection::Ascending => StoredSortDirection::Ascending,
            SortDirection::Descending => StoredSortDirection::Descending,
        };
        self.folders_first = order.folders_first;
    }

    pub fn hidden(&self) -> HiddenPreference {
        if self.show_hidden {
            HiddenPreference::showing_hidden()
        } else {
            HiddenPreference::default()
        }
    }

    /// The value a tab is opened with. This is the whole of the global policy:
    /// there is one source, and every tab starts from it.
    pub fn view_preferences(&self) -> ViewPreferences {
        ViewPreferences {
            order: self.order(),
            hidden: self.hidden(),
        }
    }
}

/// Where the preferences file is and how it is read and written.
#[derive(Clone, Debug)]
pub struct PreferenceStore {
    path: PathBuf,
}

/// What a load produced, including whether anything was wrong with the file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedPreferences {
    pub preferences: FilesPreferences,
    /// Set when a file existed and could not be used. The window shows it once
    /// rather than silently reverting the user's settings.
    pub problem: Option<String>,
}

impl PreferenceStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The per-user location: `$XDG_CONFIG_HOME/better-os/files/view.json`,
    /// falling back to `~/.config` the way the specification defines.
    pub fn from_env() -> Self {
        Self::at_path(config_home().join("better-os/files/view.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> LoadedPreferences {
        match fs::read_to_string(&self.path) {
            Ok(text) => match serde_json::from_str::<FilesPreferences>(&text) {
                Ok(preferences) => LoadedPreferences {
                    preferences,
                    problem: None,
                },
                Err(error) => LoadedPreferences {
                    preferences: FilesPreferences::default(),
                    problem: Some(format!("files.prefs.error.unreadable:{error}")),
                },
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => LoadedPreferences {
                preferences: FilesPreferences::default(),
                problem: None,
            },
            Err(error) => LoadedPreferences {
                preferences: FilesPreferences::default(),
                problem: Some(format!("files.prefs.error.unreadable:{error}")),
            },
        }
    }

    /// Writes through a temporary file and a rename, so an interrupted save
    /// leaves the previous settings rather than half a file.
    pub fn save(&self, preferences: &FilesPreferences) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(preferences)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, text.as_bytes())?;
        fs::rename(&temporary, &self.path)
    }
}

/// `$XDG_CONFIG_HOME`, or `~/.config`, or the current directory as the last
/// resort a test environment can still write to.
pub(crate) fn config_home() -> PathBuf {
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return path;
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home);
        if path.is_absolute() {
            return path.join(".config");
        }
    }
    PathBuf::from(".config")
}
