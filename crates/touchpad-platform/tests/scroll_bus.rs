//! The scroll pass-through's client against a fake service on a private bus.
//!
//! `SystemBusScroll` connects to the system bus, which zbus finds through
//! `DBUS_SYSTEM_BUS_ADDRESS`; this test points that at a private
//! `dbus-daemon` so nothing reaches the machine's own bus. It is the only test
//! in this binary because it sets that variable for the process.
#![cfg(feature = "dconf-write")]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use touchpad_core::{ScrollFactor, SettingId, SettingValue};
use touchpad_platform::scroll::{ScrollChannel, SystemBusScroll};
use touchpad_platform::{GnomeBackend, TouchpadBackend, WriteOutcome};
use touchpad_scroll::factors::Factors;
use touchpad_scroll::ipc;
use zbus::{fdo, interface};

struct Bus(Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn private_bus() -> Option<(Bus, String)> {
    let mut child = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut line = String::new();
    BufReader::new(child.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    Some((Bus(child), line.trim().to_string()))
}

struct Fake {
    factors: Arc<Mutex<Factors>>,
}

#[interface(name = "org.betteros.TouchpadScroll1")]
impl Fake {
    async fn set_factors(&self, vertical: f64, horizontal: f64) -> fdo::Result<()> {
        *self.factors.lock().unwrap() =
            Factors::new(vertical, horizontal).map_err(fdo::Error::InvalidArgs)?;
        Ok(())
    }

    async fn describe(&self) -> (String, f64, f64) {
        let factors = *self.factors.lock().unwrap();
        ("Fake".to_string(), factors.vertical, factors.horizontal)
    }
}

#[test]
fn the_window_reads_and_sets_every_pass_through_on_the_bus() {
    let (_bus, address) = test_prerequisites::require!(
        "dbus-daemon",
        private_bus().ok_or("a private dbus-daemon did not start")
    );
    // SAFETY: the only test in this binary, before any thread reads it.
    unsafe { std::env::set_var("DBUS_SYSTEM_BUS_ADDRESS", &address) };

    let client = SystemBusScroll::connect().unwrap();
    // No pass-through yet: nothing to read, and a write is refused.
    assert_eq!(client.factors(), Ok(None));
    assert!(client.set(Factors::NEUTRAL).is_err());

    let first = Arc::new(Mutex::new(Factors::new(2.0, 1.0).unwrap()));
    let second = Arc::new(Mutex::new(Factors::NEUTRAL));
    let mut servers = Vec::new();
    for (event, factors) in [("event3", &first), ("event9", &second)] {
        servers.push(
            zbus::blocking::connection::Builder::address(address.as_str())
                .unwrap()
                .name(ipc::bus_name(event))
                .unwrap()
                .serve_at(
                    ipc::OBJECT_PATH,
                    Fake {
                        factors: factors.clone(),
                    },
                )
                .unwrap()
                .build()
                .unwrap(),
        );
    }

    // The first by name answers the read.
    assert_eq!(client.factors(), Ok(Some(Factors::new(2.0, 1.0).unwrap())));

    // Through the backend: the factors are live, and a write reaches both.
    let mut backend = GnomeBackend::read_only("/nonexistent/dconf/user", None)
        .with_scroll(Box::new(client), None);
    assert!(
        backend
            .capabilities()
            .is_available(SettingId::VerticalScrollFactor)
    );
    let outcomes = backend.write_all(&[(
        SettingId::HorizontalScrollFactor,
        Some(SettingValue::factor(ScrollFactor::new(0.5).unwrap())),
    )]);
    assert_eq!(
        outcomes,
        vec![(SettingId::HorizontalScrollFactor, WriteOutcome::Written)]
    );
    assert_eq!(*first.lock().unwrap(), Factors::new(2.0, 0.5).unwrap());
    assert_eq!(*second.lock().unwrap(), Factors::new(2.0, 0.5).unwrap());

    // A value the service refuses is a failed write, not a silent one.
    let refused = SystemBusScroll::connect().unwrap().set(Factors {
        vertical: 50.0,
        horizontal: 1.0,
    });
    assert!(refused.is_err());
    drop(servers);
}
