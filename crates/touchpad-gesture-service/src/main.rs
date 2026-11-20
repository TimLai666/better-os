//! `better-touchpad-gestured`: the resident gesture pipeline.
//!
//! It connects to the session bus, takes `org.betteros.TouchpadGestures1`,
//! waits for the GNOME Shell adapter extension, and then reads gestures until
//! the session ends. There is no window, no polling loop, and no privilege: it
//! blocks on one stream of bus events and does nothing at all between them.
//!
//! It ends on purpose in two cases only, both with status 0: safe mode is on,
//! or no gesture is configured (which includes the integration having turned
//! itself off). Better Manager's health rule reads exactly that as finished
//! on purpose. A missing adapter is waited for, never a reason to exit (ticket
//! 131: the adapter appears a moment after a login starts this service, and
//! exiting then left gestures dead for the whole session). Asked to stop by
//! SIGTERM or SIGINT, it gives GNOME its gestures back first and then ends by
//! that same signal, so systemd records a stop and not a finish.

use std::process::ExitCode;
use std::sync::mpsc;

use touchpad_core::TouchpadStore;
use touchpad_gesture_service::{
    GesturePipeline, ServiceExit, SysfsDeviceIdentities, safe_mode_enabled, serve,
};
use touchpad_gestures::GestureStore;
use touchpad_session::{
    GnomeShellAdapter, LauncherActivationAdapter, RoutingAdapter, ServiceBus, ServiceBusError,
    SessionAdapter, SessionBusShell, SessionEvent,
};

fn main() -> ExitCode {
    // SIGTERM (systemd stopping the unit, a logout, Better Manager's restart)
    // and SIGINT are taken by a thread of their own, so GNOME's gestures are
    // given back before the process goes. They are blocked here, before any
    // other thread exists, so every thread inherits the mask and only that one
    // receives them.
    let stops = match stop_on_signals() {
        Ok(stops) => stops,
        Err(error) => {
            eprintln!("the stop signals could not be handled: {error}");
            return ExitCode::FAILURE;
        }
    };

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
            }
            Err(reason) => {
                eprintln!(
                    "safe mode is on: no gesture is recognized; the adapter could not be \
                     reached to restore GNOME's gestures ({reason}), and it restores them \
                     itself when it starts without this service"
                );
            }
        }
        eprintln!("exit: safe mode, nothing to listen for");
        return ExitCode::SUCCESS;
    }

    // Installing is enough for gestures to work (owner, 2026-11-13): the first
    // run puts the shipped preset in force. A file that already exists, or
    // will not parse, is left exactly as it is.
    match gestures.apply_first_run_default() {
        Ok(true) => eprintln!("first run: the Mac-style gestures are now in force"),
        Ok(false) => {}
        Err(error) => eprintln!("the first-run gestures could not be written: {error}"),
    }

    match gestures.load_profiles() {
        Ok(profiles) => {
            let listening = std::iter::once(&profiles.global)
                .chain(profiles.devices.values())
                .any(|profile| !profile.active().is_empty());
            if !listening {
                eprintln!("exit: no gesture is configured, so there is nothing to listen for");
                return ExitCode::SUCCESS;
            }
        }
        // A configuration that will not parse is not a first run, and starting
        // from defaults would recognize gestures the user never configured.
        Err(error) => {
            eprintln!("the gesture configuration could not be read: {error}");
            return ExitCode::FAILURE;
        }
    }

    let mut bus = match ServiceBus::connect() {
        Ok(bus) => bus,
        Err(ServiceBusError::AlreadyRunning) => {
            eprintln!("another better-touchpad-gestured already owns its bus name");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("the session bus is not reachable: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (signal_number, stop) = stops;
    let to_bus = bus.stop_sender();
    std::thread::spawn(move || {
        if stop.recv().is_ok() {
            let _ = to_bus.send(SessionEvent::Stop);
        }
    });

    let mut pipeline_for = |adapter: Box<dyn SessionAdapter>| -> Result<GesturePipeline, String> {
        let profiles = gestures
            .load_profiles()
            .map_err(|error| format!("the gesture configuration could not be read: {error}"))?;
        let mut routes: Vec<Box<dyn SessionAdapter>> = Vec::new();
        match launcher_platform::bus::SessionBusRegistry::connect_forward_only() {
            Ok(registry) => {
                routes.push(Box::new(LauncherActivationAdapter::new(Box::new(registry))))
            }
            // The desktop's own gestures still work without it; the launcher
            // gesture then verifies as unsupported and says why.
            Err(error) => eprintln!("Better Launcher's activation route is unavailable: {error}"),
        }
        routes.push(adapter);
        Ok(GesturePipeline::with_profiles(
            profiles,
            GestureStore::at(&store_directory),
            Box::new(RoutingAdapter::new(routes)),
            Box::new(SysfsDeviceIdentities::system()),
        ))
    };
    let exit = serve(&mut bus, &mut pipeline_for, &mut |line| eprintln!("{line}"));
    drop(bus);

    let restored_words = |restored: bool| {
        if restored {
            "GNOME has its own gestures"
        } else {
            "GNOME's gestures could not be restored from here; the extension restores them \
             now that this service has left the bus"
        }
    };
    match exit {
        ServiceExit::Stopped { restored } => {
            let signal = signal_number.load(std::sync::atomic::Ordering::SeqCst);
            eprintln!(
                "exit: stopped by signal {signal}; {}",
                restored_words(restored)
            );
            end_by_signal(signal);
            ExitCode::SUCCESS
        }
        ServiceExit::TurnedOff { restored } => {
            eprintln!(
                "exit: gestures turned themselves off, so there is nothing to listen for; {}",
                restored_words(restored)
            );
            ExitCode::SUCCESS
        }
        ServiceExit::Closed { restored } => {
            eprintln!(
                "exit: the session bus is gone; {}",
                restored_words(restored)
            );
            ExitCode::FAILURE
        }
        ServiceExit::Failed(error) => {
            eprintln!("exit: {error}");
            ExitCode::FAILURE
        }
    }
}

type Stops = (
    std::sync::Arc<std::sync::atomic::AtomicI32>,
    mpsc::Receiver<()>,
);

/// Blocks SIGTERM and SIGINT for the process and waits for either on a thread
/// of its own; the receiver gets one message when one arrives, and the number
/// says which.
fn stop_on_signals() -> std::io::Result<Stops> {
    // SAFETY: plain libc calls on a set this function owns, made before any
    // other thread is started.
    let set = unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        let blocked = libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        if blocked != 0 {
            return Err(std::io::Error::from_raw_os_error(blocked));
        }
        set
    };
    let number = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(libc::SIGTERM));
    let seen = number.clone();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut received: libc::c_int = 0;
        // SAFETY: `set` is a valid, initialised signal set.
        if unsafe { libc::sigwait(&set, &mut received) } == 0 {
            seen.store(received, std::sync::atomic::Ordering::SeqCst);
            let _ = sender.send(());
        }
    });
    Ok((number, receiver))
}

/// Ends the process by the signal that asked it to stop, now that GNOME's
/// gestures are back, so systemd records a stop (`ExecMainCode=killed`) and
/// Better Manager never reads it as a service that finished on purpose.
fn end_by_signal(signal: i32) {
    // SAFETY: restoring the default action and unblocking one signal in this
    // thread, then raising it; nothing else runs in between.
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, signal);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        libc::raise(signal);
    }
}

/// Reaches the extension only to put GNOME's gestures back.
fn restore_only() -> Result<(), String> {
    let shell = SessionBusShell::connect().map_err(|error| error.to_string())?;
    if !shell.is_reachable() {
        return Err("the adapter extension is not on the session bus".to_string());
    }
    let mut adapter =
        GnomeShellAdapter::connect(Box::new(shell)).map_err(|error| error.to_string())?;
    adapter.suppress_built_in_gestures(false);
    Ok(())
}
