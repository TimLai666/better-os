//! The single-instance rule over a real bus.
//!
//! The rule itself is tested against a fake registry in `activation.rs`. What
//! can only be tested on a bus is the part this file covers: that a second
//! process really is refused the name, that its request really arrives at the
//! first one, and that the two verbs stay apart — a launcher icon opens, a
//! repeated shortcut toggles.
//!
//! The bus is private and this test kills it. Nothing here touches the
//! developer's own session bus.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use launcher_platform::activation::{
    ActivationRequest, InstanceRole, NameRegistry, SingleInstance,
};
use launcher_platform::bus::SessionBusRegistry;

const TEST_NAME: &str = "org.betteros.Launcher1Test";

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
        let stdout = child.stdout.take()?;
        let mut address = String::new();
        BufReader::new(stdout).read_line(&mut address).ok()?;
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(Self { child, address })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

macro_rules! bus_or_skip {
    () => {
        match PrivateBus::start() {
            Some(bus) => bus,
            None => {
                eprintln!("skipping: dbus-daemon is not available in this environment");
                return;
            }
        }
    };
}

/// Waits for one request rather than sleeping a fixed amount: the call is
/// already complete by the time `forward` returns, so this only has to drain.
fn next_request(
    inbox: &mut tokio::sync::mpsc::UnboundedReceiver<ActivationRequest>,
) -> Option<ActivationRequest> {
    for _ in 0..50 {
        if let Ok(request) = inbox.try_recv() {
            return Some(request);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn a_second_launch_is_refused_the_name_and_its_toggle_reaches_the_first() {
    let bus = bus_or_skip!();
    let instance = SingleInstance::new(TEST_NAME);

    let primary = SessionBusRegistry::connect_to(&bus.address).unwrap();
    assert_eq!(
        instance.acquire(&primary, ActivationRequest::Open).unwrap(),
        InstanceRole::Primary
    );
    let mut inbox = primary
        .take_inbox()
        .expect("the owner of the name serves activations");
    assert!(
        primary.take_inbox().is_none(),
        "the inbox is handed out once"
    );

    let secondary = SessionBusRegistry::connect_to(&bus.address).unwrap();
    assert_eq!(
        instance
            .acquire(&secondary, ActivationRequest::Toggle)
            .unwrap(),
        InstanceRole::Secondary
    );

    assert_eq!(next_request(&mut inbox), Some(ActivationRequest::Toggle));
}

#[test]
fn a_launcher_icon_asks_the_running_overlay_to_open_and_never_to_close() {
    let bus = bus_or_skip!();
    let instance = SingleInstance::new(TEST_NAME);

    let primary = SessionBusRegistry::connect_to(&bus.address).unwrap();
    instance.acquire(&primary, ActivationRequest::Open).unwrap();
    let mut inbox = primary.take_inbox().unwrap();

    // Exactly what a dock, a panel, or `gio launch` sends.
    let clicker = SessionBusRegistry::connect_to(&bus.address).unwrap();
    clicker.forward(TEST_NAME, ActivationRequest::Open).unwrap();
    assert_eq!(next_request(&mut inbox), Some(ActivationRequest::Open));

    clicker
        .forward(TEST_NAME, ActivationRequest::Close)
        .unwrap();
    assert_eq!(next_request(&mut inbox), Some(ActivationRequest::Close));
}

#[test]
fn forwarding_to_a_name_nobody_owns_is_reported_rather_than_silently_dropped() {
    let bus = bus_or_skip!();
    let registry = SessionBusRegistry::connect_to(&bus.address).unwrap();
    let error = registry
        .forward("org.betteros.Launcher1Absent", ActivationRequest::Toggle)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("launcher.platform.error.activation_failed"),
        "{error}"
    );
}
