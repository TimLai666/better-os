//! Every hwmon sensor the kernel publishes.
//!
//! `/sys/class/hwmon/hwmonN` is one chip: a driver such as `k10temp`,
//! `amdgpu`, `nvme`, `asus`, or `acpi_fan`, named by its `name` file. Each
//! chip exposes inputs by kind and index (`temp1_input`, `fan1_input`,
//! `in0_input`, `curr1_input`, `power1_input`, `freq1_input`) with an
//! optional `_label`, in the fixed units the kernel's
//! `Documentation/hwmon/sysfs-interface.rst` defines: millidegrees, RPM,
//! millivolts, milliamperes, microwatts, and hertz. This module reads all of
//! them, converts them to plain SI units, and keeps the label the driver gave.
//!
//! The hwmon number is not stable across boots, so a reading is keyed by its
//! chip's name and its input (`k10temp/temp1`). Two chips with the same name
//! (two NVMe drives) are told apart by the device they belong to.
//!
//! Nothing here guesses. A chip the machine does not have is simply absent; an
//! input whose read fails is reported as unknown with the reason, never as 0.

use crate::catalog::{collector_id, gauge, identity, metric_id, sys_source};
use crate::fsread::{ReadError, list_dir, read_attribute};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::BTreeMap;
use std::path::Path;

pub const SENSORS_COLLECTOR: &str = "linux.sensors";

/// What an hwmon input measures.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SensorKind {
    Temperature,
    Fan,
    Voltage,
    Current,
    Power,
    Frequency,
}

impl SensorKind {
    /// The sysfs prefix of the input files.
    pub fn prefix(self) -> &'static str {
        match self {
            SensorKind::Temperature => "temp",
            SensorKind::Fan => "fan",
            SensorKind::Voltage => "in",
            SensorKind::Current => "curr",
            SensorKind::Power => "power",
            SensorKind::Frequency => "freq",
        }
    }

    /// The word stored in `sensor.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            SensorKind::Temperature => "temperature",
            SensorKind::Fan => "fan",
            SensorKind::Voltage => "voltage",
            SensorKind::Current => "current",
            SensorKind::Power => "power",
            SensorKind::Frequency => "frequency",
        }
    }

    /// The metric the value is stored under.
    pub fn metric(self) -> &'static str {
        match self {
            SensorKind::Temperature => "sensor.temperature",
            SensorKind::Fan => "sensor.fan.speed",
            SensorKind::Voltage => "sensor.voltage",
            SensorKind::Current => "sensor.current",
            SensorKind::Power => "sensor.power",
            SensorKind::Frequency => "sensor.frequency",
        }
    }

    /// What one raw unit is worth in the stored unit.
    fn scale(self) -> f64 {
        match self {
            SensorKind::Temperature => 1e-3,
            SensorKind::Fan => 1.0,
            SensorKind::Voltage => 1e-3,
            SensorKind::Current => 1e-3,
            SensorKind::Power => 1e-6,
            SensorKind::Frequency => 1.0,
        }
    }

    const ALL: [SensorKind; 6] = [
        SensorKind::Temperature,
        SensorKind::Fan,
        SensorKind::Voltage,
        SensorKind::Current,
        SensorKind::Power,
        SensorKind::Frequency,
    ];

    /// Which kind an attribute file name belongs to, and its index: `temp1_input`
    /// is `(Temperature, "1")`. Power is read from `_input` or, for drivers that
    /// only average, `_average`.
    fn parse_input(file_name: &str) -> Option<(SensorKind, &str, bool)> {
        for kind in SensorKind::ALL {
            let Some(rest) = file_name.strip_prefix(kind.prefix()) else {
                continue;
            };
            // `in` is also the start of nothing else we read, but `fan` and
            // `freq` must not swallow each other: require a digit next.
            if !rest.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            if let Some(index) = rest.strip_suffix("_input") {
                return Some((kind, index, false));
            }
            if kind == SensorKind::Power
                && let Some(index) = rest.strip_suffix("_average")
            {
                return Some((kind, index, true));
            }
        }
        None
    }
}

/// One hwmon input, read.
#[derive(Clone, Debug, PartialEq)]
pub struct SensorReading {
    /// The driver's name for the chip (`k10temp`).
    pub chip: String,
    /// The chip as this crate keys it: its name, or its name and device when
    /// two chips share a name.
    pub chip_key: String,
    /// The device the chip belongs to, as the last part of its `device` link
    /// (`0000:63:00.0`, `nvme0`, `asus-nb-wmi`).
    pub device: Option<String>,
    /// The input's own name (`temp1`).
    pub input: String,
    pub kind: SensorKind,
    /// The driver's label (`Tctl`, `edge`, `cpu_fan`), when it gave one.
    pub label: Option<String>,
    /// In degrees Celsius, RPM, volts, amperes, watts, or hertz. `Err` keeps
    /// why the input could not be read.
    pub value: Result<f64, ReadError>,
    /// Temperature thresholds the driver publishes, in degrees Celsius.
    pub high: Option<f64>,
    pub critical: Option<f64>,
}

impl SensorReading {
    /// The key a reading's entity carries.
    pub fn key(&self) -> String {
        format!("{}/{}", self.chip_key, self.input)
    }
}

/// A sysfs number, which for temperatures may be negative.
fn read_signed(path: &Path) -> Result<i64, ReadError> {
    let raw = read_attribute(path)?;
    raw.parse::<i64>().map_err(|_| ReadError::Failed {
        path: path.to_path_buf(),
        detail: format!("not an integer: {raw:?}"),
    })
}

/// A temperature threshold, in degrees Celsius. Drivers publish placeholders
/// such as 0 or 255 °C when a threshold is not set, and those are left out.
fn threshold(chip: &Path, input: &str, kind: SensorKind, suffix: &str) -> Option<f64> {
    if kind != SensorKind::Temperature {
        return None;
    }
    let raw = read_signed(&chip.join(format!("{input}_{suffix}"))).ok()?;
    Some(raw as f64 / 1000.0).filter(|celsius| *celsius > 0.0 && *celsius < 200.0)
}

/// The last component of a chip's `device` link.
fn device_name(chip: &Path) -> Option<String> {
    let target = std::fs::read_link(chip.join("device")).ok()?;
    target
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

/// Read every hwmon chip under the sys root.
///
/// `Err` only when `class/hwmon` itself cannot be listed; a chip or an input
/// that cannot be read is carried as a reading with its error, or skipped when
/// not even its name is readable.
pub fn scan_hwmon(roots: &Roots) -> Result<Vec<SensorReading>, ReadError> {
    let mut readings = Vec::new();
    for chip in list_chips(roots)? {
        readings.extend(read_chip(&chip));
    }
    sort_readings(&mut readings);
    Ok(readings)
}

/// One hwmon chip: where it is and what it is called.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub dir: std::path::PathBuf,
    pub name: String,
    pub key: String,
    pub device: Option<String>,
}

/// Every chip under `class/hwmon` with a readable name.
pub fn list_chips(roots: &Roots) -> Result<Vec<Chip>, ReadError> {
    let mut named: Vec<(String, Option<String>, std::path::PathBuf)> = Vec::new();
    for chip in list_dir(&roots.sys("class/hwmon"))? {
        let Ok(name) = read_attribute(&chip.join("name")) else {
            continue;
        };
        named.push((name, device_name(&chip), chip));
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (name, _, _) in &named {
        *counts.entry(name.clone()).or_default() += 1;
    }
    Ok(named
        .into_iter()
        .map(|(name, device, dir)| {
            let key = if counts.get(&name).copied().unwrap_or(0) > 1 {
                match &device {
                    Some(device) => format!("{name}@{device}"),
                    None => format!(
                        "{name}@{}",
                        dir.file_name().and_then(|n| n.to_str()).unwrap_or("hwmon")
                    ),
                }
            } else {
                name.clone()
            };
            Chip {
                dir,
                name,
                key,
                device,
            }
        })
        .collect())
}

/// Every input of one chip.
pub fn read_chip(chip: &Chip) -> Vec<SensorReading> {
    let Ok(attributes) = list_dir(&chip.dir) else {
        return Vec::new();
    };
    // Power can come as both `_input` and `_average`; the instant reading
    // wins when there are both.
    let mut chosen: BTreeMap<(SensorKind, String), (std::path::PathBuf, bool)> = BTreeMap::new();
    for attribute in &attributes {
        let Some(file_name) = attribute.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((kind, index, average)) = SensorKind::parse_input(file_name) else {
            continue;
        };
        let slot = (kind, index.to_string());
        let replace = match chosen.get(&slot) {
            None => true,
            Some((_, existing_average)) => *existing_average && !average,
        };
        if replace {
            chosen.insert(slot, (attribute.clone(), average));
        }
    }
    let mut readings = Vec::new();
    for ((kind, index), (path, _)) in chosen {
        let input = format!("{}{index}", kind.prefix());
        let label = read_attribute(&chip.dir.join(format!("{input}_label")))
            .ok()
            .map(|label| label.trim().to_string())
            .filter(|label| !label.is_empty());
        let value = read_signed(&path).map(|raw| raw as f64 * kind.scale());
        readings.push(SensorReading {
            chip: chip.name.clone(),
            chip_key: chip.key.clone(),
            device: chip.device.clone(),
            input: input.clone(),
            kind,
            label,
            value,
            high: threshold(&chip.dir, &input, kind, "max"),
            critical: threshold(&chip.dir, &input, kind, "crit"),
        });
    }
    readings
}

fn sort_readings(readings: &mut [SensorReading]) {
    readings.sort_by(|a, b| {
        a.chip_key
            .cmp(&b.chip_key)
            .then(a.kind.cmp(&b.kind))
            .then(natural(&a.input).cmp(&natural(&b.input)))
    });
}

/// A chip whose read took longer than this is read at most every
/// [`SLOW_CHIP_INTERVAL`]. Embedded-controller and ACPI chips (`acpitz`,
/// `acpi_fan`, a battery's hwmon) answer through firmware calls that take
/// milliseconds each, and a temperature or a fan speed does not need a fresh
/// firmware call every second.
pub const SLOW_CHIP: std::time::Duration = std::time::Duration::from_millis(1);
pub const SLOW_CHIP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether a chip read at `last` that took `cost` is due again at `now`.
pub fn chip_due(cost: std::time::Duration, last: Timestamp, now: Timestamp) -> bool {
    if cost < SLOW_CHIP {
        return true;
    }
    Timestamp::interval_seconds(last, now).is_none_or(|s| s >= SLOW_CHIP_INTERVAL.as_secs_f64())
}

/// `temp2` before `temp10`.
fn natural(input: &str) -> (String, u64) {
    let split = input
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(input.len());
    (
        input[..split].to_string(),
        input[split..].parse().unwrap_or(0),
    )
}

/// The last readings of a chip, when they were taken, and what reading it
/// cost.
struct ChipCache {
    readings: Vec<SensorReading>,
    at: Timestamp,
    cost: std::time::Duration,
}

/// Every hwmon reading, one entity each.
pub struct SensorsCollector {
    roots: Roots,
    cache: BTreeMap<std::path::PathBuf, ChipCache>,
}

impl SensorsCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            cache: BTreeMap::new(),
        }
    }

    /// Every reading, re-reading a slow chip only when it is due.
    fn read_all(&mut self, roots: &Roots, at: Timestamp) -> Result<Vec<SensorReading>, ReadError> {
        let chips = list_chips(roots)?;
        let mut readings = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for chip in chips {
            seen.insert(chip.dir.clone());
            let due = match self.cache.get(&chip.dir) {
                Some(cached) => chip_due(cached.cost, cached.at, at),
                None => true,
            };
            if due {
                let started = std::time::Instant::now();
                let fresh = read_chip(&chip);
                self.cache.insert(
                    chip.dir.clone(),
                    ChipCache {
                        readings: fresh,
                        at,
                        cost: started.elapsed(),
                    },
                );
            }
            if let Some(cached) = self.cache.get(&chip.dir) {
                readings.extend(cached.readings.iter().cloned());
            }
        }
        self.cache.retain(|dir, _| seen.contains(dir));
        sort_readings(&mut readings);
        Ok(readings)
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        let source = || sys_source("class/hwmon");
        vec![
            identity("sensor.chip", source(), "the hwmon driver's chip name"),
            identity(
                "sensor.label",
                source(),
                "the driver's label for the input, or the input's own name",
            ),
            identity(
                "sensor.kind",
                source(),
                "temperature, fan, voltage, current, power, or frequency",
            ),
            identity(
                "sensor.device",
                source(),
                "the device the chip belongs to, from its device link",
            ),
            gauge(
                "sensor.temperature",
                Unit::DegreesCelsius,
                source(),
                "tempN_input converted from millidegrees",
            ),
            gauge(
                "sensor.temperature.high",
                Unit::DegreesCelsius,
                source(),
                "tempN_max, the driver's high threshold",
            ),
            gauge(
                "sensor.temperature.critical",
                Unit::DegreesCelsius,
                source(),
                "tempN_crit, the driver's critical threshold",
            ),
            gauge(
                "sensor.fan.speed",
                Unit::RevolutionsPerMinute,
                source(),
                "fanN_input",
            ),
            gauge(
                "sensor.voltage",
                Unit::Volts,
                source(),
                "inN_input converted from millivolts",
            ),
            gauge(
                "sensor.current",
                Unit::Amperes,
                source(),
                "currN_input converted from milliamperes",
            ),
            gauge(
                "sensor.power",
                Unit::Watts,
                source(),
                "powerN_input or powerN_average converted from microwatts",
            ),
            gauge("sensor.frequency", Unit::Hertz, source(), "freqN_input"),
        ]
    }

    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(SENSORS_COLLECTOR), at);
        let readings = match self.read_all(roots, at) {
            Ok(readings) => readings,
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
        let mut failed = 0usize;
        for reading in readings {
            let mut metrics = MetricSet::new();
            metrics.insert(
                metric_id("sensor.chip"),
                Observation::text(reading.chip.clone()),
            );
            metrics.insert(
                metric_id("sensor.label"),
                Observation::text(
                    reading
                        .label
                        .clone()
                        .unwrap_or_else(|| reading.input.clone()),
                ),
            );
            metrics.insert(
                metric_id("sensor.kind"),
                Observation::text(reading.kind.as_str()),
            );
            if let Some(device) = &reading.device {
                metrics.insert(
                    metric_id("sensor.device"),
                    Observation::text(device.clone()),
                );
            }
            metrics.insert(
                metric_id(reading.kind.metric()),
                match &reading.value {
                    Ok(value) => Observation::float(*value),
                    Err(error) => {
                        failed += 1;
                        match error {
                            // A file that listed and then could not be read
                            // is a driver that has no reading right now.
                            ReadError::Missing { .. } => {
                                Observation::Unknown(UnknownReason::EntityDisappeared)
                            }
                            other => other.clone().into_observation(),
                        }
                    }
                },
            );
            if let Some(high) = reading.high {
                metrics.insert(
                    metric_id("sensor.temperature.high"),
                    Observation::float(high),
                );
            }
            if let Some(critical) = reading.critical {
                metrics.insert(
                    metric_id("sensor.temperature.critical"),
                    Observation::float(critical),
                );
            }
            report.entities.push(Entity::new(
                EntityId::new(EntityKind::Sensor, reading.key()),
                metrics,
            ));
        }
        if failed > 0 {
            report.health = CollectorHealth::Degraded {
                detail: format!("{failed} hwmon inputs could not be read"),
            };
        }
        report
    }
}

impl Collector for SensorsCollector {
    fn id(&self) -> CollectorId {
        collector_id(SENSORS_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        SensorsCollector::descriptors()
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
    fn input_files_are_recognised_by_kind_and_index() {
        assert_eq!(
            SensorKind::parse_input("temp1_input"),
            Some((SensorKind::Temperature, "1", false))
        );
        assert_eq!(
            SensorKind::parse_input("fan2_input"),
            Some((SensorKind::Fan, "2", false))
        );
        assert_eq!(
            SensorKind::parse_input("freq1_input"),
            Some((SensorKind::Frequency, "1", false))
        );
        assert_eq!(
            SensorKind::parse_input("power1_average"),
            Some((SensorKind::Power, "1", true))
        );
        for other in ["temp1_label", "temp1_crit", "name", "fan_input", "uevent"] {
            assert_eq!(SensorKind::parse_input(other), None, "{other}");
        }
    }

    #[test]
    fn a_slow_chip_waits_and_a_fast_one_does_not() {
        use std::time::Duration;
        let at = |s: u64| Timestamp {
            unix_ms: s * 1000,
            monotonic_ns: s * 1_000_000_000,
        };
        assert!(chip_due(Duration::from_micros(100), at(1), at(2)));
        assert!(!chip_due(Duration::from_millis(8), at(1), at(2)));
        assert!(chip_due(Duration::from_millis(8), at(1), at(6)));
    }

    #[test]
    fn inputs_sort_naturally() {
        assert!(natural("temp2") < natural("temp10"));
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = SensorsCollector::descriptors();
        let mut ids: Vec<_> = descriptors.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), descriptors.len());
        for kind in SensorKind::ALL {
            assert!(
                descriptors.iter().any(|d| d.id.as_str() == kind.metric()),
                "{kind:?}"
            );
        }
    }
}
