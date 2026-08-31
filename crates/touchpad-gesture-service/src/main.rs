//! `better-touchpad-gestured`: the resident gesture pipeline.
//!
//! It connects to the session bus, asks the GNOME Shell adapter extension what
//! it can do, and then reads gestures until the session ends. There is no
//! window, no polling loop, and no privilege: it blocks on the extension's
//! signal stream and does nothing at all between gestures.
//!
//! Everything it can refuse to start for is a stated reason with its own exit
//! status, because a service that exits silently and a service that is working
//! look the same in a journal.

use std::process::ExitCode;

use touchpad_core::TouchpadStore;
use touchpad_gesture_service::{GesturePipeline, StopReason, safe_mode_enabled};
use touchpad_gestures::{GestureStore, SuppressionEvent};
use touchpad_session::{
    GnomeShellAdapter, LauncherActivationAdapter, RoutingAdapter, SessionBusShell,
};

fn main() -> ExitCode {
    let store_directory = TouchpadStore::user_directory();
    let settings = TouchpadStore::new(&store_directory);
    let gestures = GestureStore::at(&store_directory);

    if safe_mode_enabled(&settings) {
        // Safe mode is the path a user reaches from a text console when the
        // desktop has become hard to use. Nothing is recognized, and the
        // desktop's own gestures are given back before anything else runs.
        match restore_only() {
            Ok(()) => {
                eprintln!("safe mode is on: no gesture is recognized and GNOME keeps its own");
                return ExitCode::SUCCESS;
            }
            Err(reason) => {
                eprintln!("safe mode is on and the adapter could not be reached: {reason}");
                return ExitCode::SUCCESS;
            }
        }
    }

    let config = match gestures.load_config() {
        Ok(config) => config,
        // A configuration that will not parse is not a first run, and starting
        // from defaults would recognize gestures the user never configured.
        Err(error) => {
            eprintln!("the gesture configuration could not be read: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !config.enabled || config.active().is_empty() {
        eprintln!("no gesture is configured, so there is nothing to listen for");
        return ExitCode::SUCCESS;
    }

    let shell = match SessionBusShell::connect() {
        Ok(shell) => shell,
        Err(error) => {
            eprintln!("the session bus is not reachable: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !shell.is_reachable() {
        eprintln!(
            "the GNOME Shell adapter extension is not on the session bus; \
             install and enable touchpad-adapter@betteros.org"
        );
        return ExitCode::SUCCESS;
    }
    let adapter = match GnomeShellAdapter::connect(Box::new(shell)) {
        Ok(adapter) => adapter,
        Err(error) => {
            eprintln!("the GNOME Shell adapter did not answer: {error}");
            return ExitCode::FAILURE;
        }
    };
    let launcher = match launcher_platform::bus::SessionBusRegistry::connect() {
        Ok(registry) => LauncherActivationAdapter::new(Box::new(registry)),
        Err(error) => {
            eprintln!("Better Launcher's activation route is unavailable: {error}");
            return ExitCode::FAILURE;
        }
    };
    let routing = RoutingAdapter::new(vec![Box::new(launcher), Box::new(adapter)]);

    let mut events = match SessionBusShell::connect().and_then(|shell| shell.events()) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("the adapter's gesture signals could not be subscribed to: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut pipeline = GesturePipeline::new(config, gestures, Box::new(routing));
    let state = pipeline.verify();
    eprintln!("gestures verified: {state:?}");
    let suppression = pipeline.suppress_built_in_gestures();
    eprintln!("built-in gestures: {suppression:?}");

    let reason = pipeline.run(&mut events);
    // Whatever ended it, GNOME gets its gestures back on the way out.
    pipeline.restore_built_in_gestures(match reason {
        StopReason::AutoDisabled => SuppressionEvent::Disabled,
        StopReason::SafeMode => SuppressionEvent::SafeMode,
        StopReason::StreamEnded => SuppressionEvent::Restored,
    });
    if let Some(problem) = pipeline.problem() {
        eprintln!("{problem}");
    }
    eprintln!("stopped: {reason:?}");
    ExitCode::SUCCESS
}

/// Reaches the extension only to put GNOME's gestures back.
fn restore_only() -> Result<(), String> {
    let shell = SessionBusShell::connect().map_err(|error| error.to_string())?;
    if !shell.is_reachable() {
        return Err("the adapter extension is not on the session bus".to_string());
    }
    let mut adapter =
        GnomeShellAdapter::connect(Box::new(shell)).map_err(|error| error.to_string())?;
    use touchpad_session::SessionAdapter;
    adapter.suppress_built_in_gestures(false);
    Ok(())
}
