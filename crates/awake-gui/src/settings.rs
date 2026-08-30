//! The preferences this window owns: the quick-session presets, the defaults a
//! new session is given, the language, and the appearance.
//!
//! These are not session state. The service owns every session and every rule;
//! this file holds only what a person chose about how sessions are offered, and
//! deleting it costs nothing but those choices. It lives beside the service's
//! own files in `$XDG_STATE_HOME/better-awake/` so an uninstall that offers to
//! remove user data has one directory to look in.
//!
//! A write that fails is reported, not swallowed: the window keeps the choice
//! for this run and says on screen that it will not survive being closed.

use std::fs;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use awake_core::SessionPolicy;
use serde::{Deserialize, Serialize};

use crate::i18n::{Copy, Locale, fill};

pub(crate) const PREFERENCES_FILE_NAME: &str = "awake-gui-preferences.json";

/// The only schema this window writes. A newer file is refused rather than
/// overwritten, following `awake-store`'s discipline.
pub(crate) const PREFERENCES_SCHEMA_VERSION: u32 = 1;

/// The largest number of presets the tray menu can show without becoming a
/// scrolling list, which is not what a quick menu is for.
pub(crate) const MAX_PRESETS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub(crate) enum PresetLength {
    /// Runs until someone ends it.
    Indefinite,
    Minutes {
        minutes: u64,
    },
}

impl PresetLength {
    pub(crate) fn label(self, c: &'static Copy) -> String {
        match self {
            PresetLength::Indefinite => c.preset_indefinite.to_string(),
            PresetLength::Minutes { minutes } if minutes % 60 == 0 && minutes >= 60 => {
                fill(c.preset_hours, "hours", &(minutes / 60).to_string())
            }
            PresetLength::Minutes { minutes } => {
                fill(c.preset_minutes, "minutes", &minutes.to_string())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StoredTheme {
    Dark,
    Light,
    System,
}

/// What a new session is given when nothing else says otherwise.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionDefaults {
    pub(crate) policy: SessionPolicy,
    /// `None` means a new session never stops itself for battery level.
    #[serde(default)]
    pub(crate) battery_stop_percent: Option<u8>,
}

impl Default for SessionDefaults {
    fn default() -> Self {
        Self {
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(awake_core::DEFAULT_BATTERY_STOP_PERCENT),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preferences {
    pub(crate) schema_version: u32,
    /// In menu order. The first is not automatically the default; `default_preset`
    /// names it, so reordering the menu does not silently change what a click does.
    pub(crate) presets: Vec<PresetLength>,
    pub(crate) default_preset: usize,
    pub(crate) defaults: SessionDefaults,
    /// A locale key: `system`, `en-US`, or `zh-TW`.
    pub(crate) locale: String,
    pub(crate) theme: StoredTheme,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            presets: Self::shipped_presets(),
            default_preset: 1,
            defaults: SessionDefaults::default(),
            locale: Locale::System.as_key().to_string(),
            // Better OS is dark-first. An unconfigured window opens dark rather
            // than taking the toolkit's own light default.
            theme: StoredTheme::Dark,
        }
    }
}

impl Preferences {
    /// The lengths the tray menu offers out of the box.
    pub(crate) fn shipped_presets() -> Vec<PresetLength> {
        vec![
            PresetLength::Minutes { minutes: 15 },
            PresetLength::Minutes { minutes: 60 },
            PresetLength::Minutes { minutes: 180 },
            PresetLength::Indefinite,
        ]
    }

    pub(crate) fn locale(&self) -> Locale {
        Locale::from_key(&self.locale)
    }

    /// Moves a preset one place up or down, keeping the default pointing at the
    /// same preset rather than at the same position.
    pub(crate) fn move_preset(&mut self, index: usize, delta: isize) -> bool {
        let target = index as isize + delta;
        if index >= self.presets.len() || target < 0 || target as usize >= self.presets.len() {
            return false;
        }
        let target = target as usize;
        let default = self.default_preset;
        self.presets.swap(index, target);
        self.default_preset = if default == index {
            target
        } else if default == target {
            index
        } else {
            default
        };
        true
    }

    pub(crate) fn remove_preset(&mut self, index: usize) -> bool {
        // The menu must keep at least one length, or the tray offers a submenu
        // that starts nothing.
        if self.presets.len() <= 1 || index >= self.presets.len() {
            return false;
        }
        self.presets.remove(index);
        if self.default_preset >= self.presets.len() {
            self.default_preset = self.presets.len() - 1;
        }
        true
    }

    pub(crate) fn add_preset(&mut self, length: PresetLength) -> bool {
        if self.presets.len() >= MAX_PRESETS || self.presets.contains(&length) {
            return false;
        }
        self.presets.push(length);
        true
    }

    pub(crate) fn restore_default_presets(&mut self) {
        self.presets = Self::shipped_presets();
        self.default_preset = 1;
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreferencesStore {
    path: PathBuf,
}

impl PreferencesStore {
    /// `$XDG_STATE_HOME/better-awake/awake-gui-preferences.json`, falling back
    /// to `~/.local/state`.
    pub(crate) fn from_default_path() -> Self {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".local").join("state"))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        Self::at_path(base.join("better-awake").join(PREFERENCES_FILE_NAME))
    }

    pub(crate) fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Test-only: the store reads and writes through its own path, and this
    /// exists so a test can assert the file lands where it is documented to.
    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the stored preferences, or the shipped defaults.
    ///
    /// A missing file is a first run. A file this build cannot understand — a
    /// newer schema, or a document that does not parse — yields the defaults
    /// and `false`, so the window can say that saving would overwrite something
    /// it could not read rather than quietly replacing it.
    pub(crate) fn load(&self) -> (Preferences, bool) {
        let Ok(bytes) = fs::read(&self.path) else {
            return (Preferences::default(), true);
        };
        match serde_json::from_slice::<Preferences>(&bytes) {
            Ok(preferences)
                if preferences.schema_version == PREFERENCES_SCHEMA_VERSION
                    && !preferences.presets.is_empty()
                    && preferences.default_preset < preferences.presets.len() =>
            {
                (preferences, true)
            }
            _ => (Preferences::default(), false),
        }
    }

    pub(crate) fn save(&self, preferences: &Preferences) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let document = serde_json::to_vec_pretty(preferences).map_err(|error| error.to_string())?;
        // Write and rename, so an interrupted save leaves the previous file
        // rather than a truncated one.
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, document).map_err(|error| error.to_string())?;
        fs::rename(&temporary, &self.path).map_err(|error| error.to_string())
    }
}
