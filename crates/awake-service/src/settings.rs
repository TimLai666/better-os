//! The service's own settings: today, only how long the lid may stay closed on
//! battery before the sessions holding it end (ticket 109).
//!
//! A service setting rather than a window preference, because the tray starts
//! sessions too and must be held to the same limit without reading the
//! window's file. It lives beside the service's other files in
//! `$XDG_STATE_HOME/better-awake/`, under its own schema stamp, with the same
//! rules `awake-store` follows: a missing file is the defaults, a newer schema
//! is kept and refused rather than overwritten, and a file that cannot be read
//! is moved aside rather than silently replaced.
//!
//! The quick-session offer (ticket 87) has a file of its own beside it,
//! `awake-quick-sessions.json`, rather than a field here: a rollback to a
//! service that knows only schema 1 of this file then still reads its
//! closed-lid limit. The offer is what the old settings window kept in
//! `awake-gui-preferences.json`; the service imports that file once, the first
//! time it starts without an offer of its own, and leaves it in place.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use awake_core::{LidBatteryLimit, SessionPolicy};
use awake_ipc::{QuickLength, QuickSessions, QuickSessionsDocument, SessionDefaults};
use serde::{Deserialize, Serialize};

pub const SETTINGS_FILE_NAME: &str = "awake-service-settings.json";

/// The only schema this service writes.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSettings {
    pub schema_version: u32,
    #[serde(default)]
    pub lid_battery_limit: LidBatteryLimit,
}

impl Default for ServiceSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            lid_battery_limit: LidBatteryLimit::default(),
        }
    }
}

/// What loading found, with anything worth saying about it as a stable key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsLoad {
    pub settings: ServiceSettings,
    pub detail: Option<String>,
    /// False when the file on disk was written by a newer Better Awake, so this
    /// one must not overwrite it.
    pub writable: bool,
}

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Next to the state file, wherever that is.
    pub fn beside(state_file: &Path) -> Self {
        let directory = state_file.parent().unwrap_or_else(|| Path::new("."));
        Self::at_path(directory.join(SETTINGS_FILE_NAME))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> SettingsLoad {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return SettingsLoad {
                    settings: ServiceSettings::default(),
                    detail: None,
                    writable: true,
                };
            }
            Err(error) => {
                return SettingsLoad {
                    settings: ServiceSettings::default(),
                    detail: Some(format!("awake.settings.unreadable:{error}")),
                    // Not known to be ours to replace.
                    writable: false,
                };
            }
        };

        #[derive(Deserialize)]
        struct Stamp {
            schema_version: u32,
        }
        match serde_json::from_slice::<Stamp>(&bytes) {
            Ok(stamp) if stamp.schema_version > SETTINGS_SCHEMA_VERSION => {
                return SettingsLoad {
                    settings: ServiceSettings::default(),
                    detail: Some(format!(
                        "awake.settings.newer_schema:{}",
                        stamp.schema_version
                    )),
                    writable: false,
                };
            }
            _ => {}
        }

        match serde_json::from_slice::<ServiceSettings>(&bytes) {
            Ok(settings) if settings.lid_battery_limit.validate().is_ok() => SettingsLoad {
                settings,
                detail: None,
                writable: true,
            },
            _ => {
                let backup = self.path.with_extension("json.corrupt");
                let moved = fs::rename(&self.path, &backup).is_ok();
                SettingsLoad {
                    settings: ServiceSettings::default(),
                    detail: Some("awake.settings.recovered_corrupt".to_string()),
                    writable: moved,
                }
            }
        }
    }

    /// Writes through a temporary file and renames, so a crash mid-write
    /// leaves the previous settings readable.
    pub fn save(&self, settings: &ServiceSettings) -> io::Result<()> {
        let mut settings = *settings;
        settings.schema_version = SETTINGS_SCHEMA_VERSION;
        let document = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, &document)?;
        fs::rename(&temporary, &self.path)
    }
}

pub const QUICK_SESSIONS_FILE_NAME: &str = "awake-quick-sessions.json";

/// The only schema of the offer file this service writes.
pub const QUICK_SESSIONS_SCHEMA_VERSION: u32 = 1;

/// The old settings window's preferences, imported once.
pub const GUI_PREFERENCES_FILE_NAME: &str = "awake-gui-preferences.json";

/// The newest schema of that file any released window wrote. Version 2
/// added the lid choice to the session defaults; a version 1 file is read
/// with it on, as that window did.
pub const GUI_PREFERENCES_NEWEST_SCHEMA: u32 = 2;

/// The notice an offer carries when the old window's file was written by a
/// newer Better Awake than this one, so it was not imported.
pub fn import_newer_schema_notice(schema: u32) -> String {
    format!("awake.quick_sessions.import_newer_schema:{schema}")
}

/// The notice when the old window's file could not be read.
pub const IMPORT_UNREADABLE_NOTICE: &str = "awake.quick_sessions.import_unreadable";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredQuickSessions {
    schema_version: u32,
    quick_sessions: QuickSessions,
    /// What the one-time import found, kept so the page can keep saying it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    import_notice: Option<String>,
}

/// What loading the offer found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickSessionsLoad {
    pub document: QuickSessionsDocument,
    /// A stable key about the offer file itself, for the status's detail.
    pub detail: Option<String>,
    /// Whether this load read the old window's file into the offer.
    pub imported: bool,
}

#[derive(Clone, Debug)]
pub struct QuickSessionsStore {
    path: PathBuf,
    gui_preferences: PathBuf,
}

impl QuickSessionsStore {
    /// Next to the state file, which is where the old window kept its
    /// preferences too (`$XDG_STATE_HOME/better-awake/`).
    pub fn beside(state_file: &Path) -> Self {
        let directory = state_file.parent().unwrap_or_else(|| Path::new("."));
        Self {
            path: directory.join(QUICK_SESSIONS_FILE_NAME),
            gui_preferences: directory.join(GUI_PREFERENCES_FILE_NAME),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn gui_preferences_path(&self) -> &Path {
        &self.gui_preferences
    }

    /// The offer, importing the old window's preferences the first time.
    ///
    /// With no offer file, the old window's file is read once and the result
    /// written as the offer, together with what the import found, so a
    /// second start reads the offer and never imports again. The old file is
    /// left as it is. A newer offer file is kept and refused; one that
    /// cannot be read is moved aside.
    pub fn load(&self) -> QuickSessionsLoad {
        let document = |quick_sessions, import_notice, writable| QuickSessionsDocument {
            quick_sessions,
            import_notice,
            writable,
        };
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return self.import(),
            Err(error) => {
                return QuickSessionsLoad {
                    document: document(QuickSessions::default(), None, false),
                    detail: Some(format!("awake.quick_sessions.unreadable:{error}")),
                    imported: false,
                };
            }
        };
        #[derive(Deserialize)]
        struct Stamp {
            schema_version: u32,
        }
        if let Ok(stamp) = serde_json::from_slice::<Stamp>(&bytes)
            && stamp.schema_version > QUICK_SESSIONS_SCHEMA_VERSION
        {
            return QuickSessionsLoad {
                document: document(QuickSessions::default(), None, false),
                detail: Some(format!(
                    "awake.quick_sessions.newer_schema:{}",
                    stamp.schema_version
                )),
                imported: false,
            };
        }
        match serde_json::from_slice::<StoredQuickSessions>(&bytes) {
            Ok(stored) if stored.quick_sessions.validate().is_ok() => QuickSessionsLoad {
                document: document(stored.quick_sessions, stored.import_notice, true),
                detail: None,
                imported: false,
            },
            _ => {
                let backup = self.path.with_extension("json.corrupt");
                let moved = fs::rename(&self.path, &backup).is_ok();
                QuickSessionsLoad {
                    document: document(QuickSessions::default(), None, moved),
                    detail: Some("awake.quick_sessions.recovered_corrupt".to_string()),
                    imported: false,
                }
            }
        }
    }

    fn import(&self) -> QuickSessionsLoad {
        let (quick_sessions, import_notice, imported) = match fs::read(&self.gui_preferences) {
            Err(_) => (QuickSessions::default(), None, false),
            Ok(bytes) => match read_gui_preferences(&bytes) {
                GuiImport::Imported(quick_sessions) => (quick_sessions, None, true),
                GuiImport::NewerSchema(schema) => (
                    QuickSessions::default(),
                    Some(import_newer_schema_notice(schema)),
                    false,
                ),
                GuiImport::Unreadable => (
                    QuickSessions::default(),
                    Some(IMPORT_UNREADABLE_NOTICE.to_string()),
                    false,
                ),
            },
        };
        let detail = self
            .save(&quick_sessions, import_notice.as_deref())
            .err()
            .map(|error| format!("awake.quick_sessions.write_failed:{error}"));
        QuickSessionsLoad {
            document: QuickSessionsDocument {
                quick_sessions,
                import_notice,
                writable: true,
            },
            detail,
            imported,
        }
    }

    /// Writes the offer through a temporary file and a rename.
    pub fn save(
        &self,
        quick_sessions: &QuickSessions,
        import_notice: Option<&str>,
    ) -> io::Result<()> {
        let stored = StoredQuickSessions {
            schema_version: QUICK_SESSIONS_SCHEMA_VERSION,
            quick_sessions: quick_sessions.clone(),
            import_notice: import_notice.map(str::to_string),
        };
        let document = serde_json::to_vec_pretty(&stored).map_err(io::Error::other)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, &document)?;
        fs::rename(&temporary, &self.path)
    }
}

/// What the old window's file holds, as far as the offer goes.
#[derive(Clone, Debug, Eq, PartialEq)]
enum GuiImport {
    Imported(QuickSessions),
    NewerSchema(u32),
    Unreadable,
}

/// Reads the presets, the default, and the session defaults out of the old
/// window's preferences. Its language and appearance are not read: the
/// control center's own settings apply (ADR 0017).
fn read_gui_preferences(bytes: &[u8]) -> GuiImport {
    #[derive(Deserialize)]
    struct Stamp {
        schema_version: u32,
    }
    #[derive(Deserialize)]
    struct GuiPreferences {
        schema_version: u32,
        presets: Vec<QuickLength>,
        default_preset: usize,
        defaults: SessionDefaults,
    }
    match serde_json::from_slice::<Stamp>(bytes) {
        Ok(stamp) if stamp.schema_version > GUI_PREFERENCES_NEWEST_SCHEMA => {
            return GuiImport::NewerSchema(stamp.schema_version);
        }
        Ok(stamp) if stamp.schema_version == 0 => return GuiImport::Unreadable,
        Ok(_) => {}
        Err(_) => return GuiImport::Unreadable,
    }
    let Ok(mut preferences) = serde_json::from_slice::<GuiPreferences>(bytes) else {
        return GuiImport::Unreadable;
    };
    if preferences.schema_version < GUI_PREFERENCES_NEWEST_SCHEMA {
        preferences.defaults.policy.prevent_lid_suspend =
            SessionPolicy::manual_default().prevent_lid_suspend;
    }
    let Ok(default_index) = u32::try_from(preferences.default_preset) else {
        return GuiImport::Unreadable;
    };
    let quick_sessions = QuickSessions {
        lengths: preferences.presets,
        default_index,
        defaults: preferences.defaults,
    };
    match quick_sessions.validate() {
        Ok(()) => GuiImport::Imported(quick_sessions),
        Err(_) => GuiImport::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, SettingsStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = SettingsStore::at_path(directory.path().join(SETTINGS_FILE_NAME));
        (directory, store)
    }

    #[test]
    fn a_missing_file_is_the_two_hour_default() {
        let (_directory, store) = store();
        let load = store.load();
        assert_eq!(load.settings.lid_battery_limit.minutes(), Some(120));
        assert!(load.writable);
        assert_eq!(load.detail, None);
    }

    #[test]
    fn a_saved_limit_is_read_back() {
        let (_directory, store) = store();
        for limit in [
            LidBatteryLimit::Never,
            LidBatteryLimit::Minutes { minutes: 30 },
        ] {
            store
                .save(&ServiceSettings {
                    lid_battery_limit: limit,
                    ..ServiceSettings::default()
                })
                .unwrap();
            assert_eq!(store.load().settings.lid_battery_limit, limit);
        }
    }

    #[test]
    fn a_newer_file_is_kept_and_not_overwritten() {
        let (_directory, store) = store();
        let newer = r#"{"schema_version":9,"lid_battery_limit":{"kind":"never"},"x":1}"#;
        fs::write(store.path(), newer).unwrap();
        let load = store.load();
        assert!(!load.writable);
        assert_eq!(load.settings, ServiceSettings::default());
        assert_eq!(fs::read_to_string(store.path()).unwrap(), newer);
    }

    #[test]
    fn an_unreadable_or_out_of_range_file_is_moved_aside() {
        for document in [
            "not json",
            r#"{"schema_version":1,"lid_battery_limit":{"kind":"minutes","minutes":1}}"#,
        ] {
            let (_directory, store) = store();
            fs::write(store.path(), document).unwrap();
            let load = store.load();
            assert_eq!(load.settings, ServiceSettings::default());
            assert_eq!(
                load.detail.as_deref(),
                Some("awake.settings.recovered_corrupt")
            );
            assert!(load.writable);
            assert_eq!(
                fs::read_to_string(store.path().with_extension("json.corrupt")).unwrap(),
                document
            );
        }
    }

    #[test]
    fn the_file_sits_beside_the_state_file() {
        let store = SettingsStore::beside(Path::new("/s/better-awake/awake-service-state.json"));
        assert_eq!(
            store.path(),
            Path::new("/s/better-awake/awake-service-settings.json")
        );
    }

    // ---- The quick-session offer and the one-time import ----------------

    fn offer_store() -> (tempfile::TempDir, QuickSessionsStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = QuickSessionsStore::beside(&directory.path().join("awake-service-state.json"));
        (directory, store)
    }

    /// A preferences file as today's `awake-gui` writes it (schema 2).
    const GUI_FILE: &str = r#"{
  "schema_version": 2,
  "presets": [
    {"kind": "minutes", "minutes": 25},
    {"kind": "indefinite"},
    {"kind": "minutes", "minutes": 90}
  ],
  "default_preset": 2,
  "defaults": {
    "policy": {
      "prevent_system_suspend": true,
      "prevent_idle": true,
      "prevent_display_sleep": true,
      "prevent_automatic_lock": false,
      "prevent_lid_suspend": false
    },
    "battery_stop_percent": 35
  },
  "locale": "en-US",
  "theme": "dark"
}"#;

    #[test]
    fn the_old_window_s_preferences_are_imported_once_and_left_in_place() {
        let (_directory, store) = offer_store();
        fs::write(store.gui_preferences_path(), GUI_FILE).unwrap();

        let first = store.load();
        assert!(first.imported);
        assert_eq!(first.detail, None);
        let offer = &first.document.quick_sessions;
        assert_eq!(
            offer.lengths,
            vec![
                QuickLength::Minutes { minutes: 25 },
                QuickLength::Indefinite,
                QuickLength::Minutes { minutes: 90 },
            ]
        );
        assert_eq!(
            offer.default_length(),
            Some(QuickLength::Minutes { minutes: 90 })
        );
        assert!(offer.defaults.policy.prevent_display_sleep);
        assert!(!offer.defaults.policy.prevent_lid_suspend);
        assert_eq!(offer.defaults.battery_stop_percent, Some(35));
        assert_eq!(first.document.import_notice, None);
        // Left in place, byte for byte.
        assert_eq!(
            fs::read_to_string(store.gui_preferences_path()).unwrap(),
            GUI_FILE
        );
        assert!(store.path().is_file());

        // A second start reads the offer and does not import again, even
        // after the old file changed.
        fs::write(
            store.gui_preferences_path(),
            GUI_FILE.replace("\"minutes\": 25", "\"minutes\": 5"),
        )
        .unwrap();
        let second = store.load();
        assert!(!second.imported);
        assert_eq!(second.document.quick_sessions, *offer);
    }

    #[test]
    fn a_change_after_the_import_is_what_the_next_start_reads() {
        let (_directory, store) = offer_store();
        fs::write(store.gui_preferences_path(), GUI_FILE).unwrap();
        let mut offer = store.load().document.quick_sessions;
        assert!(offer.add_length(QuickLength::Minutes { minutes: 45 }));
        store.save(&offer, None).unwrap();
        let again = store.load();
        assert!(!again.imported);
        assert_eq!(again.document.quick_sessions, offer);
    }

    #[test]
    fn a_schema_1_window_file_is_imported_with_the_lid_choice_on() {
        let (_directory, store) = offer_store();
        let schema_1 = r#"{"schema_version":1,"presets":[{"kind":"minutes","minutes":15}],"default_preset":0,"defaults":{"policy":{"prevent_system_suspend":true,"prevent_idle":true,"prevent_display_sleep":false,"prevent_automatic_lock":false},"battery_stop_percent":null},"locale":"zh-TW"}"#;
        fs::write(store.gui_preferences_path(), schema_1).unwrap();
        let load = store.load();
        assert!(load.imported);
        assert!(
            load.document
                .quick_sessions
                .defaults
                .policy
                .prevent_lid_suspend
        );
        assert_eq!(
            load.document.quick_sessions.defaults.battery_stop_percent,
            None
        );
    }

    #[test]
    fn a_newer_window_file_is_left_alone_and_said_so() {
        let (_directory, store) = offer_store();
        let newer =
            r#"{"schema_version":3,"presets":[{"kind":"weeks","weeks":1}],"default_preset":0}"#;
        fs::write(store.gui_preferences_path(), newer).unwrap();
        let load = store.load();
        assert!(!load.imported);
        assert_eq!(load.document.quick_sessions, QuickSessions::default());
        assert_eq!(
            load.document.import_notice.as_deref(),
            Some("awake.quick_sessions.import_newer_schema:3")
        );
        assert_eq!(
            fs::read_to_string(store.gui_preferences_path()).unwrap(),
            newer
        );
        // Still said after a restart, and still not imported.
        let again = store.load();
        assert!(!again.imported);
        assert_eq!(again.document.import_notice, load.document.import_notice);
    }

    #[test]
    fn an_unreadable_window_file_imports_nothing_and_is_said() {
        for document in [
            "not json",
            r#"{"schema_version":2,"presets":[],"default_preset":0,"defaults":{"policy":{"prevent_system_suspend":true,"prevent_idle":true,"prevent_display_sleep":false,"prevent_automatic_lock":false}}}"#,
        ] {
            let (_directory, store) = offer_store();
            fs::write(store.gui_preferences_path(), document).unwrap();
            let load = store.load();
            assert!(!load.imported);
            assert_eq!(load.document.quick_sessions, QuickSessions::default());
            assert_eq!(
                load.document.import_notice.as_deref(),
                Some(IMPORT_UNREADABLE_NOTICE)
            );
        }
    }

    #[test]
    fn no_window_file_is_the_shipped_offer_with_nothing_to_say() {
        let (_directory, store) = offer_store();
        let load = store.load();
        assert!(!load.imported);
        assert_eq!(load.document.quick_sessions, QuickSessions::default());
        assert_eq!(load.document.import_notice, None);
        assert!(load.document.writable);
    }

    #[test]
    fn a_newer_offer_file_is_kept_and_a_broken_one_moved_aside() {
        let (_directory, store) = offer_store();
        let newer = r#"{"schema_version":7}"#;
        fs::write(store.path(), newer).unwrap();
        let load = store.load();
        assert!(!load.document.writable);
        assert_eq!(fs::read_to_string(store.path()).unwrap(), newer);

        let (_directory, store) = offer_store();
        fs::write(store.path(), "{").unwrap();
        let load = store.load();
        assert!(load.document.writable);
        assert_eq!(
            load.detail.as_deref(),
            Some("awake.quick_sessions.recovered_corrupt")
        );
    }
}
