//! Reads what a machine has for the System page and writes what needs no
//! privilege (ticket 86, spec section 11).
//!
//! - [`sysfs`] reads `power_supply`, `hwmon`, `leds`, and DMI under a root
//!   a test points at a fixture tree, and finds the two attributes the
//!   daemon may write.
//! - [`bus`] talks to power-profiles-daemon, gsd-power, UPower, and GNOME
//!   Shell, on connections the caller supplies.
//! - [`privileged`] asks `manager-daemon`'s `org.betteros.Hardware1` for the
//!   two root writes.
//! - [`follow_store`] keeps the person's "switch with the charger" choice.
//!
//! Nothing here writes sysfs. Every root write goes through the daemon.

pub mod bus;
#[cfg(feature = "test-support")]
pub mod fakes;
pub mod follow_store;
pub mod privileged;
pub mod sysfs;
pub mod usb;

/// Reads everything the System page shows, at once, blocking. A bus that
/// could not be reached is `None`, and what lives on it reads as absent,
/// unless sysfs shows the hardware is there (a backlight, a keyboard LED):
/// then it is present and unreadable, never a made-up value.
#[cfg(feature = "sensors")]
pub fn read_machine(
    root: &sysfs::SysRoot,
    system: Option<&zbus::blocking::Connection>,
    session: Option<&zbus::blocking::Connection>,
) -> hardware_core::page::MachineFacts {
    use hardware_core::Presence;
    let power_mode = system.map_or(Presence::Absent, bus::power_profiles);
    let mut brightness = session.map_or(Presence::Absent, bus::screen_brightness);
    if brightness.is_absent() && sysfs::display_backlight(root).is_some() && session.is_none() {
        brightness = Presence::Unreadable("the session bus is not reachable".to_string());
    }
    let mut keyboard = match (session, system) {
        (Some(session), Some(system)) => bus::keyboard(session, system),
        _ => Presence::Absent,
    };
    if keyboard.is_absent() && sysfs::keyboard_led(root).is_some() {
        keyboard = Presence::Unreadable(
            "neither gsd-power nor UPower answers for the keyboard backlight".to_string(),
        );
    }
    hardware_core::page::MachineFacts {
        power_mode,
        charge_limit: sysfs::charge_limit(root),
        battery: sysfs::battery(root),
        charge: sysfs::charge_readings(root),
        brightness,
        keyboard,
        lighting: sysfs::lighting_hint(root),
        fan: sysfs::fan_mode(root),
        sensors: sysfs::sensors(root),
    }
}
