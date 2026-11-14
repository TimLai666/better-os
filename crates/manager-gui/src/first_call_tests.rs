//! A window answers the first `ShowPage` that arrives the instant it owns
//! the name.
//!
//! zbus starts a connection's object server lazily, and a call that reaches
//! the connection before the server is reading is dropped with no reply
//! (ticket 107). A second `better-manager` forwards the moment it finds the
//! name taken, so the real claim path, [`bus::claim`] on a [`bus::connect`]
//! connection, is started [`STARTS`] times on a private `dbus-daemon`, and
//! each time a client calls the moment the bus says the name has an owner.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::deep_link::{BUS_NAME, INTERFACE, OBJECT_PATH, Request, bus};

/// How many times a window claims the name.
const STARTS: u32 = 200;

/// How long a call may go unanswered once the window owns the name, and how
/// long the name may take to change hands; long enough that only a call nobody
/// will ever answer, or a claim that never happens, runs into it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut address = String::new();
        let read = child
            .stdout
            .take()
            .map(|out| BufReader::new(out).read_line(&mut address));
        let bus = Self {
            child,
            address: address.trim().to_string(),
        };
        (matches!(read, Some(Ok(_))) && !bus.address.is_empty()).then_some(bus)
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn every_claim_answers_a_call_made_the_instant_the_name_appears() {
    let Some(private) = PrivateBus::start() else {
        eprintln!("skipping: dbus-daemon is not available in this environment");
        return;
    };
    let caller = bus::connect(Some(&private.address), ANSWER_TIMEOUT).unwrap();
    // The owner changes are read on a thread of their own and handed over a
    // channel, so a claim that panics or never happens fails the test after
    // ANSWER_TIMEOUT instead of leaving it waiting for a signal forever.
    let (changes, owner_changes) = std::sync::mpsc::channel();
    {
        let dbus = zbus::blocking::fdo::DBusProxy::new(&caller).unwrap();
        let owners = dbus
            .receive_name_owner_changed_with_args(&[(0, BUS_NAME)])
            .unwrap();
        std::thread::spawn(move || {
            for signal in owners {
                let owned = signal.args().is_ok_and(|args| args.new_owner().is_some());
                if changes.send(owned).is_err() {
                    return;
                }
            }
        });
    }
    let next_owner = || owner_changes.recv_timeout(ANSWER_TIMEOUT).ok();

    let mut unanswered = Vec::new();
    for start in 0..STARTS {
        let address = private.address.clone();
        let window = std::thread::spawn(move || {
            bus::claim(bus::connect(Some(&address), bus::FORWARD_TIMEOUT).unwrap()).unwrap()
        });

        // The instant the bus says the name has an owner. A claim that
        // panicked is reported with its own message.
        match next_owner() {
            Some(true) => {}
            other => {
                if window.is_finished()
                    && let Err(panic) = window.join()
                {
                    std::panic::resume_unwind(panic);
                }
                panic!("start {start}: the name gained no owner ({other:?})");
            }
        }
        let answer = caller.call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "ShowPage",
            &("",),
        );

        let claim = window
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        let bus::Claim::Owner(owner) = claim else {
            panic!("start {start}: the window owns the name");
        };
        match answer {
            Ok(_) => assert_eq!(
                owner.requests.try_recv(),
                Ok(Request::Raise),
                "start {start}: the answered call reached the window"
            ),
            Err(error) => unanswered.push(format!("start {start}: {error}")),
        }

        // Closing the window gives the name up before the next one claims it.
        drop(owner);
        assert_eq!(
            next_owner(),
            Some(false),
            "start {start}: the name was given up"
        );
    }

    assert!(
        unanswered.is_empty(),
        "{} of {STARTS} claims left the first call unanswered: {unanswered:#?}",
        unanswered.len()
    );
    eprintln!("{STARTS} of {STARTS} claims answered the first call");
}
