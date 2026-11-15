//! `better-touchpad-scrolld`: the scroll-speed pass-through for one touchpad.
//!
//! Started by udev, one instance per touchpad node, as
//! `better-touchpad-scroll@eventN.service`. In order:
//!
//! 1. As root, inside a unit whose device policy allows exactly
//!    `/dev/input/eventN` and `/dev/uinput`: open those two nodes and nothing
//!    else, and judge the touchpad by its capabilities. Anything that is not a
//!    touchpad, or that can report a key, is refused and the instance exits.
//! 2. Drop to `_better-touchpad-scroll`, with no supplementary groups and no
//!    capabilities. Everything after this runs unprivileged on the two
//!    descriptors already open.
//! 3. Own `org.betteros.TouchpadScroll1.eventN` on the system bus and run the
//!    event loop, beating the watchdog as it turns.
//!
//! `--eligible` prints the touchpads sysfs describes, judged by the same rule,
//! for the maintainer script.

mod bus;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use touchpad_scroll::factors::{Factors, SharedFactors};
use touchpad_scroll::linux::{EvdevSource, UinputFactory};
use touchpad_scroll::passthrough::{Passthrough, Source};
use touchpad_scroll::{classify, ipc, notify, sysfs};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

const USAGE: &str = "usage: better-touchpad-scrolld eventN | --eligible | --clone-properties SYSPATH | --help | --version";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--help"] => {
            println!("{USAGE}");
            println!("Scales one touchpad's scrolling for Better Touchpad. Started by udev.");
            ExitCode::SUCCESS
        }
        ["--version"] => {
            println!("better-touchpad-scrolld {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        ["--clone-properties", syspath] => match touchpad_scroll::udev::clone_properties(syspath) {
            Some(properties) => {
                print!("{properties}");
                ExitCode::SUCCESS
            }
            None => ExitCode::FAILURE,
        },
        ["--eligible"] => {
            for name in sysfs::eligible(&sysfs::class_root()) {
                println!("{name}");
            }
            ExitCode::SUCCESS
        }
        [event] if sysfs::is_event_name(event) => run(event),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(event: &str) -> ExitCode {
    let node = PathBuf::from("/dev/input").join(event);
    let source = match EvdevSource::open(&node) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}: {error}", node.display());
            return ExitCode::FAILURE;
        }
    };
    if let Err(refusal) = classify(source.descriptor()) {
        // Success, so the unit is not restarted: the answer will not change.
        eprintln!(
            "{} ({}) is not held: {refusal}",
            node.display(),
            source.descriptor().name
        );
        return ExitCode::SUCCESS;
    }
    let factory = match UinputFactory::open(Path::new("/dev/uinput")) {
        Ok(factory) => factory,
        Err(error) => {
            eprintln!("/dev/uinput: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = drop_privileges() {
        eprintln!("could not drop privileges, so nothing is held: {error}");
        return ExitCode::FAILURE;
    }
    // From here the uinput descriptor can declare no key outside the button
    // block, whatever the code running in this process does.
    if let Err(error) = touchpad_scroll::seccomp::install() {
        eprintln!("could not install the uinput filter, so nothing is held: {error}");
        return ExitCode::FAILURE;
    }

    let state_file = PathBuf::from(ipc::STATE_FILE);
    let (stored, problem) = Factors::load(&state_file);
    if let Some(problem) = problem {
        eprintln!("stored factors ignored, holding nothing until set again: {problem}");
    }
    let factors = SharedFactors::new(stored);
    let device_name = source.descriptor().name.clone();

    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }

    // The bus is how the factor changes, not how the pad works: without it the
    // stored factors still apply, and the window says the service cannot be
    // reached.
    let service = bus::ScrollService {
        factors: factors.clone(),
        state_file,
        device_name: device_name.clone(),
        refusals: Default::default(),
    };
    let _connection = match zbus::blocking::connection::Builder::system()
        .and_then(|builder| builder.name(ipc::bus_name(event)))
        .and_then(|builder| builder.serve_at(ipc::OBJECT_PATH, service))
        .and_then(|builder| builder.build())
    {
        Ok(connection) => Some(connection),
        Err(error) => {
            eprintln!("system bus unavailable, the stored factors still apply: {error}");
            None
        }
    };

    eprintln!(
        "{} ({device_name}): vertical {} horizontal {}",
        node.display(),
        stored.vertical,
        stored.horizontal
    );
    let _ = notify::notify("READY=1");

    let mut passthrough = Passthrough::new(source, factory);
    let mut beat = Instant::now();
    let mut report = Instant::now();
    let mut mode = None;
    while !STOP.load(Ordering::SeqCst) {
        let current = factors.get();
        if let Err(error) = passthrough.step(current, Duration::from_millis(250)) {
            if error.raw_os_error() == Some(libc::ENODEV) {
                // A failure, not a success: the unit is BindsTo= its device,
                // and a node that came back under the same name after a
                // replug or a resume must get a fresh instance.
                eprintln!("{} is gone", node.display());
                return ExitCode::FAILURE;
            }
            eprintln!("{}: {error}", node.display());
            return ExitCode::FAILURE;
        }
        let now = passthrough.mode(current);
        if mode != Some(now) {
            eprintln!("{}: {now:?}", node.display());
            mode = Some(now);
        }
        if report.elapsed() >= Duration::from_secs(60) {
            let cost = passthrough.take_cost();
            if cost.batches > 0 {
                eprintln!(
                    "{}: forwarded {} reads, added {:?} on average and {:?} at worst",
                    node.display(),
                    cost.batches,
                    cost.mean(),
                    cost.worst
                );
            }
            report = Instant::now();
        }
        if beat.elapsed() >= Duration::from_secs(1) {
            let _ = notify::notify("WATCHDOG=1");
            beat = Instant::now();
        }
    }
    let _ = notify::notify("STOPPING=1");
    // `passthrough` is dropped here: ungrab, then destroy the clone.
    ExitCode::SUCCESS
}

/// Become `_better-touchpad-scroll`, with no groups and no way back.
///
/// A process not started as root (a developer running the binary by hand)
/// stays who it is.
fn drop_privileges() -> Result<(), String> {
    // SAFETY: plain libc calls on values this function owns; getpwnam is
    // called before any other thread exists.
    unsafe {
        if libc::geteuid() != 0 {
            return Ok(());
        }
        let name = std::ffi::CString::new(ipc::SERVICE_USER).expect("no NUL in the name");
        let entry = libc::getpwnam(name.as_ptr());
        if entry.is_null() {
            return Err(format!("the account {} does not exist", ipc::SERVICE_USER));
        }
        let (uid, gid) = ((*entry).pw_uid, (*entry).pw_gid);
        if uid == 0 {
            return Err(format!("{} is uid 0", ipc::SERVICE_USER));
        }
        if libc::setgroups(0, std::ptr::null()) != 0
            || libc::setgid(gid) != 0
            || libc::setuid(uid) != 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if libc::setuid(0) == 0 || libc::geteuid() == 0 {
            return Err("root could be regained".to_string());
        }
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}
