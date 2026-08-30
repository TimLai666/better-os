//! CPU time, load, frequency, and temperature.
//!
//! Upstream interface: `/proc/stat` and `/proc/loadavg`, documented in the
//! kernel's `Documentation/filesystems/proc.rst`; per-CPU frequency and
//! governor from `Documentation/admin-guide/pm/cpufreq.rst`; temperatures from
//! `Documentation/hwmon/sysfs-interface.rst`.
//!
//! Two semantics are easy to get wrong and are handled explicitly here.
//! First, the kernel folds guest time into the `user` field (and guest-nice
//! into `nice`), so reporting the raw field as user time double counts a
//! virtualised workload; the reported user category has guest subtracted, the
//! way procps does it. Second, `iowait` is not idle and is not busy, so the
//! derived `busy` category excludes both `idle` and `iowait` instead of
//! treating waiting on I/O as work done.

use crate::catalog::{
    MINIMUM_DELTA_INTERVAL, collector_id, derived_source, gauge, identity, metric_id, proc_source,
    rate, saturation, sys_source, utilization,
};
use crate::fsread::{
    MalformedInput, ReadError, field_f64, field_u64, list_dir, read_attribute, read_text,
    read_u64_attribute,
};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::BTreeMap;

/// The `/proc` ABI expresses CPU time in USER_HZ, which the kernel fixes at
/// 100 for userspace regardless of the configured internal `CONFIG_HZ`.
pub const USER_HZ: u64 = 100;

/// The ten CPU time counters of one `/proc/stat` `cpu` line, in USER_HZ ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    pub guest: u64,
    pub guest_nice: u64,
}

impl CpuTimes {
    /// Parse the counters that follow the `cpu` label.
    ///
    /// Kernels before 2.6.11 stop at `iowait` and there is no guarantee a
    /// future kernel will not append an eleventh counter, so anything past
    /// `guest_nice` is ignored and anything missing after `idle` reads as
    /// zero. Fewer than four counters is malformed.
    pub fn parse(context: &'static str, fields: &[&str]) -> Result<Self, MalformedInput> {
        if fields.len() < 4 {
            return Err(MalformedInput::new(
                context,
                format!(
                    "a cpu line needs at least four counters, got {}",
                    fields.len()
                ),
            ));
        }
        let at = |index: usize| -> Result<u64, MalformedInput> {
            if index < fields.len() {
                field_u64(context, fields, index)
            } else {
                Ok(0)
            }
        };
        Ok(Self {
            user: at(0)?,
            nice: at(1)?,
            system: at(2)?,
            idle: at(3)?,
            iowait: at(4)?,
            irq: at(5)?,
            softirq: at(6)?,
            steal: at(7)?,
            guest: at(8)?,
            guest_nice: at(9)?,
        })
    }

    /// User time with guest time removed, because the kernel counts a guest
    /// tick in both places.
    pub fn user_excluding_guest(&self) -> u64 {
        self.user.saturating_sub(self.guest)
    }

    pub fn nice_excluding_guest(&self) -> u64 {
        self.nice.saturating_sub(self.guest_nice)
    }

    pub fn guest_total(&self) -> u64 {
        self.guest.saturating_add(self.guest_nice)
    }

    /// All time accounted for. `guest` and `guest_nice` are deliberately not
    /// added: they are already inside `user` and `nice`.
    pub fn total(&self) -> u64 {
        self.user
            .saturating_add(self.nice)
            .saturating_add(self.system)
            .saturating_add(self.idle)
            .saturating_add(self.iowait)
            .saturating_add(self.irq)
            .saturating_add(self.softirq)
            .saturating_add(self.steal)
    }

    /// Time doing work. Waiting on I/O is not work, so `iowait` is excluded
    /// along with `idle`.
    pub fn busy(&self) -> u64 {
        self.total()
            .saturating_sub(self.idle)
            .saturating_sub(self.iowait)
    }

    /// The difference from an earlier read. Counters can appear to go
    /// backwards when a CPU is hot-unplugged and replugged, so the difference
    /// saturates at zero rather than wrapping into a huge spike.
    pub fn since(&self, earlier: &CpuTimes) -> CpuTimes {
        CpuTimes {
            user: self.user.saturating_sub(earlier.user),
            nice: self.nice.saturating_sub(earlier.nice),
            system: self.system.saturating_sub(earlier.system),
            idle: self.idle.saturating_sub(earlier.idle),
            iowait: self.iowait.saturating_sub(earlier.iowait),
            irq: self.irq.saturating_sub(earlier.irq),
            softirq: self.softirq.saturating_sub(earlier.softirq),
            steal: self.steal.saturating_sub(earlier.steal),
            guest: self.guest.saturating_sub(earlier.guest),
            guest_nice: self.guest_nice.saturating_sub(earlier.guest_nice),
        }
    }
}

/// Everything one read of `/proc/stat` produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcStat {
    pub total: CpuTimes,
    /// `(logical cpu number, counters)`, in the order the kernel listed them.
    pub per_cpu: Vec<(u32, CpuTimes)>,
    pub context_switches: Option<u64>,
    pub interrupts: Option<u64>,
    pub soft_interrupts: Option<u64>,
    pub boot_time_unix_s: Option<u64>,
    pub processes_created: Option<u64>,
    pub procs_running: Option<u64>,
    pub procs_blocked: Option<u64>,
}

const PROC_STAT: &str = "/proc/stat";

pub fn parse_proc_stat(input: &str) -> Result<ProcStat, MalformedInput> {
    let mut parsed = ProcStat::default();
    let mut saw_total = false;
    for line in input.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(label) = fields.first() else {
            continue;
        };
        let rest = &fields[1..];
        match *label {
            "cpu" => {
                parsed.total = CpuTimes::parse(PROC_STAT, rest)?;
                saw_total = true;
            }
            "ctxt" => parsed.context_switches = Some(field_u64(PROC_STAT, rest, 0)?),
            "btime" => parsed.boot_time_unix_s = Some(field_u64(PROC_STAT, rest, 0)?),
            "processes" => parsed.processes_created = Some(field_u64(PROC_STAT, rest, 0)?),
            "procs_running" => parsed.procs_running = Some(field_u64(PROC_STAT, rest, 0)?),
            "procs_blocked" => parsed.procs_blocked = Some(field_u64(PROC_STAT, rest, 0)?),
            "intr" => parsed.interrupts = Some(field_u64(PROC_STAT, rest, 0)?),
            "softirq" => parsed.soft_interrupts = Some(field_u64(PROC_STAT, rest, 0)?),
            other => {
                if let Some(number) = other.strip_prefix("cpu") {
                    let index = number.parse::<u32>().map_err(|_| {
                        MalformedInput::new(PROC_STAT, format!("bad cpu index {other:?}"))
                    })?;
                    parsed
                        .per_cpu
                        .push((index, CpuTimes::parse(PROC_STAT, rest)?));
                }
            }
        }
    }
    if !saw_total {
        return Err(MalformedInput::new(PROC_STAT, "no aggregate cpu line"));
    }
    Ok(parsed)
}

/// `/proc/loadavg`: three exponentially damped run-queue averages, the
/// runnable and total task counts, and the last PID allocated.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadAverage {
    pub one_minute: f64,
    pub five_minute: f64,
    pub fifteen_minute: f64,
    pub runnable: u64,
    pub total_tasks: u64,
}

const PROC_LOADAVG: &str = "/proc/loadavg";

pub fn parse_loadavg(input: &str) -> Result<LoadAverage, MalformedInput> {
    let fields: Vec<&str> = input.split_whitespace().collect();
    let entity = fields
        .get(3)
        .ok_or_else(|| MalformedInput::new(PROC_LOADAVG, "missing runnable/total field"))?;
    let (runnable, total) = entity
        .split_once('/')
        .ok_or_else(|| MalformedInput::new(PROC_LOADAVG, format!("bad task field {entity:?}")))?;
    Ok(LoadAverage {
        one_minute: field_f64(PROC_LOADAVG, &fields, 0)?,
        five_minute: field_f64(PROC_LOADAVG, &fields, 1)?,
        fifteen_minute: field_f64(PROC_LOADAVG, &fields, 2)?,
        runnable: runnable.parse().map_err(|_| {
            MalformedInput::new(PROC_LOADAVG, format!("bad runnable count {runnable:?}"))
        })?,
        total_tasks: total
            .parse()
            .map_err(|_| MalformedInput::new(PROC_LOADAVG, format!("bad task count {total:?}")))?,
    })
}

/// CPU temperatures found under `/sys/class/hwmon`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuTemperatures {
    /// Degrees Celsius keyed by the `core_id` a `coretemp` `Core N` label
    /// names. Only Intel's `coretemp` driver publishes these; AMD's `k10temp`
    /// publishes a package sensor only.
    pub per_core: BTreeMap<u64, f64>,
    /// The package or die sensor, where one is published.
    pub package: Option<f64>,
    /// Why `per_core` is empty, in the driver's own terms, so an unsupported
    /// reading can say something better than "no data".
    pub detail: String,
}

/// Drivers that publish a CPU package or die temperature.
const CPU_HWMON_DRIVERS: [&str; 3] = ["coretemp", "k10temp", "zenpower"];

/// Labels that name a whole package or die rather than one core.
const PACKAGE_LABELS: [&str; 4] = ["package id 0", "tctl", "tdie", "tccd1"];

/// Scan `/sys/class/hwmon` for CPU temperature sensors.
///
/// Known limitation: `Core N` labels are matched against `core_id` without
/// resolving which package the hwmon device belongs to, so a multi-socket
/// machine with repeated core numbering would collide. Single-socket desktops
/// and laptops are the supported target; multi-socket needs the package
/// resolution that ticket 23 can add once there is hardware to test it on.
pub fn scan_cpu_temperatures(roots: &Roots) -> Result<CpuTemperatures, ReadError> {
    let hwmon_root = roots.sys("class/hwmon");
    let devices = list_dir(&hwmon_root)?;
    let mut found = CpuTemperatures {
        detail: "no coretemp, k10temp, or zenpower hwmon device".to_string(),
        ..CpuTemperatures::default()
    };
    for device in devices {
        let Ok(name) = read_attribute(&device.join("name")) else {
            continue;
        };
        if !CPU_HWMON_DRIVERS.contains(&name.as_str()) {
            continue;
        }
        found.detail = format!("{name} publishes no per-core label");
        let Ok(attributes) = list_dir(&device) else {
            continue;
        };
        for attribute in attributes {
            let Some(file_name) = attribute.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(index) = file_name
                .strip_prefix("temp")
                .and_then(|rest| rest.strip_suffix("_input"))
            else {
                continue;
            };
            let Ok(millidegrees) = read_u64_attribute(&attribute) else {
                continue;
            };
            let celsius = millidegrees as f64 / 1000.0;
            let label = read_attribute(&device.join(format!("temp{index}_label")))
                .unwrap_or_default()
                .to_ascii_lowercase();
            if let Some(core) = label.strip_prefix("core ") {
                if let Ok(core_id) = core.trim().parse::<u64>() {
                    found.per_core.insert(core_id, celsius);
                    found.detail = format!("{name} publishes per-core labels");
                }
            } else if PACKAGE_LABELS.contains(&label.as_str())
                || (label.is_empty() && found.package.is_none())
            {
                found.package = Some(celsius);
            }
        }
    }
    Ok(found)
}

const CPU_COLLECTOR: &str = "linux.cpu";

/// The eight kernel CPU time categories plus the two derived ones, as
/// `(metric suffix, extractor)`.
type CategoryExtractor = fn(&CpuTimes) -> u64;
const CATEGORIES: [(&str, CategoryExtractor); 10] = [
    ("user", |times| times.user_excluding_guest()),
    ("nice", |times| times.nice_excluding_guest()),
    ("system", |times| times.system),
    ("idle", |times| times.idle),
    ("iowait", |times| times.iowait),
    ("irq", |times| times.irq),
    ("softirq", |times| times.softirq),
    ("steal", |times| times.steal),
    ("guest", |times| times.guest_total()),
    ("busy", |times| times.busy()),
];

fn utilization_metric(category: &str) -> String {
    format!("cpu.utilization.{category}")
}

/// Turn a pair of reads into the ten utilization ratios.
fn utilization_set(
    earlier: &CpuTimes,
    later: &CpuTimes,
    interval_long_enough: bool,
    set: &mut MetricSet,
) {
    let delta = later.since(earlier);
    let total = delta.total();
    for (category, extract) in CATEGORIES {
        let id = metric_id(&utilization_metric(category));
        let observation = if !interval_long_enough || total == 0 {
            Observation::Unknown(UnknownReason::IntervalTooShort)
        } else {
            Observation::float(extract(&delta) as f64 / total as f64)
        };
        set.insert(id, observation);
    }
}

fn not_yet_sampled(set: &mut MetricSet) {
    for (category, _) in CATEGORIES {
        set.insert(
            metric_id(&utilization_metric(category)),
            Observation::Unknown(UnknownReason::NotYetSampled),
        );
    }
}

/// A rate derived from two reads of a monotonic counter.
fn counter_rate(earlier: Option<u64>, later: Option<u64>, seconds: Option<f64>) -> Observation {
    match (earlier, later, seconds) {
        (Some(earlier), Some(later), Some(seconds)) => {
            Observation::float(later.saturating_sub(earlier) as f64 / seconds)
        }
        (_, None, _) => Observation::Unsupported(UnsupportedReason::NotReported {
            detail: "counter absent from /proc/stat on this kernel".into(),
        }),
        _ => Observation::Unknown(UnknownReason::NotYetSampled),
    }
}

struct CpuSnapshot {
    at: Timestamp,
    stat: ProcStat,
}

/// CPU utilization, load, frequency, and temperature.
pub struct CpuCollector {
    roots: Roots,
    previous: Option<CpuSnapshot>,
}

impl CpuCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            previous: None,
        }
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        let mut descriptors = Vec::new();
        for (category, _) in CATEGORIES {
            descriptors.push(utilization(
                &utilization_metric(category),
                derived_source("/proc/stat cpu time deltas"),
                "fraction of CPU time in this category over the sampling interval",
            ));
        }
        descriptors.extend([
            saturation(
                "cpu.load.average.1m",
                Unit::Count,
                proc_source("loadavg"),
                "one-minute run-queue average as the kernel reports it",
            ),
            saturation(
                "cpu.load.average.5m",
                Unit::Count,
                proc_source("loadavg"),
                "five-minute run-queue average",
            ),
            saturation(
                "cpu.load.average.15m",
                Unit::Count,
                proc_source("loadavg"),
                "fifteen-minute run-queue average",
            ),
            saturation(
                "cpu.tasks.runnable",
                Unit::Count,
                proc_source("loadavg"),
                "tasks currently runnable",
            ),
            gauge(
                "cpu.tasks.total",
                Unit::Count,
                proc_source("loadavg"),
                "tasks the scheduler knows about",
            ),
            saturation(
                "cpu.tasks.blocked",
                Unit::Count,
                proc_source("stat"),
                "tasks blocked waiting on I/O",
            ),
            rate(
                "cpu.context_switches.rate",
                Unit::CountPerSecond,
                proc_source("stat"),
                "context switches per second, from the ctxt counter",
            ),
            rate(
                "cpu.interrupts.rate",
                Unit::CountPerSecond,
                proc_source("stat"),
                "hardware interrupts per second, from the intr total",
            ),
            rate(
                "cpu.soft_interrupts.rate",
                Unit::CountPerSecond,
                proc_source("stat"),
                "soft interrupts per second, from the softirq total",
            ),
            rate(
                "cpu.processes.created.rate",
                Unit::CountPerSecond,
                proc_source("stat"),
                "forks per second, from the processes counter",
            ),
            gauge(
                "cpu.logical.count",
                Unit::Count,
                proc_source("stat"),
                "logical CPUs the kernel lists in /proc/stat",
            ),
            gauge(
                "cpu.boot_time",
                Unit::Seconds,
                proc_source("stat"),
                "boot time as a Unix timestamp, from btime",
            ),
            gauge(
                "cpu.package.temperature",
                Unit::DegreesCelsius,
                sys_source("class/hwmon"),
                "package or die temperature where a CPU hwmon driver publishes one",
            ),
            gauge(
                "cpu.temperature",
                Unit::DegreesCelsius,
                sys_source("class/hwmon"),
                "per-core temperature where the hwmon driver publishes Core N labels",
            ),
            gauge(
                "cpu.frequency.current",
                Unit::Hertz,
                sys_source("devices/system/cpu/cpuN/cpufreq/scaling_cur_freq"),
                "current clock, converted from the kilohertz cpufreq reports",
            ),
            gauge(
                "cpu.frequency.min",
                Unit::Hertz,
                sys_source("devices/system/cpu/cpuN/cpufreq/scaling_min_freq"),
                "lowest clock the current policy allows",
            ),
            gauge(
                "cpu.frequency.max",
                Unit::Hertz,
                sys_source("devices/system/cpu/cpuN/cpufreq/scaling_max_freq"),
                "highest clock the current policy allows",
            ),
            identity(
                "cpu.governor",
                sys_source("devices/system/cpu/cpuN/cpufreq/scaling_governor"),
                "active cpufreq governor",
            ),
            gauge(
                "cpu.core.id",
                Unit::Count,
                sys_source("devices/system/cpu/cpuN/topology/core_id"),
                "physical core this logical CPU belongs to",
            ),
        ]);
        descriptors
    }

    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(CPU_COLLECTOR), at);
        let stat_path = roots.proc("stat");
        let stat = match read_text(&stat_path) {
            Ok(raw) => match parse_proc_stat(&raw) {
                Ok(stat) => stat,
                Err(error) => {
                    report.health = CollectorHealth::Failed {
                        detail: format!("{}: {}", error.context, error.detail),
                    };
                    return report;
                }
            },
            Err(error) => {
                report.health = match &error {
                    ReadError::Missing { path } => {
                        CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
                            path: path.display().to_string(),
                        })
                    }
                    other => CollectorHealth::Failed {
                        detail: format!("{}", other.path().display()),
                    },
                };
                return report;
            }
        };

        let seconds = self
            .previous
            .as_ref()
            .and_then(|previous| Timestamp::interval_seconds(previous.at, at));
        let interval_long_enough =
            seconds.is_some_and(|seconds| seconds >= MINIMUM_DELTA_INTERVAL.as_secs_f64());

        match self.previous.as_ref() {
            Some(previous) => utilization_set(
                &previous.stat.total,
                &stat.total,
                interval_long_enough,
                &mut report.metrics,
            ),
            None => not_yet_sampled(&mut report.metrics),
        }

        let previous_stat = self.previous.as_ref().map(|previous| &previous.stat);
        for (id, earlier, later) in [
            (
                "cpu.context_switches.rate",
                previous_stat.and_then(|stat| stat.context_switches),
                stat.context_switches,
            ),
            (
                "cpu.interrupts.rate",
                previous_stat.and_then(|stat| stat.interrupts),
                stat.interrupts,
            ),
            (
                "cpu.soft_interrupts.rate",
                previous_stat.and_then(|stat| stat.soft_interrupts),
                stat.soft_interrupts,
            ),
            (
                "cpu.processes.created.rate",
                previous_stat.and_then(|stat| stat.processes_created),
                stat.processes_created,
            ),
        ] {
            let seconds = if interval_long_enough { seconds } else { None };
            report
                .metrics
                .insert(metric_id(id), counter_rate(earlier, later, seconds));
        }

        report.metrics.insert(
            metric_id("cpu.logical.count"),
            Observation::unsigned(stat.per_cpu.len() as u64),
        );
        report.metrics.insert(
            metric_id("cpu.boot_time"),
            match stat.boot_time_unix_s {
                Some(value) => Observation::unsigned(value),
                None => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "btime absent from /proc/stat".into(),
                }),
            },
        );
        report.metrics.insert(
            metric_id("cpu.tasks.blocked"),
            match stat.procs_blocked {
                Some(value) => Observation::unsigned(value),
                None => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "procs_blocked absent from /proc/stat".into(),
                }),
            },
        );

        self.read_load_average(roots, &mut report);
        let temperatures = scan_cpu_temperatures(roots);
        report.metrics.insert(
            metric_id("cpu.package.temperature"),
            match &temperatures {
                Ok(found) => match found.package {
                    Some(celsius) => Observation::float(celsius),
                    None => Observation::Unsupported(UnsupportedReason::NotReported {
                        detail: found.detail.clone(),
                    }),
                },
                Err(error) => error.clone().into_observation(),
            },
        );

        let previous_per_cpu: BTreeMap<u32, CpuTimes> = self
            .previous
            .as_ref()
            .map(|previous| previous.stat.per_cpu.iter().copied().collect())
            .unwrap_or_default();
        for (index, times) in &stat.per_cpu {
            let mut metrics = MetricSet::new();
            match previous_per_cpu.get(index) {
                Some(earlier) => {
                    utilization_set(earlier, times, interval_long_enough, &mut metrics)
                }
                None => not_yet_sampled(&mut metrics),
            }
            self.read_cpu_sysfs(roots, *index, &temperatures, &mut metrics);
            report.entities.push(Entity::new(
                EntityId::new(EntityKind::LogicalCpu, index.to_string()),
                metrics,
            ));
        }

        self.previous = Some(CpuSnapshot { at, stat });
        report
    }

    fn read_load_average(&self, roots: &Roots, report: &mut CollectorReport) {
        let path = roots.proc("loadavg");
        let ids = [
            "cpu.load.average.1m",
            "cpu.load.average.5m",
            "cpu.load.average.15m",
            "cpu.tasks.runnable",
            "cpu.tasks.total",
        ];
        let observations = match read_text(&path) {
            Ok(raw) => match parse_loadavg(&raw) {
                Ok(load) => [
                    Observation::float(load.one_minute),
                    Observation::float(load.five_minute),
                    Observation::float(load.fifteen_minute),
                    Observation::unsigned(load.runnable),
                    Observation::unsigned(load.total_tasks),
                ],
                Err(error) => std::array::from_fn(|_| error.clone().into_observation()),
            },
            Err(error) => std::array::from_fn(|_| error.clone().into_observation()),
        };
        for (id, observation) in ids.iter().zip(observations) {
            report.metrics.insert(metric_id(id), observation);
        }
    }

    fn read_cpu_sysfs(
        &self,
        roots: &Roots,
        index: u32,
        temperatures: &Result<CpuTemperatures, ReadError>,
        metrics: &mut MetricSet,
    ) {
        let base = format!("devices/system/cpu/cpu{index}");
        for (id, attribute) in [
            ("cpu.frequency.current", "scaling_cur_freq"),
            ("cpu.frequency.min", "scaling_min_freq"),
            ("cpu.frequency.max", "scaling_max_freq"),
        ] {
            let path = roots.sys(&format!("{base}/cpufreq/{attribute}"));
            let observation = match read_u64_attribute(&path) {
                // cpufreq reports kilohertz; the descriptor promises hertz.
                Ok(kilohertz) => Observation::unsigned(kilohertz.saturating_mul(1_000)),
                Err(error) => error.into_observation(),
            };
            metrics.insert(metric_id(id), observation);
        }
        let governor = roots.sys(&format!("{base}/cpufreq/scaling_governor"));
        metrics.insert(
            metric_id("cpu.governor"),
            match read_attribute(&governor) {
                Ok(value) => Observation::text(value),
                Err(error) => error.into_observation(),
            },
        );

        let core_path = roots.sys(&format!("{base}/topology/core_id"));
        let core_id = read_u64_attribute(&core_path);
        metrics.insert(
            metric_id("cpu.core.id"),
            match &core_id {
                Ok(value) => Observation::unsigned(*value),
                Err(error) => error.clone().into_observation(),
            },
        );

        let temperature = match (temperatures, &core_id) {
            (Ok(found), Ok(core)) => match found.per_core.get(core) {
                Some(celsius) => Observation::float(*celsius),
                None => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: found.detail.clone(),
                }),
            },
            (Ok(found), Err(_)) => Observation::Unsupported(UnsupportedReason::NotReported {
                detail: format!("{}; no topology/core_id to match against", found.detail),
            }),
            (Err(error), _) => error.clone().into_observation(),
        };
        metrics.insert(metric_id("cpu.temperature"), temperature);
    }
}

impl Collector for CpuCollector {
    fn id(&self) -> CollectorId {
        collector_id(CPU_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        CpuCollector::descriptors()
    }

    fn collect(&mut self, at: Timestamp) -> CollectorReport {
        let roots = self.roots.clone();
        self.sample(&roots, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::ObservationState;

    const REAL_STAT: &str = concat!(
        "cpu  61655 1278 22094 3589305 2730 0 1691 0 0 0\n",
        "cpu0 7489 269 2102 142368 262 0 223 0 0 0\n",
        "cpu1 3458 14 1273 147950 215 0 221 0 0 0\n",
        "intr 9732061 406 540830\n",
        "ctxt 38383729\n",
        "btime 1788063833\n",
        "processes 45636\n",
        "procs_running 1\n",
        "procs_blocked 0\n",
        "softirq 9732061 406 540830\n",
    );

    #[test]
    fn parses_the_aggregate_and_per_cpu_lines_of_a_real_proc_stat() {
        let stat = parse_proc_stat(REAL_STAT).unwrap();
        assert_eq!(stat.total.user, 61655);
        assert_eq!(stat.total.idle, 3_589_305);
        assert_eq!(stat.total.softirq, 1691);
        assert_eq!(stat.per_cpu.len(), 2);
        assert_eq!(stat.per_cpu[0].0, 0);
        assert_eq!(stat.per_cpu[1].1.user, 3458);
        assert_eq!(stat.context_switches, Some(38_383_729));
        assert_eq!(stat.boot_time_unix_s, Some(1_788_063_833));
        assert_eq!(stat.interrupts, Some(9_732_061));
        assert_eq!(stat.procs_blocked, Some(0));
    }

    #[test]
    fn guest_time_is_not_counted_twice_because_the_kernel_folds_it_into_user() {
        // 100 user ticks of which 40 were spent running a guest.
        let times = CpuTimes {
            user: 100,
            guest: 40,
            idle: 900,
            ..CpuTimes::default()
        };
        assert_eq!(times.user_excluding_guest(), 60);
        assert_eq!(times.guest_total(), 40);
        // Adding guest to the total would make it 1040 and understate every
        // ratio.
        assert_eq!(times.total(), 1000);
    }

    #[test]
    fn waiting_on_io_is_neither_idle_nor_busy() {
        let times = CpuTimes {
            user: 100,
            system: 100,
            idle: 700,
            iowait: 100,
            ..CpuTimes::default()
        };
        assert_eq!(times.total(), 1000);
        assert_eq!(times.busy(), 200);
    }

    #[test]
    fn a_counter_that_went_backwards_saturates_instead_of_wrapping() {
        let later = CpuTimes {
            user: 10,
            ..CpuTimes::default()
        };
        let earlier = CpuTimes {
            user: 50,
            ..CpuTimes::default()
        };
        assert_eq!(later.since(&earlier).user, 0);
    }

    #[test]
    fn an_old_kernel_that_stops_at_idle_still_parses() {
        let stat = parse_proc_stat("cpu  10 20 30 40\n").unwrap();
        assert_eq!(stat.total.idle, 40);
        assert_eq!(stat.total.steal, 0);
        assert_eq!(stat.total.total(), 100);
    }

    #[test]
    fn a_truncated_cpu_line_is_malformed_rather_than_zero() {
        let error = parse_proc_stat("cpu  10 20\n").unwrap_err();
        assert_eq!(error.context, "/proc/stat");
        assert!(error.detail.contains("at least four"));
    }

    #[test]
    fn a_stat_file_with_no_aggregate_line_is_malformed() {
        let error = parse_proc_stat("ctxt 5\nbtime 7\n").unwrap_err();
        assert!(error.detail.contains("no aggregate"));
    }

    #[test]
    fn a_non_numeric_counter_is_malformed() {
        let error = parse_proc_stat("cpu  sixty 1 2 3\n").unwrap_err();
        assert!(error.detail.contains("not a number"));
    }

    #[test]
    fn parses_a_load_average_line() {
        let load = parse_loadavg("0.90 0.83 0.81 1/1359 45614\n").unwrap();
        assert_eq!(load.one_minute, 0.90);
        assert_eq!(load.fifteen_minute, 0.81);
        assert_eq!(load.runnable, 1);
        assert_eq!(load.total_tasks, 1359);
    }

    #[test]
    fn a_truncated_load_average_is_malformed() {
        let error = parse_loadavg("0.90 0.83\n").unwrap_err();
        assert!(error.detail.contains("missing runnable"));
    }

    #[test]
    fn a_load_average_without_a_slash_is_malformed() {
        let error = parse_loadavg("0.90 0.83 0.81 1359 45614\n").unwrap_err();
        assert!(error.detail.contains("bad task field"));
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = CpuCollector::descriptors();
        let mut seen = std::collections::BTreeSet::new();
        for descriptor in &descriptors {
            assert!(
                seen.insert(descriptor.id.clone()),
                "duplicate metric {}",
                descriptor.id
            );
            assert!(!descriptor.summary.is_empty());
        }
        assert_eq!(seen.len(), descriptors.len());
    }

    #[test]
    fn the_first_round_reports_unknown_utilization_rather_than_zero() {
        let roots = Roots::at(crate::test_support::fixture("synthetic-a"));
        let mut collector = CpuCollector::new(roots.clone());
        let report = collector.sample(&roots, crate::test_support::at(0));
        assert_eq!(
            report.metrics.state_of(&metric_id("cpu.utilization.busy")),
            ObservationState::Unknown
        );
        assert_eq!(
            report
                .metrics
                .state_of(&metric_id("cpu.context_switches.rate")),
            ObservationState::Unknown
        );
    }

    #[test]
    fn two_samples_produce_the_utilization_the_tick_deltas_imply() {
        // Synthetic sample B adds 60 user, 20 system, 10 idle, 10 iowait
        // ticks: 100 in total, of which 80 are busy.
        let a = Roots::at(crate::test_support::fixture("synthetic-a"));
        let b = Roots::at(crate::test_support::fixture("synthetic-b"));
        let mut collector = CpuCollector::new(a.clone());
        collector.sample(&a, crate::test_support::at(0));
        let report = collector.sample(&b, crate::test_support::at(1_000));

        let busy = report
            .metrics
            .get(&metric_id("cpu.utilization.busy"))
            .unwrap();
        assert!((busy.as_f64().unwrap() - 0.80).abs() < 1e-9);
        let user = report
            .metrics
            .get(&metric_id("cpu.utilization.user"))
            .unwrap();
        assert!((user.as_f64().unwrap() - 0.60).abs() < 1e-9);
        let iowait = report
            .metrics
            .get(&metric_id("cpu.utilization.iowait"))
            .unwrap();
        assert!((iowait.as_f64().unwrap() - 0.10).abs() < 1e-9);
        // ctxt went from 20000 to 22000 over one second.
        let switches = report
            .metrics
            .get(&metric_id("cpu.context_switches.rate"))
            .unwrap();
        assert!((switches.as_f64().unwrap() - 2000.0).abs() < 1e-6);
    }

    #[test]
    fn two_samples_taken_too_close_together_refuse_to_answer() {
        let a = Roots::at(crate::test_support::fixture("synthetic-a"));
        let b = Roots::at(crate::test_support::fixture("synthetic-b"));
        let mut collector = CpuCollector::new(a.clone());
        collector.sample(&a, crate::test_support::at(0));
        let report = collector.sample(&b, crate::test_support::at(10));
        assert_eq!(
            report.metrics.get(&metric_id("cpu.utilization.busy")),
            Some(&Observation::Unknown(UnknownReason::IntervalTooShort))
        );
    }

    #[test]
    fn per_logical_cpu_utilization_is_reported_for_every_cpu_line() {
        let a = Roots::at(crate::test_support::fixture("synthetic-a"));
        let b = Roots::at(crate::test_support::fixture("synthetic-b"));
        let mut collector = CpuCollector::new(a.clone());
        collector.sample(&a, crate::test_support::at(0));
        let report = collector.sample(&b, crate::test_support::at(1_000));
        let cpus: Vec<_> = report.entities_of(EntityKind::LogicalCpu).collect();
        assert_eq!(cpus.len(), 2);
        for cpu in cpus {
            let busy = cpu.metrics.get(&metric_id("cpu.utilization.busy")).unwrap();
            assert!((busy.as_f64().unwrap() - 0.80).abs() < 1e-9);
        }
    }

    #[test]
    fn a_malformed_stat_file_fails_the_collector_instead_of_reporting_zeroes() {
        let roots = Roots::at(crate::test_support::fixture("malformed"));
        let mut collector = CpuCollector::new(roots.clone());
        let report = collector.sample(&roots, crate::test_support::at(0));
        assert!(matches!(report.health, CollectorHealth::Failed { .. }));
        assert!(report.metrics.is_empty());
    }

    #[test]
    fn a_truncated_stat_file_fails_the_collector() {
        let roots = Roots::at(crate::test_support::fixture("truncated"));
        let mut collector = CpuCollector::new(roots.clone());
        let report = collector.sample(&roots, crate::test_support::at(0));
        assert!(matches!(report.health, CollectorHealth::Failed { .. }));
    }

    #[test]
    fn a_host_without_proc_reports_the_subsystem_unsupported() {
        let roots = Roots::at(crate::test_support::fixture("does-not-exist"));
        let mut collector = CpuCollector::new(roots.clone());
        let report = collector.sample(&roots, crate::test_support::at(0));
        assert!(matches!(report.health, CollectorHealth::Unsupported(_)));
    }

    #[test]
    fn an_amd_host_reports_a_package_sensor_and_no_per_core_temperature() {
        // The captured host runs k10temp, which publishes Tctl and no Core N
        // labels. Per-core temperature must say why it is missing, not zero.
        let roots = Roots::at(crate::test_support::fixture("snapshot-a"));
        let temperatures = scan_cpu_temperatures(&roots).unwrap();
        assert!(temperatures.per_core.is_empty());
        assert!(temperatures.package.is_some());
        assert!(temperatures.detail.contains("k10temp"));

        let mut collector = CpuCollector::new(roots.clone());
        let report = collector.sample(&roots, crate::test_support::at(0));
        assert_eq!(
            report
                .metrics
                .state_of(&metric_id("cpu.package.temperature")),
            ObservationState::Value
        );
        let cpu0 = report
            .entities_of(EntityKind::LogicalCpu)
            .next()
            .expect("a logical cpu");
        assert_eq!(
            cpu0.metrics.state_of(&metric_id("cpu.temperature")),
            ObservationState::Unsupported
        );
    }

    #[test]
    fn cpufreq_kilohertz_are_converted_to_hertz() {
        let roots = Roots::at(crate::test_support::fixture("snapshot-a"));
        let mut collector = CpuCollector::new(roots.clone());
        let report = collector.sample(&roots, crate::test_support::at(0));
        let cpu0 = report.entities_of(EntityKind::LogicalCpu).next().unwrap();
        let hertz = cpu0
            .metrics
            .get(&metric_id("cpu.frequency.current"))
            .unwrap()
            .as_f64()
            .expect("a frequency");
        // The captured value was 3595718 kHz.
        assert_eq!(hertz, 3_595_718_000.0);
        assert_eq!(
            cpu0.metrics.state_of(&metric_id("cpu.governor")),
            ObservationState::Value
        );
    }

    #[test]
    fn a_host_without_hwmon_reports_temperature_unsupported_not_absent() {
        let roots = Roots::at(crate::test_support::fixture("synthetic-a"));
        let mut collector = CpuCollector::new(roots.clone());
        let report = collector.sample(&roots, crate::test_support::at(0));
        assert_eq!(
            report
                .metrics
                .state_of(&metric_id("cpu.package.temperature")),
            ObservationState::Unsupported
        );
    }

    #[test]
    fn a_missing_loadavg_leaves_the_rest_of_the_cpu_report_intact() {
        let temporary = crate::test_support::TempTree::copy_of("synthetic-a");
        std::fs::remove_file(temporary.roots().proc("loadavg")).unwrap();
        let mut collector = CpuCollector::new(temporary.roots());
        let report = collector.sample(&temporary.roots(), crate::test_support::at(0));
        assert_eq!(
            report.metrics.state_of(&metric_id("cpu.load.average.1m")),
            ObservationState::Unsupported
        );
        assert_eq!(
            report.metrics.state_of(&metric_id("cpu.logical.count")),
            ObservationState::Value
        );
    }
}
