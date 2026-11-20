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
use crate::health_words::GestureBridge;
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
        let (adapter, bridge, service_running, watch_problem) = build_session_adapter(&options);
        if let Some(fact) = bridge {
            let anything_configured = std::iter::once(&gesture_profiles.global)
                .chain(gesture_profiles.devices.values())
                .any(|profile| !profile.active().is_empty());
            model.set_gesture_bridge(gesture_connection(
                fact,
                service_running,
                watch_problem,
                store.safe_mode_enabled(),
                anything_configured,
            ));
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
///
/// What looking for the adapter found.
#[derive(Clone, Debug, Eq, PartialEq)]
enum AdapterFact {
    Answered {
        shell_version: String,
    },
    /// Not on the bus. GNOME's switch that turns off every extension wins over
    /// the extension list, and is named when it is the reason.
    Missing {
        all_extensions_off: bool,
    },
    Failed(String),
}

/// What the gesture connection check says (ticket 131).
///
/// The adapter being on the bus is half of it. The other half is the gesture
/// service, the process that actually recognizes gestures: with the adapter
/// there and the service dead, the window can still reach the desktop, and
/// nothing a hand does on the touchpad is recognized. That is not connected,
/// and the check says which half is missing. A service that runs with safe
/// mode on, or with nothing switched on, is there and idle, and the check says
/// that instead.
fn gesture_connection(
    adapter: AdapterFact,
    service_running: Option<bool>,
    watch_problem: Option<String>,
    safe_mode: bool,
    anything_configured: bool,
) -> GestureBridge {
    match (adapter, service_running) {
        (AdapterFact::Answered { .. }, Some(true)) if watch_problem.is_some() => {
            GestureBridge::NotWatching {
                problem: watch_problem.unwrap_or_default(),
            }
        }
        (AdapterFact::Missing { all_extensions_off }, _) => {
            GestureBridge::AdapterMissing { all_extensions_off }
        }
        (AdapterFact::Failed(detail), _) => GestureBridge::Unreachable { detail },
        (AdapterFact::Answered { .. }, Some(false)) => GestureBridge::ServiceStopped,
        (AdapterFact::Answered { .. }, _) if safe_mode => GestureBridge::SafeMode,
        (AdapterFact::Answered { .. }, _) if !anything_configured => {
            GestureBridge::NothingConfigured
        }
        (AdapterFact::Answered { shell_version }, _) => GestureBridge::Listening { shell_version },
    }
}

/// The route, the bridge fact, whether the gesture service owns its name
/// (`None` when nobody looked), and why it cannot watch its configuration,
/// when it says it cannot.
type SessionRoute = (
    Box<dyn SessionAdapter>,
    Option<AdapterFact>,
    Option<bool>,
    Option<String>,
);

fn build_session_adapter(options: &StartupOptions) -> SessionRoute {
    if options.offline || options.in_memory {
        return (Box::new(MockSessionAdapter::new()), None, None, None);
    }
    let shell = match SessionBusShell::connect() {
        Ok(shell) if shell.is_reachable() => shell,
        Ok(shell) => {
            return (
                Box::new(MockSessionAdapter::new()),
                Some(AdapterFact::Missing {
                    all_extensions_off: shell.user_extensions_enabled() == Some(false),
                }),
                // The missing adapter is the reason to show; the service
                // waits for it either way.
                None,
                None,
            );
        }
        Err(error) => {
            return (
                Box::new(MockSessionAdapter::new()),
                Some(AdapterFact::Failed(error.to_string())),
                None,
                None,
            );
        }
    };
    let service_running = Some(shell.gesture_service_running());
    let watch_problem = shell
        .gesture_service_status()
        .filter(|status| !status.watching)
        .map(|status| status.problem);
    // The window never takes GNOME's gestures itself: the service does, once
    // gestures arrive, and the extension refuses anyone else (ticket 131).
    let adapter = match GnomeShellAdapter::connect(Box::new(shell)) {
        Ok(adapter) => adapter.leaving_suppression_to_the_service(),
        Err(error) => {
            return (
                Box::new(MockSessionAdapter::new()),
                Some(AdapterFact::Failed(error.to_string())),
                service_running,
                watch_problem,
            );
        }
    };
    let fact = AdapterFact::Answered {
        shell_version: adapter.reported().shell_version.clone(),
    };
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
        Some(fact),
        service_running,
        watch_problem,
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
    let database = match GnomeBackend::session_database() {
        Ok(database) => database,
        Err(error) => return KnownShortcuts::unavailable(error.reason()),
    };
    match keybindings::read(&database.path) {
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
        return Box::new(GnomeBackend::read_only_session(device.as_ref()));
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
    use super::{AdapterFact, gesture_connection};
    use crate::health_words::GestureBridge;
    use crate::i18n::Locale;

    fn answered() -> AdapterFact {
        AdapterFact::Answered {
            shell_version: "46.0".to_string(),
        }
    }

    #[test]
    fn a_dead_gesture_service_is_not_connected_even_with_the_adapter_there() {
        let bridge = gesture_connection(answered(), Some(false), None, false, true);
        assert_eq!(bridge, GestureBridge::ServiceStopped);
        assert!(!bridge.connected());
        let english = bridge.words(Locale::EnUs);
        assert!(english.contains("not running"), "{english}");
        assert!(english.contains("Check again"), "{english}");
        assert!(bridge.words(Locale::ZhTw).contains("重新檢查"));
    }

    #[test]
    fn both_halves_there_is_connected() {
        let bridge = gesture_connection(answered(), Some(true), None, false, true);
        assert!(bridge.connected());
        assert!(bridge.words(Locale::EnUs).contains("listening"));
    }

    /// Safe mode and nothing switched on leave the service running and idle,
    /// and the check says so rather than "listening" or "not running".
    #[test]
    fn a_resident_idle_service_is_said_to_be_idle() {
        let safe = gesture_connection(answered(), Some(true), None, true, true);
        assert_eq!(safe, GestureBridge::SafeMode);
        assert!(safe.connected());
        assert!(safe.words(Locale::EnUs).contains("safe mode"));
        assert!(safe.words(Locale::ZhTw).contains("安全模式"));

        let idle = gesture_connection(answered(), Some(true), None, false, false);
        assert_eq!(idle, GestureBridge::NothingConfigured);
        assert!(idle.words(Locale::EnUs).contains("waits for a gesture"));
    }

    /// Review of ticket 131: a watch that cannot be made costs no gesture,
    /// and the check says why changes are slow to arrive, in both languages.
    #[test]
    fn a_service_that_cannot_watch_is_degraded_and_says_why() {
        let bridge = gesture_connection(
            answered(),
            Some(true),
            Some("inotify instances used up".to_string()),
            false,
            true,
        );
        assert!(!bridge.connected());
        assert!(
            bridge
                .words(Locale::EnUs)
                .contains("inotify instances used up")
        );
        assert!(bridge.words(Locale::EnUs).contains("every minute"));
        assert!(bridge.words(Locale::ZhTw).contains("無法追蹤設定檔"));
    }

    #[test]
    fn a_missing_adapter_names_the_all_extensions_off_switch() {
        let off = gesture_connection(
            AdapterFact::Missing {
                all_extensions_off: true,
            },
            None,
            None,
            false,
            true,
        );
        assert!(!off.connected());
        assert!(off.words(Locale::EnUs).contains("turns off all extensions"));
        let missing = gesture_connection(
            AdapterFact::Missing {
                all_extensions_off: false,
            },
            None,
            None,
            false,
            true,
        );
        assert!(missing.words(Locale::EnUs).contains("not enabled"));
        assert!(missing.words(Locale::ZhTw).contains("沒有啟用"));
    }
}
