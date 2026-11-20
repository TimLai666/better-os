//! `better-touchpad-gestured`: the resident gesture pipeline.
//!
//! It connects to the session bus, takes `org.betteros.TouchpadGestures1`,
//! waits for the GNOME Shell adapter extension, and then reads gestures until
//! the session ends. There is no window, no polling loop, and no privilege: it
//! blocks on one stream of bus and file events and does nothing at all between
//! them.
//!
//! It never ends on purpose (ticket 131). With safe mode on, nothing
//! configured, or the integration turned off by itself, it stays resident and
//! idle, owning its bus name and watching `gestures.json` and the safe mode
//! marker, so a change made in a window takes effect at once and turning
//! gestures back on needs nothing else. A missing adapter is waited for.
//! Asked to stop by SIGTERM or SIGINT, it gives GNOME its gestures back first
//! and then ends by that same signal, so systemd records a stop and Better
//! Manager starts it again.

use std::process::ExitCode;
use std::sync::mpsc;

use touchpad_core::TouchpadStore;
use touchpad_gesture_service::{
    GesturePipeline, ServiceExit, SysfsDeviceIdentities, read_configuration, serve,
};
use touchpad_gestures::{ConfigurationWatch, GestureProfiles, GestureStore, WatchState};
use touchpad_session::{
    LauncherActivationAdapter, RoutingAdapter, ServiceBus, ServiceBusError, ServiceStatus,
    SessionAdapter, SessionEvent,
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

    // Installing is enough for gestures to work (owner, 2026-11-13): the first
    // run puts the shipped preset in force. A file that already exists, or
    // will not parse, is left exactly as it is. Safe mode writes nothing.
    if !settings.safe_mode_enabled() {
        match gestures.apply_first_run_default() {
            Ok(true) => eprintln!("first run: the Mac-style gestures are now in force"),
            Ok(false) => {}
            Err(error) => eprintln!("the first-run gestures could not be written: {error}"),
        }
    }

    let mut bus = match ServiceBus::connect() {
        Ok(bus) => bus,
        Err(ServiceBusError::AlreadyRunning) => {
            eprintln!("another process already owns org.betteros.TouchpadGestures1");
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
    // A watch that cannot be made (the person's inotify instances used up)
    // must not cost the gestures: the service keeps working on what it read,
    // says so in the journal and in its Status, and tries again on a slow
    // timer (ticket 131).
    let changes = bus.stop_sender();
    let status = bus.status_handle();
    let _watch = ConfigurationWatch::start(
        &[gestures.config_path(), settings.safe_mode_path()],
        touchpad_gestures::watch::RETRY,
        move || {
            let _ = changes.send(SessionEvent::ConfigurationChanged);
        },
        move |state| {
            let reported = match &state {
                WatchState::Watching => {
                    eprintln!("watching the gesture configuration");
                    ServiceStatus {
                        watching: true,
                        problem: String::new(),
                    }
                }
                WatchState::Unavailable(problem) => {
                    eprintln!(
                        "not watching the gesture configuration ({problem}); gestures keep \
                         working as read, a change reaches them when the watch is back, \
                         which is tried again every minute"
                    );
                    ServiceStatus {
                        watching: false,
                        problem: problem.clone(),
                    }
                }
            };
            if let Ok(mut slot) = status.lock() {
                *slot = reported;
            }
        },
    );

    let mut configuration = || read_configuration(&settings, &gestures);
    let mut pipeline_for =
        |adapter: Box<dyn SessionAdapter>, profiles: GestureProfiles| -> GesturePipeline {
            let mut routes: Vec<Box<dyn SessionAdapter>> = Vec::new();
            match launcher_platform::bus::SessionBusRegistry::connect_forward_only() {
                Ok(registry) => {
                    routes.push(Box::new(LauncherActivationAdapter::new(Box::new(registry))))
                }
                // The desktop's own gestures still work without it; the
                // launcher gesture then verifies as unsupported and says why.
                Err(error) => {
                    eprintln!("Better Launcher's activation route is unavailable: {error}")
                }
            }
            routes.push(adapter);
            GesturePipeline::with_profiles(
                profiles,
                GestureStore::at(&store_directory),
                Box::new(RoutingAdapter::new(routes)),
                Box::new(SysfsDeviceIdentities::system()),
            )
        };
    let exit = serve(
        &mut bus,
        &mut configuration,
        &mut pipeline_for,
        &mut |line| eprintln!("{line}"),
    );
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
/// gestures are back, so systemd records a stop (`ExecMainCode=killed`), which
/// Better Manager's startup pass and Check again start again.
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
