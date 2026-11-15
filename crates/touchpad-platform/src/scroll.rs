//! The scroll factors, which GNOME has no key for, through Better Touchpad's
//! scroll pass-through (ADR 0030, `touchpad-scroll`).
//!
//! The factors are not a dconf value. Each touchpad has its own system service
//! (`better-touchpad-scroll@eventN.service`) that owns
//! `org.betteros.TouchpadScroll1.eventN` on the system bus; this module finds
//! them, reads the factors in force from the first, writes both factors to
//! every one, and reads them back to verify. It holds no privilege and opens
//! no device: the window asks, and the service, which checks that the caller
//! is the person at the machine, decides.
//!
//! The factors belong to the machine's touchpads, not to one person's
//! session, the same way the touchpad itself does.

use touchpad_scroll::factors::Factors;

/// What the GNOME backend needs from the pass-through. The real one is
/// [`SystemBusScroll`]; tests use a fake.
pub trait ScrollChannel: Send {
    /// The factors in force, or `None` when no touchpad has a pass-through.
    fn factors(&self) -> Result<Option<Factors>, String>;
    /// Set both factors on every touchpad's pass-through.
    fn set(&self, factors: Factors) -> Result<(), String>;
}

#[cfg(feature = "dconf-write")]
pub use bus::SystemBusScroll;

#[cfg(feature = "dconf-write")]
mod bus {
    use super::*;
    use touchpad_scroll::ipc;

    /// The pass-throughs on the system bus.
    pub struct SystemBusScroll {
        connection: zbus::blocking::Connection,
    }

    impl std::fmt::Debug for SystemBusScroll {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("SystemBusScroll")
        }
    }

    impl SystemBusScroll {
        pub fn connect() -> Result<Self, String> {
            zbus::blocking::Connection::system()
                .map(|connection| Self { connection })
                .map_err(|error| error.to_string())
        }

        /// Every pass-through's bus name, sorted.
        pub fn services(&self) -> Result<Vec<String>, String> {
            let dbus = zbus::blocking::fdo::DBusProxy::new(&self.connection)
                .map_err(|error| error.to_string())?;
            let mut names: Vec<String> = dbus
                .list_names()
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|name| name.to_string())
                .filter(|name| name.starts_with(ipc::BUS_NAME_PREFIX))
                .collect();
            names.sort();
            Ok(names)
        }

        fn describe(&self, destination: &str) -> Result<(String, f64, f64), String> {
            self.connection
                .call_method(
                    Some(destination),
                    ipc::OBJECT_PATH,
                    Some(ipc::INTERFACE),
                    "Describe",
                    &(),
                )
                .and_then(|reply| reply.body().deserialize::<(String, f64, f64)>())
                .map_err(|error| error.to_string())
        }

        fn set_factors(&self, destination: &str, factors: Factors) -> Result<(), String> {
            self.connection
                .call_method(
                    Some(destination),
                    ipc::OBJECT_PATH,
                    Some(ipc::INTERFACE),
                    "SetFactors",
                    &(factors.vertical, factors.horizontal),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }

    impl ScrollChannel for SystemBusScroll {
        fn factors(&self) -> Result<Option<Factors>, String> {
            let Some(first) = self.services()?.into_iter().next() else {
                return Ok(None);
            };
            let (_, vertical, horizontal) = self.describe(&first)?;
            Factors::new(vertical, horizontal).map(Some)
        }

        fn set(&self, factors: Factors) -> Result<(), String> {
            let services = self.services()?;
            if services.is_empty() {
                return Err("no touchpad has a scroll pass-through".to_string());
            }
            for service in services {
                self.set_factors(&service, factors)?;
            }
            Ok(())
        }
    }
}
