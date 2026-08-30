//! The collector contract.
//!
//! A collector reports what it saw, including that it saw nothing. It never
//! returns `Err` for a metric it could not read, because a failed read is data
//! the user needs — the Overview has to be able to say "this is unobserved"
//! rather than showing an empty chart. A collector only reports a health state
//! when the whole subsystem is unavailable.

use crate::MonitorError;
use crate::metric::{MetricCapability, MetricDescriptor, MetricId, SupportState};
use crate::observation::{MetricSet, Observation, UnsupportedReason};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The longest collector identifier a diagnostics page or export directory
/// name can carry.
pub const MAX_COLLECTOR_ID_LENGTH: usize = 48;

/// A collector name such as `linux.cpu`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollectorId(String);

impl CollectorId {
    pub fn new(value: impl Into<String>) -> Result<Self, MonitorError> {
        let value = value.into();
        let shaped = !value.is_empty()
            && value.len() <= MAX_COLLECTOR_ID_LENGTH
            && value.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '.'
            })
            && !value.starts_with('.')
            && !value.ends_with('.')
            && !value.contains("..");
        if !shaped {
            return Err(MonitorError::InvalidCollectorId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CollectorId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// What a set of readings is about.
///
/// Keeping the kind separate from the key stops a device named `1` and a
/// process with PID 1 from ever colliding in a store or an export.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// The machine as a whole.
    System,
    /// One logical CPU as the kernel numbers them in `/proc/stat`.
    LogicalCpu,
    /// One process.
    Process,
    /// One whole block device, never a partition.
    BlockDevice,
    /// One network interface.
    NetworkInterface,
    /// One PSI resource: `cpu`, `memory`, or `io`.
    PressureResource,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct EntityId {
    pub kind: EntityKind,
    pub key: String,
}

impl EntityId {
    pub fn new(kind: EntityKind, key: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.into(),
        }
    }

    pub fn system() -> Self {
        Self::new(EntityKind::System, "system")
    }
}

/// One thing being measured, and everything measured about it this round.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub metrics: MetricSet,
}

impl Entity {
    pub fn new(id: EntityId, metrics: MetricSet) -> Self {
        Self { id, metrics }
    }
}

/// Wall-clock and monotonic time for one sampling round.
///
/// Both are recorded because they answer different questions. The wall clock
/// is what a user reads on an incident; the monotonic clock is what a rate
/// must be divided by, because it does not jump when the system clock is
/// corrected.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Timestamp {
    pub unix_ms: u64,
    /// Nanoseconds since this process started observing. Only differences
    /// between two `Timestamp`s from the same process are meaningful.
    pub monotonic_ns: u64,
}

fn monotonic_origin() -> Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    *ORIGIN.get_or_init(Instant::now)
}

impl Timestamp {
    pub fn now() -> Self {
        let unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0);
        Self {
            unix_ms,
            monotonic_ns: monotonic_origin().elapsed().as_nanos() as u64,
        }
    }

    /// Seconds between two rounds, taken from the monotonic clock. Returns
    /// `None` when the later timestamp is not actually later, so a caller can
    /// report `Unknown` instead of dividing by zero.
    pub fn interval_seconds(earlier: Timestamp, later: Timestamp) -> Option<f64> {
        let delta = later.monotonic_ns.checked_sub(earlier.monotonic_ns)?;
        if delta == 0 {
            return None;
        }
        Some(delta as f64 / 1_000_000_000.0)
    }
}

/// Whether the collector itself is working, as opposed to whether any one
/// metric had a value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectorHealth {
    Healthy,
    /// Running, but some of what it promises is missing this round.
    Degraded {
        detail: String,
    },
    /// Could not produce anything useful this round, and expects to recover.
    Failed {
        detail: String,
    },
    /// This host will never support the subsystem. A kernel without
    /// `CONFIG_PSI` is the example the product has to handle gracefully.
    Unsupported(UnsupportedReason),
}

/// One collector's output for one sampling round.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectorReport {
    pub collector: CollectorId,
    pub observed_at: Timestamp,
    pub health: CollectorHealth,
    /// Readings about the machine as a whole.
    pub metrics: MetricSet,
    /// Readings about individual CPUs, processes, devices, or interfaces.
    pub entities: Vec<Entity>,
}

impl CollectorReport {
    pub fn new(collector: CollectorId, observed_at: Timestamp) -> Self {
        Self {
            collector,
            observed_at,
            health: CollectorHealth::Healthy,
            metrics: MetricSet::new(),
            entities: Vec::new(),
        }
    }

    pub fn entities_of(&self, kind: EntityKind) -> impl Iterator<Item = &Entity> {
        self.entities
            .iter()
            .filter(move |entity| entity.id.kind == kind)
    }

    pub fn entity(&self, id: &EntityId) -> Option<&Entity> {
        self.entities.iter().find(|entity| &entity.id == id)
    }

    /// The strongest support statement this round makes about a metric.
    ///
    /// One CPU reporting a temperature proves the host supports the metric
    /// even if another CPU does not, so support is resolved across entities
    /// rather than per entity.
    pub fn support_of(&self, id: &MetricId) -> SupportState {
        let mut best = SupportState::Unknown;
        let sets = std::iter::once(&self.metrics).chain(self.entities.iter().map(|e| &e.metrics));
        for set in sets {
            let Some(observation) = set.get(id) else {
                continue;
            };
            let candidate = observation.support_state();
            if candidate == SupportState::Supported {
                return SupportState::Supported;
            }
            if best == SupportState::Unknown {
                best = candidate;
            }
        }
        best
    }

    /// Pair a declared catalog with what this round proved about it.
    pub fn capabilities(&self, descriptors: &[MetricDescriptor]) -> Vec<MetricCapability> {
        descriptors
            .iter()
            .map(|descriptor| MetricCapability {
                descriptor: descriptor.clone(),
                support: self.support_of(&descriptor.id),
            })
            .collect()
    }

    /// Every observation this round produced, system-level and per-entity, in
    /// a single pass. Coverage reporting and export redaction both need this.
    pub fn observations(
        &self,
    ) -> impl Iterator<Item = (Option<&EntityId>, &MetricId, &Observation)> {
        let system = self
            .metrics
            .iter()
            .map(|(id, observation)| (None, id, observation));
        let entities = self.entities.iter().flat_map(|entity| {
            entity
                .metrics
                .iter()
                .map(move |(id, observation)| (Some(&entity.id), id, observation))
        });
        system.chain(entities)
    }
}

/// A source of readings.
///
/// `collect` cannot fail: an unreadable subsystem is reported through
/// `CollectorHealth` and through per-metric observations, so a caller always
/// gets a truthful report rather than an error that erases the round.
pub trait Collector {
    fn id(&self) -> CollectorId;

    /// Everything this collector can ever emit, whether or not this host
    /// supports it. The catalog is static so the GUI can render an
    /// unsupported page truthfully instead of hiding it.
    fn descriptors(&self) -> Vec<MetricDescriptor>;

    fn collect(&mut self, at: Timestamp) -> CollectorReport;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::{MetricSource, SamplingBehavior, SemanticType, Unit};
    use crate::observation::{UnknownReason, UnsupportedReason};
    use std::time::Duration;

    fn metric(raw: &str) -> MetricId {
        MetricId::new(raw).unwrap()
    }

    fn descriptor(raw: &str) -> MetricDescriptor {
        MetricDescriptor::new(
            metric(raw),
            Unit::DegreesCelsius,
            SemanticType::Gauge,
            MetricSource::Sys("class/hwmon".into()),
            SamplingBehavior::instant(Duration::from_secs(5)),
            "test descriptor",
        )
    }

    fn report_with_two_cpus(first: Observation, second: Observation) -> CollectorReport {
        let mut report = CollectorReport::new(
            CollectorId::new("linux.cpu").unwrap(),
            Timestamp {
                unix_ms: 1,
                monotonic_ns: 0,
            },
        );
        let mut cpu0 = MetricSet::new();
        cpu0.insert(metric("cpu.temperature"), first);
        let mut cpu1 = MetricSet::new();
        cpu1.insert(metric("cpu.temperature"), second);
        report.entities.push(Entity::new(
            EntityId::new(EntityKind::LogicalCpu, "0"),
            cpu0,
        ));
        report.entities.push(Entity::new(
            EntityId::new(EntityKind::LogicalCpu, "1"),
            cpu1,
        ));
        report
    }

    #[test]
    fn a_collector_id_rejects_a_shape_a_directory_name_could_not_carry() {
        assert!(CollectorId::new("linux.cpu").is_ok());
        for candidate in ["", "Linux.cpu", "linux cpu", "linux..cpu", ".cpu", "cpu."] {
            assert!(CollectorId::new(candidate).is_err(), "{candidate:?}");
        }
    }

    #[test]
    fn one_entity_reporting_a_value_proves_the_host_supports_the_metric() {
        let report = report_with_two_cpus(
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "no label".into(),
            }),
            Observation::float(48.5),
        );
        assert_eq!(
            report.support_of(&metric("cpu.temperature")),
            SupportState::Supported
        );
    }

    #[test]
    fn a_metric_no_entity_reported_stays_unknown_rather_than_unsupported() {
        let report = report_with_two_cpus(
            Observation::Unknown(UnknownReason::NotYetSampled),
            Observation::Unknown(UnknownReason::NotYetSampled),
        );
        assert_eq!(
            report.support_of(&metric("cpu.temperature")),
            SupportState::Unknown
        );
        assert_eq!(
            report.support_of(&metric("cpu.frequency.current")),
            SupportState::Unknown
        );
    }

    #[test]
    fn capabilities_pair_the_declared_catalog_with_what_the_round_proved() {
        let report = report_with_two_cpus(
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "k10temp exposes Tctl only".into(),
            }),
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "k10temp exposes Tctl only".into(),
            }),
        );
        let capabilities =
            report.capabilities(&[descriptor("cpu.temperature"), descriptor("cpu.power")]);
        assert_eq!(capabilities.len(), 2);
        assert!(matches!(
            capabilities[0].support,
            SupportState::Unsupported(_)
        ));
        assert_eq!(capabilities[1].support, SupportState::Unknown);
    }

    #[test]
    fn observations_walk_system_and_entity_readings_together() {
        let mut report = report_with_two_cpus(Observation::float(1.0), Observation::float(2.0));
        report
            .metrics
            .insert(metric("cpu.load.1m"), Observation::float(0.9));
        let all: Vec<_> = report.observations().collect();
        assert_eq!(all.len(), 3);
        assert_eq!(
            all.iter().filter(|(entity, _, _)| entity.is_none()).count(),
            1
        );
    }

    #[test]
    fn an_interval_needs_the_monotonic_clock_to_have_moved_forward() {
        let earlier = Timestamp {
            unix_ms: 1_000,
            monotonic_ns: 1_000_000_000,
        };
        let later = Timestamp {
            unix_ms: 3_000,
            monotonic_ns: 3_000_000_000,
        };
        assert_eq!(Timestamp::interval_seconds(earlier, later), Some(2.0));
        assert_eq!(Timestamp::interval_seconds(earlier, earlier), None);
        assert_eq!(Timestamp::interval_seconds(later, earlier), None);
    }

    #[test]
    fn the_wall_clock_moving_backwards_does_not_change_a_rate_interval() {
        // A clock correction between rounds must not make a rate look like a
        // spike, so the interval comes from the monotonic field only.
        let earlier = Timestamp {
            unix_ms: 10_000,
            monotonic_ns: 0,
        };
        let later = Timestamp {
            unix_ms: 5_000,
            monotonic_ns: 1_000_000_000,
        };
        assert_eq!(Timestamp::interval_seconds(earlier, later), Some(1.0));
    }

    #[test]
    fn now_produces_a_monotonic_clock_that_does_not_go_backwards() {
        let first = Timestamp::now();
        let second = Timestamp::now();
        assert!(second.monotonic_ns >= first.monotonic_ns);
    }
}
