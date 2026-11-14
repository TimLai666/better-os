//! `better-window-attention`: the user service behind Better Manager's
//! Window attention page. See the library documentation and ADR 0018.

use std::process::ExitCode;

use window_attention_core::store::Store;
use window_attention_service::Relay;
use window_attention_service::bus::{ServeError, serve_until};

fn main() -> ExitCode {
    if let Some(argument) = std::env::args().nth(1) {
        if argument == "--version" {
            println!("better-window-attention {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "better-window-attention takes no arguments; it is started by its user unit \
             (better-window-attention.service)"
        );
        return ExitCode::from(2);
    }
    // SIGTERM (systemd stopping the unit, the package's removal) and SIGINT
    // are taken by a thread of their own, so the service can take back its
    // taskbar marks before it goes. They are blocked here, before any other
    // thread exists, so every thread inherits the mask and only that one
    // receives them.
    let stop = match stop_on_signals() {
        Ok(stop) => stop,
        Err(error) => {
            eprintln!("the stop signals could not be handled: {error}");
            return ExitCode::FAILURE;
        }
    };
    let connection = match zbus::blocking::Connection::session() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("the session bus is not reachable: {error}");
            return ExitCode::FAILURE;
        }
    };
    let relay = Relay::new(Store::for_user());
    match serve_until(connection, relay, |line| eprintln!("{line}"), stop) {
        Ok(()) => ExitCode::SUCCESS,
        // Another instance is already answering; this one has nothing to do.
        Err(ServeError::AlreadyRunning) => {
            eprintln!("another better-window-attention is already running");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Blocks SIGTERM and SIGINT for the process and waits for either on a thread
/// of its own; the receiver gets one message when one arrives.
fn stop_on_signals() -> std::io::Result<std::sync::mpsc::Receiver<()>> {
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
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut received: libc::c_int = 0;
        // SAFETY: `set` is a valid, initialised signal set.
        if unsafe { libc::sigwait(&set, &mut received) } == 0 {
            let _ = sender.send(());
        }
    });
    Ok(receiver)
}
