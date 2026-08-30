//! AC power and battery percentage, from `/sys/class/power_supply`.
//!
//! One provider serves two condition kinds because they come from the same
//! directory scan; splitting them would double the I/O to answer the same
//! question twice.
//!
//! Polling rather than netlink is a deliberate, bounded choice. A `udev` netlink
//! socket would deliver charger events immediately, but it needs a socket, a
//! reader task, and a reconnect path, and the thing being watched moves on the
//! order of seconds at best. Ten seconds of latency on "the charger was
//! unplugged" is not a behaviour a user can perceive; two small file reads every
//! ten seconds is not a cost a user can perceive either. If the event path is
//! ever wanted, [`PowerProvider`] is where it lands and nothing above it changes.

use std::path::PathBuf;

use awake_core::{Observations, ProviderKind};

use crate::provider::{Cadence, POWER_POLL_SECONDS, TriggerProvider};
use crate::roots::{ReadError, Roots, list_dir, read_attribute, read_u64_attribute};

/// What one scan of the power supplies found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PowerReading {
    /// `None` when no mains supply exists at all, which is what a desktop with
    /// no battery and no charger node looks like.
    pub ac_connected: Option<bool>,
    /// `None` when the machine has no battery.
    pub battery_percent: Option<u8>,
    /// Whether any battery node was found. This is what "battery-powered
    /// device" means for the safety default, and it is answered from the
    /// hardware rather than guessed from the chassis type.
    pub has_battery: bool,
}

/// Reads the charger and the battery.
#[derive(Clone, Debug)]
pub struct PowerProvider {
    roots: Roots,
    /// Which condition kind this instance reports availability for. The two
    /// share a scan but are separate capabilities: a desktop has a charger and
    /// no battery, and saying "battery percentage is unavailable" there is the
    /// honest answer rather than reporting a hundred percent forever.
    kind: ProviderKind,
}

impl PowerProvider {
    /// The AC-power half.
    pub fn ac(roots: Roots) -> Self {
        Self {
            roots,
            kind: ProviderKind::AcPower,
        }
    }

    /// The battery-percentage half.
    pub fn battery(roots: Roots) -> Self {
        Self {
            roots,
            kind: ProviderKind::BatteryPercent,
        }
    }

    fn supplies_dir(&self) -> PathBuf {
        self.roots.sys_path("class/power_supply")
    }

    /// Scans every power supply node once.
    pub fn read(&self) -> Result<PowerReading, ReadError> {
        let entries = list_dir(&self.supplies_dir())?;
        let mut reading = PowerReading::default();

        for entry in entries {
            let Ok(supply_type) = read_attribute(&entry.join("type")) else {
                // A node with no type is not a power supply this code
                // understands. Skipping it is right; failing the whole scan
                // because one USB-C port exposed something odd is not.
                continue;
            };
            match supply_type.as_str() {
                "Mains" | "USB" | "USB_PD" | "USB_PD_DRP" => {
                    if let Ok(online) = read_u64_attribute(&entry.join("online")) {
                        // Any supply reporting online means the machine is on
                        // external power, so the answer is a union across nodes
                        // rather than whichever node was read last.
                        reading.ac_connected =
                            Some(reading.ac_connected.unwrap_or(false) || online != 0);
                    }
                }
                "Battery" => {
                    reading.has_battery = true;
                    if let Ok(capacity) = read_u64_attribute(&entry.join("capacity")) {
                        // A driver reporting past 100 is reporting nonsense; it
                        // is clamped rather than allowed to make a `u8` wrap.
                        let percent = capacity.min(100) as u8;
                        // With two batteries the lower one is the one that will
                        // run out, so it is the one protection must act on.
                        reading.battery_percent = Some(match reading.battery_percent {
                            Some(existing) => existing.min(percent),
                            None => percent,
                        });
                    }
                }
                _ => continue,
            }
        }

        Ok(reading)
    }

    /// Whether this machine runs on a battery, which decides whether the battery
    /// stop threshold is on by default.
    pub fn has_battery(&self) -> bool {
        self.read()
            .map(|reading| reading.has_battery)
            .unwrap_or(false)
    }
}

impl TriggerProvider for PowerProvider {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn cadence(&self) -> Cadence {
        Cadence::Poll {
            seconds: POWER_POLL_SECONDS,
        }
    }

    fn sample(&mut self, _now_unix_seconds: u64, into: &mut Observations) {
        match self.read() {
            Err(error) => into.mark_unavailable(self.kind, error.explanation()),
            Ok(reading) => match self.kind {
                ProviderKind::AcPower => match reading.ac_connected {
                    Some(connected) => {
                        into.ac_power_connected = Some(connected);
                        into.mark_available(ProviderKind::AcPower);
                    }
                    None => into
                        .mark_unavailable(ProviderKind::AcPower, "awake.provider.no_mains_supply"),
                },
                ProviderKind::BatteryPercent => match reading.battery_percent {
                    Some(percent) => {
                        into.battery_percent = Some(percent);
                        into.mark_available(ProviderKind::BatteryPercent);
                    }
                    None => into.mark_unavailable(
                        ProviderKind::BatteryPercent,
                        "awake.provider.no_battery",
                    ),
                },
                other => into.mark_unavailable(other, "awake.provider.wrong_provider"),
            },
        }
    }
}

/// Builds a fake `/sys/class/power_supply` node, used by tests here and by the
/// service's battery-safety tests.
#[cfg(any(test, feature = "test-support"))]
pub fn write_supply(sys_dir: &std::path::Path, name: &str, attributes: &[(&str, &str)]) {
    let directory = sys_dir.join("class/power_supply").join(name);
    std::fs::create_dir_all(&directory).unwrap();
    for (attribute, value) in attributes {
        std::fs::write(directory.join(attribute), format!("{value}\n")).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(supplies: &[(&str, &[(&str, &str)])]) -> (tempfile::TempDir, Roots) {
        let directory = tempfile::tempdir().unwrap();
        let sys = directory.path().join("sys");
        for (name, attributes) in supplies {
            write_supply(&sys, name, attributes);
        }
        let roots = Roots::at(directory.path());
        (directory, roots)
    }

    #[test]
    fn a_laptop_on_the_charger_reports_ac_and_its_battery_level() {
        let (_directory, roots) = fixture(&[
            ("ACAD", &[("type", "Mains"), ("online", "1")]),
            ("BAT1", &[("type", "Battery"), ("capacity", "65")]),
        ]);

        let mut observations = Observations::at(1_000);
        PowerProvider::ac(roots.clone()).sample(1_000, &mut observations);
        PowerProvider::battery(roots).sample(1_000, &mut observations);

        assert_eq!(observations.ac_power_connected, Some(true));
        assert_eq!(observations.battery_percent, Some(65));
        assert!(
            observations
                .availability_of(ProviderKind::AcPower)
                .is_available()
        );
    }

    #[test]
    fn a_desktop_with_no_battery_says_so_instead_of_reporting_full() {
        let (_directory, roots) = fixture(&[("ACAD", &[("type", "Mains"), ("online", "1")])]);

        let mut observations = Observations::at(1_000);
        PowerProvider::battery(roots.clone()).sample(1_000, &mut observations);

        assert_eq!(observations.battery_percent, None);
        assert_eq!(
            observations
                .availability_of(ProviderKind::BatteryPercent)
                .explanation(),
            Some("awake.provider.no_battery"),
            "a machine with no battery must not be reported as a full one"
        );
        assert!(!PowerProvider::battery(roots).has_battery());
    }

    #[test]
    fn a_machine_with_no_power_supply_directory_at_all_is_unavailable_not_unplugged() {
        let directory = tempfile::tempdir().unwrap();
        let roots = Roots::at(directory.path());

        let mut observations = Observations::at(1_000);
        PowerProvider::ac(roots).sample(1_000, &mut observations);

        assert_eq!(observations.ac_power_connected, None);
        let explanation = observations
            .availability_of(ProviderKind::AcPower)
            .explanation()
            .unwrap()
            .to_string();
        assert!(
            explanation.starts_with("awake.provider.interface_missing"),
            "unexpected explanation: {explanation}"
        );
    }

    #[test]
    fn two_batteries_report_the_one_that_will_run_out_first() {
        let (_directory, roots) = fixture(&[
            ("BAT0", &[("type", "Battery"), ("capacity", "80")]),
            ("BAT1", &[("type", "Battery"), ("capacity", "22")]),
        ]);
        assert_eq!(roots_battery(&roots), Some(22));
    }

    #[test]
    fn a_usb_c_supply_that_is_delivering_power_counts_as_on_ac() {
        let (_directory, roots) = fixture(&[
            ("ACAD", &[("type", "Mains"), ("online", "0")]),
            (
                "ucsi-source-psy-USBC000:001",
                &[("type", "USB"), ("online", "1")],
            ),
        ]);
        let mut observations = Observations::at(1_000);
        PowerProvider::ac(roots).sample(1_000, &mut observations);
        assert_eq!(
            observations.ac_power_connected,
            Some(true),
            "a laptop charged over USB-C is on external power"
        );
    }

    #[test]
    fn a_supply_node_with_no_type_does_not_fail_the_whole_scan() {
        let (_directory, roots) = fixture(&[
            ("weird", &[("online", "1")]),
            ("BAT1", &[("type", "Battery"), ("capacity", "50")]),
        ]);
        assert_eq!(roots_battery(&roots), Some(50));
    }

    #[test]
    fn a_driver_reporting_past_a_hundred_percent_is_clamped_not_wrapped() {
        let (_directory, roots) = fixture(&[("BAT1", &[("type", "Battery"), ("capacity", "300")])]);
        assert_eq!(roots_battery(&roots), Some(100));
    }

    #[test]
    fn a_malformed_capacity_leaves_the_battery_unknown_rather_than_zero() {
        let (_directory, roots) =
            fixture(&[("BAT1", &[("type", "Battery"), ("capacity", "unknown")])]);
        let mut observations = Observations::at(1_000);
        PowerProvider::battery(roots).sample(1_000, &mut observations);
        assert_eq!(
            observations.battery_percent, None,
            "an unreadable capacity is not a flat battery"
        );
    }

    #[test]
    fn the_battery_provider_polls_at_the_documented_interval() {
        let provider = PowerProvider::battery(Roots::system());
        assert_eq!(
            provider.cadence(),
            Cadence::Poll {
                seconds: POWER_POLL_SECONDS
            }
        );
    }

    fn roots_battery(roots: &Roots) -> Option<u8> {
        let mut observations = Observations::at(1_000);
        PowerProvider::battery(roots.clone()).sample(1_000, &mut observations);
        observations.battery_percent
    }
}
