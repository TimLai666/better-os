//! The power mode in force, followed as it changes (ticket 295).
//!
//! It is read from power-profiles-daemon on the system bus by the same
//! reader the System page uses (`hardware_platform::bus`), never from
//! `platform_profile`, which the daemon drives (ADR 0017). A mode set on the
//! System page, by "Switch power mode with the charger", or by any other
//! program reaches the daemon's `PropertiesChanged`, and the item reads again
//! at once; the daemon's name coming or going is followed too, so a daemon
//! that starts late or restarts is picked up.
//!
//! The reads are blocking calls on a thread of their own, so the item's own
//! connection and its runtime never wait on the system bus.

use std::sync::Arc;

use hardware_core::Presence;
use hardware_platform::bus;

use crate::Tray;
use crate::menu::PowerFacts;

/// Reads the daemon on `system` now and after every change, handing each
/// reading to `send`, until `send` returns `false` or the bus closes.
pub fn follow(system: &zbus::blocking::Connection, send: impl FnMut(PowerFacts) -> bool) {
    bus::follow_power_profiles(system, send);
}

/// Follows the daemon for `tray` on the system bus, from a thread of its
/// own. Without a system bus the mode cannot be read, which the menu says;
/// nothing is guessed.
pub fn spawn(tray: Arc<Tray>) {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<PowerFacts>();
    let reader = std::thread::Builder::new()
        .name("power-mode".into())
        .spawn(move || match zbus::blocking::Connection::system() {
            Ok(system) => follow(&system, |facts| sender.send(facts).is_ok()),
            Err(error) => {
                let _ = sender.send(Presence::Unreadable(format!(
                    "the system bus is not reachable: {error}"
                )));
            }
        });
    if let Err(error) = reader {
        eprintln!("better-monitor-tray: the power mode cannot be followed: {error}");
        return;
    }
    tokio::spawn(async move {
        let mut said = false;
        while let Some(facts) = receiver.recv().await {
            // Said once when it starts failing, as the service's state is.
            match &facts {
                Presence::Unreadable(reason) if !said => {
                    eprintln!("better-monitor-tray: the power mode could not be read: {reason}");
                    said = true;
                }
                Presence::Unreadable(_) => {}
                _ => said = false,
            }
            let changes = tray.state.lock().await.adopt_power(facts);
            tray.announce(changes).await;
        }
    });
}
