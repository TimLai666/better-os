//! Batteries and chargers, from `/sys/class/power_supply`.
//!
//! The kernel's `Documentation/ABI/testing/sysfs-class-power` defines the
//! attributes. A battery reports either energy (`energy_*`, microwatt-hours)
//! or charge (`charge_*`, microampere-hours); a charge-reporting battery is
//! converted to watt-hours with its design minimum voltage, the same rule
//! UPower uses, so every battery is described in the same unit. The draw comes
//! from `power_now` or, for a charge-reporting battery, `current_now` times
//! `voltage_now`.
//!
//! A value the firmware does not report is said to be not reported. Many
//! laptops publish `cycle_count` as 0 on a battery that has plainly been
//! cycled; that zero is a placeholder and is reported as such, not as a new
//! battery.

use crate::catalog::{collector_id, derived_source, gauge, identity, metric_id, sys_source};
use crate::fsread::{ReadError, list_dir, read_attribute};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnsupportedReason,
};
use std::path::Path;

pub const BATTERY_COLLECTOR: &str = "linux.battery";

/// What a battery is doing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatteryStatus {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

impl BatteryStatus {
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "Charging" => Self::Charging,
            "Discharging" => Self::Discharging,
            "Full" => Self::Full,
            "Not charging" => Self::NotCharging,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::Full => "full",
            Self::NotCharging => "not_charging",
            Self::Unknown => "unknown",
        }
    }
}

/// One battery, read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatteryReading {
    pub name: String,
    pub status: Option<BatteryStatus>,
    /// `0.0..=1.0`.
    pub charge: Option<f64>,
    /// Watt-hours.
    pub energy_now: Option<f64>,
    pub energy_full: Option<f64>,
    pub energy_design: Option<f64>,
    /// Watts flowing in or out, as a magnitude.
    pub power: Option<f64>,
    pub voltage: Option<f64>,
    /// `None` when absent; `Some(0)` is kept so the caller can say the
    /// firmware does not count.
    pub cycles: Option<u64>,
    pub technology: Option<String>,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    /// The charge limit the firmware stops at, `0.0..=1.0`.
    pub charge_limit: Option<f64>,
    /// Whether the battery belongs to an attached device (a mouse) rather
    /// than to this machine.
    pub device_scope: bool,
}

impl BatteryReading {
    /// Full capacity as a share of what it was designed to hold.
    pub fn health(&self) -> Option<f64> {
        let design = self.energy_design.filter(|d| *d > 0.0)?;
        Some((self.energy_full? / design).clamp(0.0, 1.5))
    }

    /// Seconds until empty while discharging, or until full while charging,
    /// at the current draw. `None` when there is no draw to divide by.
    pub fn time_remaining(&self) -> Option<(BatteryStatus, f64)> {
        let power = self.power.filter(|p| *p > 0.05)?;
        match self.status? {
            BatteryStatus::Discharging => Some((
                BatteryStatus::Discharging,
                self.energy_now? / power * 3600.0,
            )),
            BatteryStatus::Charging => {
                let target = match self.charge_limit {
                    Some(limit) if limit > 0.0 => self.energy_full? * limit,
                    _ => self.energy_full?,
                };
                Some((
                    BatteryStatus::Charging,
                    (target - self.energy_now?).max(0.0) / power * 3600.0,
                ))
            }
            _ => None,
        }
    }
}

/// One thing that supplies power: a mains adapter or a USB-C port.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerSourceReading {
    pub name: String,
    /// `mains`, `usb`, or the kernel's type in lower case.
    pub kind: String,
    pub online: Option<bool>,
    /// The negotiated USB type, the bracketed choice in `usb_type`.
    pub usb_type: Option<String>,
    /// The most it can deliver now, in watts.
    pub max_power: Option<f64>,
}

fn number(dir: &Path, name: &str) -> Option<f64> {
    read_attribute(&dir.join(name)).ok()?.parse::<f64>().ok()
}

fn text(dir: &Path, name: &str) -> Option<String> {
    read_attribute(&dir.join(name))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The bracketed choice in a sysfs list such as `C [PD] PD_PPS`.
pub fn bracketed(raw: &str) -> Option<String> {
    let start = raw.find('[')?;
    let end = raw[start..].find(']')? + start;
    Some(raw[start + 1..end].to_string())
}

/// Read one battery directory.
pub fn read_battery(dir: &Path, name: &str) -> BatteryReading {
    let micro = |file: &str| number(dir, file).map(|v| v / 1e6);
    let design_voltage = micro("voltage_min_design").or_else(|| micro("voltage_max_design"));
    let voltage = micro("voltage_now");
    // Energy files are microwatt-hours; charge files are microampere-hours and
    // become watt-hours through the design voltage.
    let energy = |stem: &str| -> Option<f64> {
        micro(&format!("energy_{stem}"))
            .or_else(|| Some(micro(&format!("charge_{stem}"))? * design_voltage.or(voltage)?))
    };
    let energy_now = energy("now");
    let energy_full = energy("full");
    let energy_design = energy("full_design");
    let power = micro("power_now")
        .or_else(|| Some(micro("current_now")? * voltage?))
        .map(f64::abs);
    let charge = number(dir, "capacity")
        .map(|percent| (percent / 100.0).clamp(0.0, 1.0))
        .or_else(|| {
            let full = energy_full.filter(|f| *f > 0.0)?;
            Some((energy_now? / full).clamp(0.0, 1.0))
        });
    BatteryReading {
        name: name.to_string(),
        status: text(dir, "status").map(|s| BatteryStatus::parse(&s)),
        charge,
        energy_now,
        energy_full,
        energy_design,
        power,
        voltage,
        cycles: number(dir, "cycle_count").map(|c| c.max(0.0) as u64),
        technology: text(dir, "technology"),
        model: text(dir, "model_name"),
        manufacturer: text(dir, "manufacturer"),
        charge_limit: number(dir, "charge_control_end_threshold")
            .filter(|limit| *limit > 0.0 && *limit <= 100.0)
            .map(|limit| limit / 100.0),
        device_scope: text(dir, "scope").as_deref() == Some("Device"),
    }
}

/// Read one mains adapter or USB port.
pub fn read_power_source(dir: &Path, name: &str, kind: &str) -> PowerSourceReading {
    let micro = |file: &str| number(dir, file).map(|v| v / 1e6);
    let online = number(dir, "online").map(|v| v > 0.0);
    let voltage = micro("voltage_now")
        .filter(|v| *v > 0.0)
        .or(micro("voltage_max"));
    let max_power = match (online, voltage, micro("current_max")) {
        (Some(true), Some(volts), Some(amps)) if volts > 0.0 && amps > 0.0 => Some(volts * amps),
        _ => None,
    };
    PowerSourceReading {
        name: name.to_string(),
        kind: kind.to_ascii_lowercase(),
        online,
        usb_type: text(dir, "usb_type").and_then(|raw| bracketed(&raw)),
        max_power,
    }
}

/// Everything under `class/power_supply`.
pub fn scan_power_supply(
    roots: &Roots,
) -> Result<(Vec<BatteryReading>, Vec<PowerSourceReading>), ReadError> {
    let mut batteries = Vec::new();
    let mut sources = Vec::new();
    for dir in list_dir(&roots.sys("class/power_supply"))? {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let Some(kind) = text(&dir, "type") else {
            continue;
        };
        if kind == "Battery" {
            if number(&dir, "present") == Some(0.0) {
                continue;
            }
            batteries.push(read_battery(&dir, &name));
        } else {
            sources.push(read_power_source(&dir, &name, &kind));
        }
    }
    Ok((batteries, sources))
}

/// Batteries and power sources.
pub struct BatteryCollector {
    roots: Roots,
}

fn or_not_reported<T>(
    value: Option<T>,
    what: &str,
    make: impl Fn(T) -> Observation,
) -> Observation {
    match value {
        Some(value) => make(value),
        None => Observation::Unsupported(UnsupportedReason::NotReported {
            detail: format!("the battery does not report {what}"),
        }),
    }
}

impl BatteryCollector {
    pub fn new(roots: Roots) -> Self {
        Self { roots }
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        let source = || sys_source("class/power_supply");
        vec![
            gauge(
                "power.on_battery",
                Unit::None,
                derived_source("a system battery reports Discharging"),
                "whether the machine is running on its battery",
            ),
            gauge(
                "power.battery.draw",
                Unit::Watts,
                derived_source("sum of discharging batteries' power"),
                "what the machine is drawing from its batteries",
            ),
            gauge(
                "battery.charge",
                Unit::Ratio,
                source(),
                "capacity, or energy_now over energy_full",
            ),
            identity(
                "battery.status",
                source(),
                "charging, discharging, full, not_charging, or unknown",
            ),
            gauge(
                "battery.energy.now",
                Unit::WattHours,
                source(),
                "energy_now, or charge_now times the design voltage",
            ),
            gauge(
                "battery.energy.full",
                Unit::WattHours,
                source(),
                "energy_full, or charge_full times the design voltage",
            ),
            gauge(
                "battery.energy.design",
                Unit::WattHours,
                source(),
                "energy_full_design, or charge_full_design times the design voltage",
            ),
            gauge(
                "battery.health",
                Unit::Ratio,
                derived_source("full capacity over design capacity"),
                "how much of its design capacity the battery still holds",
            ),
            gauge(
                "battery.power",
                Unit::Watts,
                source(),
                "power_now, or current_now times voltage_now",
            ),
            gauge(
                "battery.time.remaining",
                Unit::Seconds,
                derived_source("energy left over the current draw"),
                "time until empty while discharging",
            ),
            gauge(
                "battery.time.to_full",
                Unit::Seconds,
                derived_source("energy missing over the current charging power"),
                "time until full, or until the charge limit, while charging",
            ),
            gauge(
                "battery.cycles",
                Unit::Count,
                source(),
                "cycle_count, unreported when the firmware publishes 0",
            ),
            gauge("battery.voltage", Unit::Volts, source(), "voltage_now"),
            gauge(
                "battery.charge.limit",
                Unit::Ratio,
                source(),
                "charge_control_end_threshold",
            ),
            identity("battery.technology", source(), "technology"),
            identity("battery.model", source(), "model_name"),
            identity("battery.manufacturer", source(), "manufacturer"),
            gauge(
                "battery.peripheral",
                Unit::None,
                source(),
                "whether the battery belongs to an attached device",
            ),
            gauge(
                "power_source.online",
                Unit::None,
                source(),
                "whether an adapter or port is supplying power",
            ),
            identity("power_source.kind", source(), "mains, usb, or another type"),
            identity(
                "power_source.usb_type",
                source(),
                "the negotiated USB power type",
            ),
            gauge(
                "power_source.power.max",
                Unit::Watts,
                source(),
                "voltage times the current limit of an online port",
            ),
        ]
    }

    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(BATTERY_COLLECTOR), at);
        let (batteries, sources) = match scan_power_supply(roots) {
            Ok(found) => found,
            Err(ReadError::Missing { path }) => {
                report.health = CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
                    path: path.display().to_string(),
                });
                return report;
            }
            Err(other) => {
                report.health = CollectorHealth::Failed {
                    detail: other.path().display().to_string(),
                };
                return report;
            }
        };

        let system: Vec<&BatteryReading> = batteries.iter().filter(|b| !b.device_scope).collect();
        let discharging: Vec<&&BatteryReading> = system
            .iter()
            .filter(|b| b.status == Some(BatteryStatus::Discharging))
            .collect();
        if system.is_empty() {
            report.metrics.insert(
                metric_id("power.on_battery"),
                Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "this machine has no battery".into(),
                }),
            );
        } else {
            report.metrics.insert(
                metric_id("power.on_battery"),
                Observation::boolean(!discharging.is_empty()),
            );
            let draw: Option<f64> = discharging
                .iter()
                .map(|b| b.power)
                .try_fold(0.0, |sum, power| Some(sum + power?));
            report.metrics.insert(
                metric_id("power.battery.draw"),
                match (discharging.is_empty(), draw) {
                    (true, _) => Observation::float(0.0),
                    (false, Some(draw)) => Observation::float(draw),
                    (false, None) => Observation::Unsupported(UnsupportedReason::NotReported {
                        detail: "the battery does not report its draw".into(),
                    }),
                },
            );
        }

        for battery in &batteries {
            let mut m = MetricSet::new();
            m.insert(
                metric_id("battery.charge"),
                or_not_reported(battery.charge, "its charge", Observation::float),
            );
            m.insert(
                metric_id("battery.status"),
                Observation::text(battery.status.unwrap_or(BatteryStatus::Unknown).as_str()),
            );
            for (id, value, what) in [
                ("battery.energy.now", battery.energy_now, "its energy"),
                (
                    "battery.energy.full",
                    battery.energy_full,
                    "its full capacity",
                ),
                (
                    "battery.energy.design",
                    battery.energy_design,
                    "its design capacity",
                ),
                ("battery.health", battery.health(), "its design capacity"),
                ("battery.power", battery.power, "its draw"),
                ("battery.voltage", battery.voltage, "its voltage"),
            ] {
                m.insert(
                    metric_id(id),
                    or_not_reported(value, what, Observation::float),
                );
            }
            let remaining = battery.time_remaining();
            let empty = remaining
                .filter(|(status, _)| *status == BatteryStatus::Discharging)
                .map(|(_, s)| s);
            let full = remaining
                .filter(|(status, _)| *status == BatteryStatus::Charging)
                .map(|(_, s)| s);
            if let Some(seconds) = empty {
                m.insert(
                    metric_id("battery.time.remaining"),
                    Observation::float(seconds),
                );
            }
            if let Some(seconds) = full {
                m.insert(
                    metric_id("battery.time.to_full"),
                    Observation::float(seconds),
                );
            }
            m.insert(
                metric_id("battery.cycles"),
                match battery.cycles {
                    Some(0) | None => Observation::Unsupported(UnsupportedReason::NotReported {
                        detail: "the firmware does not count charge cycles".into(),
                    }),
                    Some(cycles) => Observation::unsigned(cycles),
                },
            );
            if let Some(limit) = battery.charge_limit {
                m.insert(metric_id("battery.charge.limit"), Observation::float(limit));
            }
            for (id, value) in [
                ("battery.technology", &battery.technology),
                ("battery.model", &battery.model),
                ("battery.manufacturer", &battery.manufacturer),
            ] {
                if let Some(value) = value {
                    m.insert(metric_id(id), Observation::text(value.clone()));
                }
            }
            m.insert(
                metric_id("battery.peripheral"),
                Observation::boolean(battery.device_scope),
            );
            report.entities.push(Entity::new(
                EntityId::new(EntityKind::Battery, battery.name.clone()),
                m,
            ));
        }

        for source in &sources {
            let mut m = MetricSet::new();
            m.insert(
                metric_id("power_source.kind"),
                Observation::text(source.kind.clone()),
            );
            if let Some(online) = source.online {
                m.insert(
                    metric_id("power_source.online"),
                    Observation::boolean(online),
                );
            }
            if let Some(usb_type) = &source.usb_type {
                m.insert(
                    metric_id("power_source.usb_type"),
                    Observation::text(usb_type.clone()),
                );
            }
            if let Some(watts) = source.max_power {
                m.insert(
                    metric_id("power_source.power.max"),
                    Observation::float(watts),
                );
            }
            report.entities.push(Entity::new(
                EntityId::new(EntityKind::PowerSource, source.name.clone()),
                m,
            ));
        }
        report
    }
}

impl Collector for BatteryCollector {
    fn id(&self) -> CollectorId {
        collector_id(BATTERY_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        BatteryCollector::descriptors()
    }

    fn collect(&mut self, at: Timestamp) -> CollectorReport {
        let roots = self.roots.clone();
        self.sample(&roots, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_word_is_read_into_its_state() {
        assert_eq!(
            BatteryStatus::parse("Discharging\n"),
            BatteryStatus::Discharging
        );
        assert_eq!(
            BatteryStatus::parse("Not charging"),
            BatteryStatus::NotCharging
        );
        assert_eq!(BatteryStatus::parse("Weird"), BatteryStatus::Unknown);
    }

    #[test]
    fn the_bracketed_usb_type_is_the_negotiated_one() {
        assert_eq!(bracketed("C [PD] PD_PPS").as_deref(), Some("PD"));
        assert_eq!(bracketed("[C] PD PD_PPS").as_deref(), Some("C"));
        assert_eq!(bracketed("C PD"), None);
    }

    #[test]
    fn time_left_divides_energy_by_draw_and_charging_stops_at_the_limit() {
        let battery = BatteryReading {
            status: Some(BatteryStatus::Discharging),
            energy_now: Some(30.0),
            energy_full: Some(60.0),
            power: Some(10.0),
            ..BatteryReading::default()
        };
        assert_eq!(
            battery.time_remaining(),
            Some((BatteryStatus::Discharging, 3.0 * 3600.0))
        );
        let charging = BatteryReading {
            status: Some(BatteryStatus::Charging),
            charge_limit: Some(0.8),
            ..battery.clone()
        };
        // 80% of 60 Wh is 48 Wh; 18 Wh missing at 10 W is 1.8 h.
        let (_, seconds) = charging.time_remaining().unwrap();
        assert!((seconds - 1.8 * 3600.0).abs() < 1e-6);
        let idle = BatteryReading {
            power: Some(0.0),
            ..battery
        };
        assert_eq!(idle.time_remaining(), None);
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = BatteryCollector::descriptors();
        let mut ids: Vec<_> = descriptors.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), descriptors.len());
    }
}
