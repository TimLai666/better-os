//! Everything that happens before the window opens.
//!
//! Assembling the application is its own step so it can be run without a
//! display server: the tests build a startup against fixture kernel trees and a
//! temporary configuration directory and assert what the first screen would
//! say, including the cases where there is no touchpad and no dconf service.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use touchpad_core::{StoreError, TouchpadConfig, TouchpadStore};
use touchpad_gestures::{GestureProfiles, GestureStore, KnownShortcuts};
use touchpad_platform::{
    DeviceInventory, GnomeBackend, KeybindingReading, MockBackend, Roots, Session, TouchpadBackend,
    devices, keybindings,
};
use touchpad_session::{
    GnomeShellAdapter, LauncherActivationAdapter, MockSessionAdapter, RoutingAdapter,
    SessionAdapter, SessionBusShell,
};

use crate::gestures_model::GestureScreen;
use crate::i18n::Locale;
use crate::model::{Page, TouchpadModel};

/// How to build the application.
pub struct StartupOptions {
    /// Where the kernel interfaces are. A fixture tree in tests.
    pub roots: Roots,
    /// Where the configuration and the capture live.
    pub store_directory: PathBuf,
    pub locale: Locale,
    /// Read the desktop but never connect to the session bus. This is what the
    /// headless smoke test runs, so opening the window cannot depend on a bus.
    pub offline: bool,
    /// Use a backend that changes nothing outside its own memory. Only tests
    /// set this.
    pub in_memory: bool,
    /// The screen to open on. The headless launch smoke uses it to prove a
    /// particular screen renders, rather than only the one the window happens
    /// to start on.
    pub page: Page,
}

impl Default for StartupOptions {
    fn default() -> Self {
        Self {
            roots: Roots::system(),
            store_directory: TouchpadStore::user_directory(),
            locale: Locale::System,
            offline: false,
            in_memory: false,
            page: Page::Overview,
        }
    }
}

pub struct Startup {
    pub model: TouchpadModel,
    pub backend: Box<dyn TouchpadBackend>,
    pub store: TouchpadStore,
    pub gestures: GestureScreen,
    pub gesture_store: GestureStore,
    pub page: Page,
}

impl Startup {
    pub fn run(options: StartupOptions) -> Self {
        let store = TouchpadStore::new(&options.store_directory);
        let session = Session::detect();
        let inventory = devices::enumerate(&options.roots);

        let (config, problem) = match store.load_config() {
            Ok(config) => (config, None),
            // A configuration that will not parse is not a first run. The file
            // is left exactly as it is and the health screen says why, because
            // starting from defaults would overwrite the only copy.
            Err(error) => (TouchpadConfig::default(), Some(describe(error))),
        };

        let backend = build_backend(&options, &inventory);
        let status = backend.status();

        let mut model = TouchpadModel::new(
            config,
            backend.capabilities().clone(),
            session,
            status,
            backend.name(),
            inventory,
            options.locale,
        );
        model.set_configuration_problem(problem);
        model.set_safe_mode(store.safe_mode_enabled());
        model.refresh(backend.as_ref());
        if let Ok(Some(backup)) = store.load_backup() {
            model.adopt_backup(backup);
        }

        // The gesture half keeps its own two files and its own adapter, so a
        // gesture configuration that will not parse, or an adapter that
        // fails, cannot reach pointer movement or two-finger scrolling.
        let gesture_store = GestureStore::at(&options.store_directory);
        let (gesture_profiles, gesture_problem) = match gesture_store.load_profiles() {
            Ok(profiles) => (profiles, None),
            Err(error) => (GestureProfiles::default(), Some(error.to_string())),
        };
        let captured = gesture_store.load_capture().ok().flatten();
        let (adapter, bridge, service_running) = build_session_adapter(&options);
        if let Some((reachable, detail)) = bridge {
            let (connected, detail) = gesture_connection(reachable, detail, service_running);
            model.set_gesture_bridge(connected, detail);
        }
        // The gesture profile follows the device the rest of the window is
        // about, so switching the selected touchpad switches the gestures with
        // it rather than leaving the two screens describing different pads.
        let device = model
            .selected_device()
            .map(|device| device.identity.clone());
        let mut gestures =
            GestureScreen::with_profiles(gesture_profiles, device, captured, adapter);
        gestures.set_service_running(service_running);
        gestures.set_known_shortcuts(known_shortcuts(&options));
        gestures.verify_all(Some(&gesture_store));
        gestures.set_problem(gesture_problem);
        // Safe mode gives the desktop its own gestures back before the window
        // has drawn anything. It is the path a user reaches when the machine
        // has become hard to use, so it undoes rather than waits to be asked.
        if store.safe_mode_enabled() {
            gestures.enter_safe_mode();
        }

        Self {
            model,
            backend,
            store,
            gestures,
            gesture_store,
            page: options.page,
        }
    }
}

/// The route the Gestures screen speaks to the desktop through, and what to
/// say about the adapter bridge.
///
/// Three outcomes, and the difference between them is what the Diagnostics
/// screen shows. Offline never looks, so it reports nothing rather than
/// reporting the adapter as missing. A session with the extension gets the real
/// route — the launcher through the interface Better Launcher already serves,
/// and the desktop through the GNOME Shell adapter. A session without it falls
/// back to the recording adapter, which changes nothing and says so.
/// Why the adapter is not on the bus, as far as the shell can say: GNOME's
/// switch that turns off every extension wins over the extension list.
fn unreachable_adapter_reason(user_extensions_enabled: Option<bool>) -> String {
    match user_extensions_enabled {
        Some(false) => "GNOME's switch that turns off all extensions is on, so the GNOME Shell \
                        adapter extension cannot run"
            .to_string(),
        _ => "the GNOME Shell adapter extension is not enabled on this session".to_string(),
    }
}

/// What the gesture connection check says (ticket 131).
///
/// The adapter being on the bus is half of it. The other half is the gesture
/// service, the process that actually recognizes gestures: with the adapter
/// there and the service dead, the window can still reach the desktop, and
/// nothing a hand does on the touchpad is recognized. That is not connected,
/// and the check says which half is missing.
fn gesture_connection(
    adapter_reachable: bool,
    adapter_detail: String,
    service_running: Option<bool>,
) -> (bool, String) {
    match (adapter_reachable, service_running) {
        (true, Some(false)) => (
            false,
            "the GNOME Shell adapter answered, but the gesture service \
             (better-touchpad-gestures.service) is not running, so no gesture is recognized \
             and GNOME keeps its own"
                .to_string(),
        ),
        (true, Some(true)) => (
            true,
            format!("{adapter_detail}, and the gesture service is listening"),
        ),
        _ => (adapter_reachable, adapter_detail),
    }
}

/// The route, the bridge fact, and whether the gesture service owns its name
/// (`None` when nobody looked).
type SessionRoute = (
    Box<dyn SessionAdapter>,
    Option<(bool, String)>,
    Option<bool>,
);

fn build_session_adapter(options: &StartupOptions) -> SessionRoute {
    if options.offline || options.in_memory {
        return (Box::new(MockSessionAdapter::new()), None, None);
    }
    let shell = match SessionBusShell::connect() {
        Ok(shell) if shell.is_reachable() => shell,
        Ok(shell) => {
            return (
                Box::new(MockSessionAdapter::new()),
                Some((
                    false,
                    unreachable_adapter_reason(shell.user_extensions_enabled()),
                )),
                // The missing adapter is the reason to show; the service
                // waits for it either way.
                None,
            );
        }
        Err(error) => {
            return (
                Box::new(MockSessionAdapter::new()),
                Some((false, error.to_string())),
                None,
            );
        }
    };
    let service_running = Some(shell.gesture_service_running());
    // The window never takes GNOME's gestures itself: the service does, once
    // gestures arrive, and the extension refuses anyone else (ticket 131).
    let adapter = match GnomeShellAdapter::connect(Box::new(shell)) {
        Ok(adapter) => adapter.leaving_suppression_to_the_service(),
        Err(error) => {
            return (
                Box::new(MockSessionAdapter::new()),
                Some((false, error.to_string())),
                service_running,
            );
        }
    };
    let detail = format!(
        "the GNOME Shell adapter answered, on shell {}",
        adapter.reported().shell_version
    );
    let mut routes: Vec<Box<dyn SessionAdapter>> = Vec::new();
    // The launcher route is separate and may be absent on its own: a session
    // with the extension and no Better Launcher still has working desktop
    // gestures, and the launcher row then says what is missing.
    if let Ok(registry) = launcher_platform::bus::SessionBusRegistry::connect() {
        routes.push(Box::new(LauncherActivationAdapter::new(Box::new(registry))));
    }
    routes.push(Box::new(adapter));
    (
        Box::new(RoutingAdapter::new(routes)),
        Some((true, detail)),
        service_running,
    )
}

fn describe(error: StoreError) -> String {
    error.to_string()
}

/// The keyboard shortcuts the session has recorded, for the collision note the
/// shortcut picker shows.
///
/// A test build reads no database at all, because there is no fixture for one
/// and reading the developer's own would make the result depend on the machine
/// the tests run on. That is [`KnownShortcuts::unavailable`] with the reason,
/// not an empty list, so the screen says it could not check rather than saying
/// there is no conflict.
fn known_shortcuts(options: &StartupOptions) -> KnownShortcuts {
    if options.in_memory {
        return KnownShortcuts::unavailable("gestures.shortcuts_not_read_in_test_mode");
    }
    match keybindings::read(&GnomeBackend::user_database_path()) {
        KeybindingReading::Recorded(bindings) => KnownShortcuts::from_bindings(
            bindings
                .into_iter()
                .map(|binding| (binding.key, binding.binding)),
        ),
        KeybindingReading::Unknown { reason, .. } => KnownShortcuts::unavailable(reason),
    }
}

fn build_backend(
    options: &StartupOptions,
    inventory: &DeviceInventory,
) -> Box<dyn TouchpadBackend> {
    if options.in_memory {
        return Box::new(MockBackend::new());
    }
    let device = inventory
        .select(None)
        .map(|device| device.capabilities.clone());
    if options.offline {
        return Box::new(GnomeBackend::read_only(
            GnomeBackend::user_database_path(),
            device.as_ref(),
        ));
    }
    Box::new(GnomeBackend::connect(device.as_ref()))
}

/// Seconds since the Unix epoch, for stamping a capture.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod gesture_connection_tests {
    use super::gesture_connection;

    #[test]
    fn a_dead_gesture_service_is_not_connected_even_with_the_adapter_there() {
        let (connected, detail) =
            gesture_connection(true, "the adapter answered".to_string(), Some(false));
        assert!(!connected);
        assert!(detail.contains("gesture service"), "{detail}");
        assert!(detail.contains("not running"), "{detail}");
    }

    #[test]
    fn both_halves_there_is_connected() {
        let (connected, detail) =
            gesture_connection(true, "the adapter answered".to_string(), Some(true));
        assert!(connected);
        assert!(detail.contains("listening"), "{detail}");
    }

    #[test]
    fn a_missing_adapter_keeps_its_own_reason() {
        let (connected, detail) =
            gesture_connection(false, "the adapter is not enabled".to_string(), None);
        assert!(!connected);
        assert_eq!(detail, "the adapter is not enabled");
    }
}

#[cfg(test)]
mod adapter_reason_tests {
    use super::unreachable_adapter_reason;

    #[test]
    fn the_all_extensions_off_switch_is_named() {
        assert!(unreachable_adapter_reason(Some(false)).contains("turns off all extensions"));
        assert!(unreachable_adapter_reason(Some(true)).contains("not enabled"));
        assert!(unreachable_adapter_reason(None).contains("not enabled"));
    }
}
