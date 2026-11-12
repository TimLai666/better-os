//! Bringing the tray up when the window opens.
//!
//! The tray starts from its autostart entry at login, once. A tray that started
//! while the service was not running, or that someone quit, stays gone until the
//! next login, and the owner found Better Awake with no taskbar icon for that
//! reason. Opening Better Awake is the moment a person is looking for it, so the
//! window starts the tray when no tray is on the bus.
//!
//! The tray is the same program its autostart entry runs, started with no
//! argument and no shell. A tray that is already running owns
//! [`TRAY_BUS_NAME`] and nothing is started.

use std::process::{Command, Stdio};

use awake_ipc::notification::TRAY_BUS_NAME;

/// The program the tray autostart entry runs.
pub(crate) const TRAY_PROGRAM: &str = "awake-tray";

/// Whether to start the tray, from whether its bus name has an owner.
/// `None` means the bus could not be asked; nothing is started then, because
/// a tray started with no session bus would only fail.
pub(crate) fn should_start(name_has_owner: Option<bool>) -> bool {
    name_has_owner == Some(false)
}

/// Asks the session bus whether a tray is running.
fn tray_is_running() -> Option<bool> {
    let connection = zbus::blocking::Connection::session().ok()?;
    let bus = zbus::blocking::fdo::DBusProxy::new(&connection).ok()?;
    let name = zbus::names::BusName::try_from(TRAY_BUS_NAME).ok()?;
    bus.name_has_owner(name).ok()
}

/// Starts the tray when none is running, off the caller's thread, and reaps
/// it when it exits so no zombie is left behind.
pub(crate) fn ensure_running() {
    std::thread::spawn(|| {
        if !should_start(tray_is_running()) {
            return;
        }
        match Command::new(TRAY_PROGRAM)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(error) => eprintln!("awake-gui: the tray could not be started: {error}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_is_started_only_when_the_bus_says_none_is_running() {
        assert!(should_start(Some(false)));
        assert!(!should_start(Some(true)));
        assert!(!should_start(None));
    }

    #[test]
    fn the_tray_program_is_the_one_its_autostart_entry_runs() {
        let entry = include_str!("../../../packaging/awake/better-awake-tray.desktop");
        assert!(
            entry
                .lines()
                .any(|line| line == format!("Exec={TRAY_PROGRAM}"))
        );
    }
}
