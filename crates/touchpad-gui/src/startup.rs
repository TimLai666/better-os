//! Everything that happens before the window opens.
//!
//! Assembling the application is its own step so it can be run without a
//! display server: the tests build a startup against fixture kernel trees and a
//! temporary configuration directory and assert what the first screen would
//! say, including the cases where there is no touchpad and no dconf service.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use touchpad_core::{StoreError, TouchpadConfig, TouchpadStore};
use touchpad_gestures::{GestureConfig, GestureStore};
use touchpad_platform::{
    DeviceInventory, GnomeBackend, MockBackend, Roots, Session, TouchpadBackend, devices,
};
use touchpad_session::MockSessionAdapter;

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
        let (gesture_config, gesture_problem) = match gesture_store.load_config() {
            Ok(config) => (config, None),
            Err(error) => (GestureConfig::default(), Some(error.to_string())),
        };
        let captured = gesture_store.load_capture().ok().flatten();
        // The recording adapter is the only one this build has. It reports
        // that it performs no system action, and the screen says so rather
        // than implying a gesture reaches the desktop.
        let mut gestures = GestureScreen::new(
            gesture_config,
            captured,
            Box::new(MockSessionAdapter::new()),
        );
        gestures.verify_all();
        gestures.set_problem(gesture_problem);

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

fn describe(error: StoreError) -> String {
    error.to_string()
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
