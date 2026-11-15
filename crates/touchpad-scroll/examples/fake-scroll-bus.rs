//! A stand-in pass-through on whatever bus `DBUS_SYSTEM_BUS_ADDRESS` names,
//! for looking at the settings window without a touchpad or a system service.
//! It serves the real interface name and methods, keeps the factors in memory,
//! and checks no caller.
//!
//! ```text
//! dbus-daemon --session --print-address --fork   # a private bus
//! DBUS_SYSTEM_BUS_ADDRESS=<that address> cargo run -p touchpad-scroll --example fake-scroll-bus
//! DBUS_SYSTEM_BUS_ADDRESS=<that address> better-touchpad
//! ```

use std::sync::Mutex;

use touchpad_scroll::factors::Factors;
use touchpad_scroll::ipc;
use zbus::{fdo, interface};

struct Fake(Mutex<Factors>);

#[interface(name = "org.betteros.TouchpadScroll1")]
impl Fake {
    async fn set_factors(&self, vertical: f64, horizontal: f64) -> fdo::Result<()> {
        let factors = Factors::new(vertical, horizontal).map_err(fdo::Error::InvalidArgs)?;
        eprintln!("set vertical {vertical} horizontal {horizontal}");
        *self.0.lock().unwrap() = factors;
        Ok(())
    }

    async fn describe(&self) -> (String, f64, f64) {
        let factors = *self.0.lock().unwrap();
        (
            "Fake Touchpad".to_string(),
            factors.vertical,
            factors.horizontal,
        )
    }
}

fn main() -> zbus::Result<()> {
    let initial = Factors::new(1.5, 1.0).expect("valid");
    let _connection = zbus::blocking::connection::Builder::system()?
        .name(ipc::bus_name("event99"))?
        .serve_at(ipc::OBJECT_PATH, Fake(Mutex::new(initial)))?
        .build()?;
    eprintln!("serving {}", ipc::bus_name("event99"));
    loop {
        std::thread::park();
    }
}
