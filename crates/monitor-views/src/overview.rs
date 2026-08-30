//! The Overview: busy, stuck, throttled, broken, or unobserved.
//!
//! The product requirement this module exists for is that a monitor which only
//! knows utilization cannot tell a machine doing a lot of work from a machine
//! that is stuck waiting. Utilization and pressure are therefore read together
//! and resolved into one verdict per resource, and every verdict that is not a
//! measurement says which of the four other things it is.
//!
//! ## What throttling can and cannot be claimed here
//!
//! Ticket 22's collectors read clock and temperature but no throttling
//! counter — there is no `thermal_throttle` or RAPL reader yet. So the only
//! throttling this module will claim is a clock held far below the maximum
//! while the CPU is pegged, which is a real symptom and is reported as such.
//! Where the clock metrics are absent the state is `NotObservable` with the
//! reason, never `NotThrottled`, because "we cannot see it" is not "it is not
//! happening".

use crate::apps::AppsModel;
use crate::field::{self, Field};
use monitor_core::{
    CollectorHealth, CollectorReport, EntityKind, MetricId, MetricSet, ObservationState,
};
use std::collections::BTreeMap;

/// Utilization at or above this is "busy" rather than "nominal".
pub const HIGH_UTILIZATION: f64 = 0.85;

/// `some` PSI at or above this means work is waiting for the resource.
///
/// PSI `avg10` is the percentage of the last ten seconds in which at least one
/// task was stalled on the resource. Twenty percent is high enough that a user
/// notices and low enough to catch a machine before it is unusable.
pub const SOME_PRESSURE_PERCENT: f64 = 20.0;

/// `full` PSI at or above this means *everything* was waiting: the machine is
/// not making progress on this resource at all.
pub const FULL_PRESSURE_PERCENT: f64 = 10.0;

/// A clock below this fraction of the maximum while the CPU is pegged is
/// evidence of throttling rather than of an idle governor.
pub const THROTTLED_CLOCK_FRACTION: f64 = 0.6;

fn id(raw: &str) -> MetricId {
    MetricId::new(raw).expect("an overview metric id must be well formed")
}

/// What the Overview concluded about one resource.
#[derive(Clone, Debug, PartialEq)]
pub enum ResourceVerdict {
    /// Working, with headroom.
    Nominal,
    /// Working hard, and nothing is waiting for it. This is a machine getting
    /// something done, not a machine in trouble.
    BusyWithoutContention,
    /// Some work is waiting on this resource.
    UnderPressure { some_percent: f64 },
    /// All work is waiting on this resource.
    Saturated { full_percent: f64 },
    /// The collector could not produce this round.
    CollectorFailed { detail: String },
    /// This host cannot report the resource at all.
    Unsupported { detail: String },
    /// Nothing has been measured yet, or the readings are not usable.
    Unobserved { detail: String },
}

impl ResourceVerdict {
    /// A stable key for tests, logs, and locale lookup.
    pub fn key(&self) -> &'static str {
        match self {
            ResourceVerdict::Nominal => "nominal",
            ResourceVerdict::BusyWithoutContention => "busy",
            ResourceVerdict::UnderPressure { .. } => "pressure",
            ResourceVerdict::Saturated { .. } => "saturated",
            ResourceVerdict::CollectorFailed { .. } => "collector-failed",
            ResourceVerdict::Unsupported { .. } => "unsupported",
            ResourceVerdict::Unobserved { .. } => "unobserved",
        }
    }

    /// Whether the verdict describes a problem the user should act on.
    pub fn is_contended(&self) -> bool {
        matches!(
            self,
            ResourceVerdict::UnderPressure { .. } | ResourceVerdict::Saturated { .. }
        )
    }
}

/// Whether the CPU is being held below its own capability.
#[derive(Clone, Debug, PartialEq)]
pub enum ThrottlingState {
    NotThrottled,
    /// The clock is far below maximum while the CPU is pegged.
    ClockHeldDown {
        current_hz: f64,
        maximum_hz: f64,
    },
    /// The collectors in this build cannot answer the question.
    NotObservable {
        detail: String,
    },
}

/// One resource's readings and the verdict drawn from them.
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceSummary {
    pub utilization: Field<f64>,
    pub pressure_some: Field<f64>,
    pub pressure_full: Field<f64>,
    pub verdict: ResourceVerdict,
}

/// The memory figures the Overview shows beyond utilization, because "free" is
/// not "available" and a monitor that shows only one of them misleads.
#[derive(Clone, Debug, PartialEq)]
pub struct MemorySummary {
    pub resource: ResourceSummary,
    pub total: Field<u64>,
    pub available: Field<u64>,
    pub used: Field<u64>,
    pub cached: Field<u64>,
    pub swap_total: Field<u64>,
    pub swap_used: Field<u64>,
    pub major_fault_rate: Field<f64>,
    pub swap_in_rate: Field<f64>,
    pub swap_out_rate: Field<f64>,
}

/// Throughput summed over devices or interfaces, with its own coverage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThroughputSummary {
    pub read: crate::apps::Aggregate,
    pub write: crate::apps::Aggregate,
    /// The device or interface contributing the most, when one can be named.
    pub busiest: Option<String>,
}

/// One collector's state this round.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectorStatus {
    pub collector: String,
    pub health: CollectorHealth,
}

impl CollectorStatus {
    pub fn is_healthy(&self) -> bool {
        matches!(self.health, CollectorHealth::Healthy)
    }

    /// One line explaining a state that is not healthy.
    pub fn detail(&self) -> Option<String> {
        match &self.health {
            CollectorHealth::Healthy => None,
            CollectorHealth::Degraded { detail } | CollectorHealth::Failed { detail } => {
                Some(detail.clone())
            }
            CollectorHealth::Unsupported(reason) => Some(format!("{reason:?}")),
        }
    }
}

/// How many observations of each state the round produced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoverageSummary {
    pub value: usize,
    pub stale: usize,
    pub unknown: usize,
    pub unsupported: usize,
    pub permission_denied: usize,
}

impl CoverageSummary {
    pub fn total(&self) -> usize {
        self.value + self.stale + self.unknown + self.unsupported + self.permission_denied
    }

    /// The share of readings that produced a value. `None` when there were no
    /// readings at all, rather than a reassuring 100%.
    pub fn observed_fraction(&self) -> Option<f64> {
        (self.total() > 0).then(|| (self.value + self.stale) as f64 / self.total() as f64)
    }
}

/// Everything the Overview page draws.
#[derive(Clone, Debug, PartialEq)]
pub struct OverviewModel {
    pub cpu: ResourceSummary,
    pub memory: MemorySummary,
    pub io: ResourceSummary,
    pub throttling: ThrottlingState,
    pub load_average_1m: Field<f64>,
    pub logical_cpus: Field<u64>,
    pub process_count: Field<u64>,
    pub storage: ThroughputSummary,
    pub network: ThroughputSummary,
    pub collectors: Vec<CollectorStatus>,
    pub coverage: CoverageSummary,
}

impl OverviewModel {
    /// Build from one round of collector reports.
    pub fn from_reports(reports: &[CollectorReport]) -> Self {
        let health = |name: &str| -> Option<CollectorHealth> {
            reports
                .iter()
                .find(|report| report.collector.as_str() == name)
                .map(|report| report.health.clone())
        };
        let system = |name: &str| -> MetricSet {
            reports
                .iter()
                .find(|report| report.collector.as_str() == name)
                .map(|report| report.metrics.clone())
                .unwrap_or_default()
        };

        let cpu_metrics = system("linux.cpu");
        let memory_metrics = system("linux.memory");
        let process_metrics = system("linux.process");
        let pressure = pressure_sets(reports);

        let cpu_utilization = field::number(&cpu_metrics, &id("cpu.utilization.busy"));
        let cpu = resource_summary(
            cpu_utilization.clone(),
            pressure.get("cpu"),
            health("linux.cpu"),
            health("linux.pressure"),
        );
        let memory_utilization = field::number(&memory_metrics, &id("memory.utilization"));
        let memory_resource = resource_summary(
            memory_utilization,
            pressure.get("memory"),
            health("linux.memory"),
            health("linux.pressure"),
        );
        let io = resource_summary(
            // There is no single "I/O utilization" for the machine: it is a
            // per-device figure. The verdict therefore rests on pressure, and
            // the per-device numbers live in `storage`.
            Field::NotCollected,
            pressure.get("io"),
            health("linux.storage"),
            health("linux.pressure"),
        );

        Self {
            throttling: throttling_state(&cpu_metrics, &cpu_utilization),
            cpu,
            memory: MemorySummary {
                resource: memory_resource,
                total: field::unsigned(&memory_metrics, &id("memory.total")),
                available: field::unsigned(&memory_metrics, &id("memory.available")),
                used: field::unsigned(&memory_metrics, &id("memory.used")),
                cached: field::unsigned(&memory_metrics, &id("memory.cached")),
                swap_total: field::unsigned(&memory_metrics, &id("memory.swap.total")),
                swap_used: field::unsigned(&memory_metrics, &id("memory.swap.used")),
                major_fault_rate: field::number(&memory_metrics, &id("memory.fault.major.rate")),
                swap_in_rate: field::number(&memory_metrics, &id("memory.swap_in.rate")),
                swap_out_rate: field::number(&memory_metrics, &id("memory.swap_out.rate")),
            },
            io,
            load_average_1m: field::number(&cpu_metrics, &id("cpu.load.average.1m")),
            logical_cpus: field::unsigned(&cpu_metrics, &id("cpu.logical.count")),
            process_count: field::unsigned(&process_metrics, &id("process.count")),
            storage: throughput(
                reports,
                "linux.storage",
                EntityKind::BlockDevice,
                "storage.read.bytes.rate",
                "storage.write.bytes.rate",
            ),
            network: throughput(
                reports,
                "linux.network",
                EntityKind::NetworkInterface,
                "network.rx.bytes.rate",
                "network.tx.bytes.rate",
            ),
            collectors: reports
                .iter()
                .map(|report| CollectorStatus {
                    collector: report.collector.to_string(),
                    health: report.health.clone(),
                })
                .collect(),
            coverage: coverage(reports),
        }
    }

    /// The collectors that are not fully working, which is what the Overview's
    /// observation-health card lists.
    pub fn unhealthy_collectors(&self) -> Vec<&CollectorStatus> {
        self.collectors
            .iter()
            .filter(|status| !status.is_healthy())
            .collect()
    }

    /// The busiest applications, taken from an already-built Apps model so the
    /// Overview and the Apps page cannot disagree.
    pub fn top_apps<'a>(&self, apps: &'a AppsModel, limit: usize) -> Vec<&'a crate::apps::AppRow> {
        apps.top_by_cpu(limit)
    }
}

/// The PSI metric sets, keyed by resource.
fn pressure_sets(reports: &[CollectorReport]) -> BTreeMap<String, MetricSet> {
    let mut sets = BTreeMap::new();
    for report in reports {
        if report.collector.as_str() != "linux.pressure" {
            continue;
        }
        for entity in report.entities_of(EntityKind::PressureResource) {
            sets.insert(entity.id.key.clone(), entity.metrics.clone());
        }
    }
    sets
}

fn resource_summary(
    utilization: Field<f64>,
    pressure: Option<&MetricSet>,
    own_health: Option<CollectorHealth>,
    pressure_health: Option<CollectorHealth>,
) -> ResourceSummary {
    let (some, full) = match pressure {
        Some(metrics) => (
            field::number(metrics, &id("pressure.some.avg10")),
            field::number(metrics, &id("pressure.full.avg10")),
        ),
        None => match &pressure_health {
            Some(CollectorHealth::Unsupported(reason)) => (
                Field::Unsupported(reason.clone()),
                Field::Unsupported(reason.clone()),
            ),
            _ => (Field::NotCollected, Field::NotCollected),
        },
    };

    let verdict = verdict_for(&utilization, &some, &full, own_health.as_ref());
    ResourceSummary {
        utilization,
        pressure_some: some,
        pressure_full: full,
        verdict,
    }
}

fn verdict_for(
    utilization: &Field<f64>,
    some: &Field<f64>,
    full: &Field<f64>,
    health: Option<&CollectorHealth>,
) -> ResourceVerdict {
    // A failed collector is the answer, whatever the last readings said.
    match health {
        Some(CollectorHealth::Failed { detail }) => {
            return ResourceVerdict::CollectorFailed {
                detail: detail.clone(),
            };
        }
        Some(CollectorHealth::Unsupported(reason)) => {
            return ResourceVerdict::Unsupported {
                detail: format!("{reason:?}"),
            };
        }
        _ => {}
    }

    // Pressure answers the question utilization cannot, so it is read first.
    if let Some(full) = full.any_value().copied() {
        if full >= FULL_PRESSURE_PERCENT {
            return ResourceVerdict::Saturated { full_percent: full };
        }
    }
    if let Some(some) = some.any_value().copied() {
        if some >= SOME_PRESSURE_PERCENT {
            return ResourceVerdict::UnderPressure { some_percent: some };
        }
    }

    match utilization.any_value().copied() {
        Some(value) if value >= HIGH_UTILIZATION => ResourceVerdict::BusyWithoutContention,
        Some(_) => ResourceVerdict::Nominal,
        None => {
            // No utilization figure. Pressure alone can still say the resource
            // is fine, which is the case for I/O where there is no single
            // machine-wide utilization to read.
            if some.any_value().is_some() || full.any_value().is_some() {
                ResourceVerdict::Nominal
            } else {
                match (utilization, some) {
                    (Field::Unsupported(reason), _) | (_, Field::Unsupported(reason)) => {
                        ResourceVerdict::Unsupported {
                            detail: format!("{reason:?}"),
                        }
                    }
                    _ => ResourceVerdict::Unobserved {
                        detail: "no utilization or pressure reading in this round".into(),
                    },
                }
            }
        }
    }
}

fn throttling_state(cpu_metrics: &MetricSet, utilization: &Field<f64>) -> ThrottlingState {
    let current = field::number(cpu_metrics, &id("cpu.frequency.current"));
    let maximum = field::number(cpu_metrics, &id("cpu.frequency.max"));
    let (Some(current_hz), Some(maximum_hz)) =
        (current.any_value().copied(), maximum.any_value().copied())
    else {
        return ThrottlingState::NotObservable {
            detail: "this build has no throttling counter and the clock is not readable".into(),
        };
    };
    if maximum_hz <= 0.0 {
        return ThrottlingState::NotObservable {
            detail: "the reported maximum clock is not usable".into(),
        };
    }
    let busy = utilization.any_value().copied().unwrap_or(0.0);
    if busy >= HIGH_UTILIZATION && current_hz < maximum_hz * THROTTLED_CLOCK_FRACTION {
        ThrottlingState::ClockHeldDown {
            current_hz,
            maximum_hz,
        }
    } else {
        ThrottlingState::NotThrottled
    }
}

fn throughput(
    reports: &[CollectorReport],
    collector: &str,
    kind: EntityKind,
    read_metric: &str,
    write_metric: &str,
) -> ThroughputSummary {
    let mut summary = ThroughputSummary::default();
    let mut busiest: Option<(String, f64)> = None;
    for report in reports.iter().filter(|r| r.collector.as_str() == collector) {
        for entity in report.entities_of(kind) {
            let read = field::number(&entity.metrics, &id(read_metric));
            let write = field::number(&entity.metrics, &id(write_metric));
            let mut device_total = 0.0;
            for (field, aggregate) in [(&read, &mut summary.read), (&write, &mut summary.write)] {
                match field.any_value().copied() {
                    Some(value) => {
                        aggregate.total += value;
                        aggregate.counted += 1;
                        device_total += value;
                    }
                    None => aggregate.missing += 1,
                }
            }
            if read.any_value().is_some() || write.any_value().is_some() {
                let better = busiest
                    .as_ref()
                    .is_none_or(|(_, best)| device_total > *best);
                if better {
                    busiest = Some((entity.id.key.clone(), device_total));
                }
            }
        }
    }
    summary.busiest = busiest.map(|(name, _)| name);
    summary
}

fn coverage(reports: &[CollectorReport]) -> CoverageSummary {
    let mut summary = CoverageSummary::default();
    for report in reports {
        for (_, _, observation) in report.observations() {
            match observation.state() {
                ObservationState::Value => summary.value += 1,
                ObservationState::Stale => summary.stale += 1,
                ObservationState::Unknown => summary.unknown += 1,
                ObservationState::Unsupported => summary.unsupported += 1,
                ObservationState::PermissionDenied => summary.permission_denied += 1,
            }
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{CollectorId, Entity, EntityId, Observation, Timestamp, UnsupportedReason};

    fn at() -> Timestamp {
        Timestamp {
            unix_ms: 1,
            monotonic_ns: 0,
        }
    }

    fn report(name: &str) -> CollectorReport {
        CollectorReport::new(CollectorId::new(name).unwrap(), at())
    }

    fn cpu(busy: f64) -> CollectorReport {
        let mut report = report("linux.cpu");
        report
            .metrics
            .insert(id("cpu.utilization.busy"), Observation::float(busy));
        report
            .metrics
            .insert(id("cpu.logical.count"), Observation::unsigned(8));
        report
    }

    fn pressure(resource: &str, some: f64, full: f64) -> CollectorReport {
        let mut report = report("linux.pressure");
        let mut metrics = MetricSet::new();
        metrics.insert(id("pressure.some.avg10"), Observation::float(some));
        metrics.insert(id("pressure.full.avg10"), Observation::float(full));
        report.entities.push(Entity::new(
            EntityId::new(EntityKind::PressureResource, resource),
            metrics,
        ));
        report
    }

    #[test]
    fn a_busy_cpu_with_no_waiting_is_not_reported_as_a_problem() {
        let model = OverviewModel::from_reports(&[cpu(0.93), pressure("cpu", 2.0, 0.0)]);
        assert_eq!(model.cpu.verdict, ResourceVerdict::BusyWithoutContention);
        assert!(!model.cpu.verdict.is_contended());
    }

    #[test]
    fn pressure_outranks_utilization_because_waiting_is_the_worse_state() {
        let model = OverviewModel::from_reports(&[cpu(0.30), pressure("cpu", 45.0, 0.0)]);
        assert_eq!(
            model.cpu.verdict,
            ResourceVerdict::UnderPressure { some_percent: 45.0 }
        );
        assert!(model.cpu.verdict.is_contended());

        let stalled = OverviewModel::from_reports(&[cpu(0.30), pressure("cpu", 60.0, 25.0)]);
        assert_eq!(
            stalled.cpu.verdict,
            ResourceVerdict::Saturated { full_percent: 25.0 }
        );
    }

    #[test]
    fn a_quiet_machine_is_nominal() {
        let model = OverviewModel::from_reports(&[cpu(0.10), pressure("cpu", 0.0, 0.0)]);
        assert_eq!(model.cpu.verdict, ResourceVerdict::Nominal);
    }

    #[test]
    fn a_kernel_without_psi_reports_unsupported_rather_than_no_pressure() {
        let mut pressure = report("linux.pressure");
        pressure.health = CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
            path: "/proc/pressure/cpu".into(),
        });
        let model = OverviewModel::from_reports(&[cpu(0.10), pressure]);
        assert!(matches!(model.cpu.pressure_some, Field::Unsupported(_)));
        // The CPU verdict still stands on its own utilization reading.
        assert_eq!(model.cpu.verdict, ResourceVerdict::Nominal);
        // And the collector list says why.
        assert_eq!(model.unhealthy_collectors().len(), 1);
    }

    #[test]
    fn a_failed_collector_is_the_verdict_rather_than_a_stale_reading() {
        let mut broken = cpu(0.9);
        broken.health = CollectorHealth::Failed {
            detail: "/proc/stat unreadable".into(),
        };
        let model = OverviewModel::from_reports(&[broken, pressure("cpu", 0.0, 0.0)]);
        assert_eq!(
            model.cpu.verdict,
            ResourceVerdict::CollectorFailed {
                detail: "/proc/stat unreadable".into()
            }
        );
        assert_eq!(model.cpu.verdict.key(), "collector-failed");
    }

    #[test]
    fn a_round_with_nothing_in_it_is_unobserved_and_not_idle() {
        let model = OverviewModel::from_reports(&[]);
        assert!(matches!(
            model.cpu.verdict,
            ResourceVerdict::Unobserved { .. }
        ));
        assert!(matches!(
            model.memory.resource.verdict,
            ResourceVerdict::Unobserved { .. }
        ));
        assert_eq!(model.cpu.utilization, Field::NotCollected);
        assert_eq!(model.coverage.total(), 0);
        assert_eq!(model.coverage.observed_fraction(), None);
    }

    #[test]
    fn io_has_no_machine_wide_utilization_and_still_reaches_a_verdict() {
        let model = OverviewModel::from_reports(&[pressure("io", 1.0, 0.0)]);
        assert_eq!(model.io.utilization, Field::NotCollected);
        assert_eq!(model.io.verdict, ResourceVerdict::Nominal);

        let stalled = OverviewModel::from_reports(&[pressure("io", 80.0, 40.0)]);
        assert_eq!(
            stalled.io.verdict,
            ResourceVerdict::Saturated { full_percent: 40.0 }
        );
    }

    #[test]
    fn throttling_is_only_claimed_when_the_clock_says_so() {
        let mut busy = cpu(0.95);
        busy.metrics
            .insert(id("cpu.frequency.current"), Observation::float(1.2e9));
        busy.metrics
            .insert(id("cpu.frequency.max"), Observation::float(4.8e9));
        let model = OverviewModel::from_reports(&[busy]);
        assert_eq!(
            model.throttling,
            ThrottlingState::ClockHeldDown {
                current_hz: 1.2e9,
                maximum_hz: 4.8e9
            }
        );

        // The same low clock on an idle machine is a governor doing its job.
        let mut idle = cpu(0.05);
        idle.metrics
            .insert(id("cpu.frequency.current"), Observation::float(1.2e9));
        idle.metrics
            .insert(id("cpu.frequency.max"), Observation::float(4.8e9));
        assert_eq!(
            OverviewModel::from_reports(&[idle]).throttling,
            ThrottlingState::NotThrottled
        );
    }

    #[test]
    fn a_host_that_cannot_report_a_clock_says_so_rather_than_claiming_no_throttling() {
        let model = OverviewModel::from_reports(&[cpu(0.95)]);
        assert!(matches!(
            model.throttling,
            ThrottlingState::NotObservable { .. }
        ));
    }

    #[test]
    fn storage_throughput_is_summed_and_the_busiest_device_is_named() {
        let mut storage = report("linux.storage");
        for (device, read, write) in [("nvme0n1", 5.0e6, 1.0e6), ("sda", 1.0e5, 0.0)] {
            let mut metrics = MetricSet::new();
            metrics.insert(id("storage.read.bytes.rate"), Observation::float(read));
            metrics.insert(id("storage.write.bytes.rate"), Observation::float(write));
            storage.entities.push(Entity::new(
                EntityId::new(EntityKind::BlockDevice, device),
                metrics,
            ));
        }
        let model = OverviewModel::from_reports(&[storage]);
        assert_eq!(model.storage.read.total, 5.1e6);
        assert!(model.storage.read.is_complete());
        assert_eq!(model.storage.busiest.as_deref(), Some("nvme0n1"));
    }

    #[test]
    fn a_device_that_did_not_report_makes_the_throughput_partial() {
        let mut storage = report("linux.storage");
        let mut good = MetricSet::new();
        good.insert(id("storage.read.bytes.rate"), Observation::float(10.0));
        good.insert(id("storage.write.bytes.rate"), Observation::float(0.0));
        storage.entities.push(Entity::new(
            EntityId::new(EntityKind::BlockDevice, "nvme0n1"),
            good,
        ));
        let mut silent = MetricSet::new();
        silent.insert(
            id("storage.read.bytes.rate"),
            Observation::Unknown(monitor_core::UnknownReason::NotYetSampled),
        );
        storage.entities.push(Entity::new(
            EntityId::new(EntityKind::BlockDevice, "sda"),
            silent,
        ));
        let model = OverviewModel::from_reports(&[storage]);
        assert!(model.storage.read.is_partial());
        assert_eq!(model.storage.read.missing, 1);
    }

    #[test]
    fn coverage_counts_every_state_the_round_produced() {
        let mut memory = report("linux.memory");
        memory
            .metrics
            .insert(id("memory.utilization"), Observation::float(0.4));
        memory.metrics.insert(
            id("memory.zswap.compressed"),
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "no zswap".into(),
            }),
        );
        let model = OverviewModel::from_reports(&[memory]);
        assert_eq!(model.coverage.value, 1);
        assert_eq!(model.coverage.unsupported, 1);
        assert_eq!(model.coverage.total(), 2);
        assert_eq!(model.coverage.observed_fraction(), Some(0.5));
    }
}
