//! What the Window attention page reads from the session and the two files,
//! and the one change it asks GNOME Shell for. Plain blocking calls, made off
//! the UI thread; the rules for what the answers mean are in
//! `window_attention_page`.

use std::collections::HashMap;
use std::path::PathBuf;

use app_catalog_core::IconReference;
use app_catalog_platform::{HostProbe, SessionEnvironment, load_catalog};
use better_ui::AppIcon;
use better_ui::icons::IconSearch;
use window_attention_core::bus::{
    EXTENSION_UUID, SERVICE_BUS_NAME, SERVICE_INTERFACE, SERVICE_OBJECT_PATH,
};
use window_attention_core::store::Store;
use window_attention_core::{AppId, Choices, SeenApps};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

use crate::window_attention_page::{
    ExtensionInfo, ExtensionState, InstalledApp, ServiceState, extension_state,
};

const SHELL_NAME: &str = "org.gnome.Shell";
const SHELL_PATH: &str = "/org/gnome/Shell";
const EXTENSIONS_INTERFACE: &str = "org.gnome.Shell.Extensions";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// Everything the page shows, read at once.
#[derive(Clone, Debug)]
pub(crate) struct Loaded {
    /// Each listed or installable app's own icon, resolved off the UI thread.
    pub(crate) icons: HashMap<AppId, AppIcon>,
    pub(crate) choices: Result<Choices, String>,
    pub(crate) seen: SeenApps,
    pub(crate) installed: Vec<InstalledApp>,
    pub(crate) extension: ExtensionState,
    pub(crate) service: ServiceState,
}

pub(crate) fn load(store: &Store) -> Loaded {
    let choices = store.load_choices().map_err(|error| error.to_string());
    // A seen list that cannot be read lists nothing; the service, which
    // writes it, reports the problem.
    let seen = store.load_seen().unwrap_or_default();
    let installed = installed_apps();
    let search = IconSearch::from_environment();
    let icons = installed
        .iter()
        .map(|app| {
            (
                app.id.clone(),
                AppIcon::for_entry(app.icon.as_deref(), &app.name, &search),
            )
        })
        .collect();
    let (extension, service) = match Connection::session() {
        Ok(connection) => (extension(&connection), service(&connection)),
        Err(error) => (
            ExtensionState::NoShell,
            ServiceState::Unreachable(error.to_string()),
        ),
    };
    Loaded {
        icons,
        choices,
        seen,
        installed,
        extension,
        service,
    }
}

/// The applications in the menu, by desktop id and name in this session's
/// language.
fn installed_apps() -> Vec<InstalledApp> {
    let session = SessionEnvironment::from_env();
    let catalog = load_catalog(&session, &HostProbe::from_env());
    catalog
        .sorted_by_name(session.locale.as_ref(), &session.environments)
        .into_iter()
        .filter_map(|record| {
            Some(InstalledApp {
                id: AppId::new(record.desktop_id.as_str()).ok()?,
                name: record.display_name(session.locale.as_ref()).to_string(),
                icon: match &record.icon {
                    Some(IconReference::Name(name)) => Some(name.clone()),
                    Some(IconReference::Path(path)) => Some(path.display().to_string()),
                    None => None,
                },
            })
        })
        .collect()
}

fn extension(connection: &Connection) -> ExtensionState {
    let info = connection.call_method(
        Some(SHELL_NAME),
        SHELL_PATH,
        Some(EXTENSIONS_INTERFACE),
        "GetExtensionInfo",
        &(EXTENSION_UUID,),
    );
    let info: HashMap<String, OwnedValue> = match info.and_then(|reply| reply.body().deserialize())
    {
        Ok(info) => info,
        Err(_) => return ExtensionState::NoShell,
    };
    let parsed = (!info.is_empty()).then(|| ExtensionInfo {
        state: info
            .get("state")
            .and_then(|value| f64::try_from(value).ok())
            .map(|state| state as u32)
            .unwrap_or(0),
        enabled: info
            .get("enabled")
            .and_then(|value| bool::try_from(value).ok())
            .unwrap_or(false),
        error: info
            .get("error")
            .and_then(|value| <&str>::try_from(value).ok())
            .unwrap_or_default()
            .to_string(),
    });
    extension_state(
        parsed.as_ref(),
        user_extensions_enabled(connection).unwrap_or(true),
        extension_installed(),
    )
}

fn user_extensions_enabled(connection: &Connection) -> Option<bool> {
    let reply = connection
        .call_method(
            Some(SHELL_NAME),
            SHELL_PATH,
            Some(PROPERTIES_INTERFACE),
            "Get",
            &(EXTENSIONS_INTERFACE, "UserExtensionsEnabled"),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    bool::try_from(&value).ok()
}

/// Whether the extension's files are where GNOME looks for them.
fn extension_installed() -> bool {
    let home = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
        });
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    std::iter::once(home)
        .chain(dirs.split(':').map(PathBuf::from))
        .any(|dir| {
            dir.join("gnome-shell/extensions")
                .join(EXTENSION_UUID)
                .join("metadata.json")
                .is_file()
        })
}

/// Asks the service for its status, which starts it through D-Bus activation
/// when it is installed and not running.
fn service(connection: &Connection) -> ServiceState {
    let reply = connection.call_method(
        Some(SERVICE_BUS_NAME),
        SERVICE_OBJECT_PATH,
        Some(SERVICE_INTERFACE),
        "Status",
        &(),
    );
    match reply.and_then(|reply| reply.body().deserialize::<(bool, bool, u32, String)>()) {
        Ok((adapter_present, suppressed, _, problem)) => ServiceState::Running {
            adapter_present,
            suppressed,
            problem,
        },
        Err(error) => ServiceState::Unreachable(error.to_string()),
    }
}

/// Turns the extension on: user extensions first when the session has them
/// all off, then the extension itself, through GNOME Shell's own extension
/// service, which writes the person's `enabled-extensions` as GNOME's
/// Extensions app would.
pub(crate) fn turn_on() -> Result<(), String> {
    let connection = Connection::session().map_err(|error| error.to_string())?;
    if user_extensions_enabled(&connection) == Some(false) {
        connection
            .call_method(
                Some(SHELL_NAME),
                SHELL_PATH,
                Some(PROPERTIES_INTERFACE),
                "Set",
                &(
                    EXTENSIONS_INTERFACE,
                    "UserExtensionsEnabled",
                    Value::from(true),
                ),
            )
            .map_err(|error| error.to_string())?;
    }
    let reply = connection
        .call_method(
            Some(SHELL_NAME),
            SHELL_PATH,
            Some(EXTENSIONS_INTERFACE),
            "EnableExtension",
            &(EXTENSION_UUID,),
        )
        .map_err(|error| error.to_string())?;
    match reply.body().deserialize::<bool>() {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("GNOME Shell refused to enable {EXTENSION_UUID}")),
        Err(error) => Err(error.to_string()),
    }
}

/// Tells the service the choices were saved, so it takes back any taskbar
/// mark they no longer ask for. Best effort: a service that is not running
/// holds no mark.
pub(crate) fn choices_changed() {
    if let Ok(connection) = Connection::session() {
        let _ = connection.call_method(
            Some(SERVICE_BUS_NAME),
            SERVICE_OBJECT_PATH,
            Some(SERVICE_INTERFACE),
            window_attention_core::bus::CHOICES_CHANGED_METHOD,
            &(),
        );
    }
}
