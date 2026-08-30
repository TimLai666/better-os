//! What one recorded moment of history looks like.
//!
//! A stored sample is not a collector round. A round carries every process on
//! the machine, and keeping that for hours is exactly the unbounded per-process
//! history the specification forbids. So a sample keeps the system-level
//! readings, the non-process entities, a bounded list of the busiest processes,
//! and every collector's health — and it keeps all of them as observations, so
//! a metric that was unsupported at 14:03 still reads as unsupported and never
//! as zero.

use std::collections::BTreeMap;

use monitor_core::{
    CollectorHealth, CollectorId, CollectorReport, EntityKind, MetricId, MetricScalar, MetricSet,
    Observation, ObservationState,
};
use serde::{Deserialize, Serialize};

/// How many processes one sample keeps. The busiest few explain a slowdown;
/// the other nine hundred are what makes a history file unbounded.
pub const DEFAULT_TRACKED_PROCESSES: usize = 10;

/// One process, as much of it as history keeps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessSample {
    pub pid: u32,
    pub name: String,
    /// Present only when the user turned command-line collection on. It is
    /// still redacted on the way into an export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_utilization: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_resident: Option<u64>,
}

/// Readings about one non-process entity: a CPU, a disk, a link, a PSI
/// resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntitySample {
    pub kind: EntityKind,
    pub key: String,
    pub metrics: MetricSet,
}

/// One collector's health at the moment of the sample.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectorState {
    pub collector: CollectorId,
    pub health: CollectorHealth,
}

/// One recorded moment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// What a person reads on a clock. Subject to correction, which is why it
    /// is not the field a rate is divided by.
    pub wall_unix_ms: u64,
    /// The recording process's monotonic clock. Only differences within one
    /// run of the service mean anything.
    pub monotonic_ns: u64,
    /// How many raw collector rounds this sample was downsampled from. One
    /// means it is a raw round.
    pub rounds: u32,
    /// Readings about the machine as a whole, merged across collectors.
    pub metrics: MetricSet,
    pub entities: Vec<EntitySample>,
    pub processes: Vec<ProcessSample>,
    pub collectors: Vec<CollectorState>,
}

impl Sample {
    /// Turn one round of collector reports into a sample.
    ///
    /// Process entities are dropped after the busiest `max_processes` are
    /// summarized; every other entity is kept whole, because there are tens of
    /// them rather than thousands.
    pub fn from_reports(reports: &[CollectorReport], max_processes: usize) -> Self {
        let wall_unix_ms = reports.first().map(|r| r.observed_at.unix_ms).unwrap_or(0);
        let monotonic_ns = reports
            .first()
            .map(|r| r.observed_at.monotonic_ns)
            .unwrap_or(0);

        let mut metrics = MetricSet::new();
        let mut entities = Vec::new();
        let mut processes = Vec::new();
        let mut collectors = Vec::new();

        for report in reports {
            for (id, observation) in report.metrics.iter() {
                metrics.insert(id.clone(), observation.clone());
            }
            for entity in &report.entities {
                if entity.id.kind == EntityKind::Process {
                    processes.push(process_sample(&entity.id.key, &entity.metrics));
                } else {
                    entities.push(EntitySample {
                        kind: entity.id.kind,
                        key: entity.id.key.clone(),
                        metrics: entity.metrics.clone(),
                    });
                }
            }
            collectors.push(CollectorState {
                collector: report.collector.clone(),
                health: report.health.clone(),
            });
        }

        // Busiest first, and memory as the tie-break so a machine that is
        // stalled rather than busy still records the processes holding it.
        processes.sort_by(|left, right| {
            right
                .cpu_utilization
                .unwrap_or(0.0)
                .total_cmp(&left.cpu_utilization.unwrap_or(0.0))
                .then(right.memory_resident.cmp(&left.memory_resident))
                .then(left.pid.cmp(&right.pid))
        });
        processes.truncate(max_processes);

        Self {
            wall_unix_ms,
            monotonic_ns,
            rounds: 1,
            metrics,
            entities,
            processes,
            collectors,
        }
    }

    /// Every observation in the sample, system-level and per-entity, for
    /// coverage accounting.
    pub fn observations(&self) -> impl Iterator<Item = (&MetricId, &Observation)> {
        self.metrics.iter().chain(
            self.entities
                .iter()
                .flat_map(|entity| entity.metrics.iter()),
        )
    }

    /// The numeric value of a system-level metric, when it has one.
    pub fn value_of(&self, id: &MetricId) -> Option<f64> {
        self.metrics.get(id).and_then(Observation::as_f64)
    }
}

fn process_sample(key: &str, metrics: &MetricSet) -> ProcessSample {
    let text = |name: &str| {
        MetricId::new(name)
            .ok()
            .and_then(|id| metrics.get(&id).and_then(Observation::as_text))
            .map(str::to_string)
    };
    let number = |name: &str| {
        MetricId::new(name)
            .ok()
            .and_then(|id| metrics.get(&id).and_then(Observation::as_f64))
    };
    ProcessSample {
        pid: key.parse().unwrap_or(0),
        name: text("process.name").unwrap_or_else(|| format!("[{key}]")),
        command_line: text("process.command_line"),
        user: text("process.user"),
        cpu_utilization: number("process.cpu.utilization"),
        memory_resident: number("process.memory.resident").map(|bytes| bytes as u64),
    }
}

/// Collapses several raw rounds into one stored sample.
///
/// Numbers are averaged, because that is what a fixed-resolution history of a
/// faster sampler means. Identities and enumerated states take the newest
/// reading, because averaging a process name is meaningless. A metric that had
/// no value in any round keeps the most recent reason it had none, so the
/// downsample cannot manufacture a zero out of a gap.
#[derive(Debug, Default)]
pub struct Downsampler {
    rounds: Vec<Sample>,
}

impl Downsampler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, sample: Sample) {
        self.rounds.push(sample);
    }

    pub fn is_empty(&self) -> bool {
        self.rounds.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rounds.len()
    }

    pub fn clear(&mut self) {
        self.rounds.clear();
    }

    /// Produce the stored sample and start a new bucket.
    pub fn take(&mut self) -> Option<Sample> {
        let rounds = std::mem::take(&mut self.rounds);
        let count = rounds.len();
        let last = rounds.last()?.clone();
        let mut merged = Sample {
            wall_unix_ms: last.wall_unix_ms,
            monotonic_ns: last.monotonic_ns,
            rounds: count as u32,
            metrics: average_sets(rounds.iter().map(|sample| &sample.metrics)),
            entities: Vec::new(),
            processes: last.processes.clone(),
            collectors: last.collectors.clone(),
        };
        for entity in &last.entities {
            let sets = rounds.iter().filter_map(|sample| {
                sample
                    .entities
                    .iter()
                    .find(|candidate| candidate.kind == entity.kind && candidate.key == entity.key)
                    .map(|found| &found.metrics)
            });
            merged.entities.push(EntitySample {
                kind: entity.kind,
                key: entity.key.clone(),
                metrics: average_sets(sets),
            });
        }
        Some(merged)
    }
}

/// The narrowest scalar type that can carry every reading in a bucket. A
/// mean is written back in the same type the collector reported, so a byte
/// count does not come out of the store as a float.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ScalarKind {
    Unsigned,
    Signed,
    Float,
}

#[derive(Debug)]
struct Mean {
    total: f64,
    count: u32,
    kind: ScalarKind,
}

impl Mean {
    fn add(&mut self, value: f64, kind: ScalarKind) {
        self.total += value;
        self.count += 1;
        self.kind = self.kind.max(kind);
    }

    fn observation(&self) -> Observation {
        let mean = self.total / self.count as f64;
        match self.kind {
            ScalarKind::Unsigned => {
                Observation::Value(MetricScalar::Unsigned(mean.round().max(0.0) as u64))
            }
            ScalarKind::Signed => Observation::Value(MetricScalar::Signed(mean.round() as i64)),
            ScalarKind::Float => Observation::float(mean),
        }
    }
}

fn average_sets<'a>(sets: impl Iterator<Item = &'a MetricSet>) -> MetricSet {
    let mut numeric: BTreeMap<MetricId, Mean> = BTreeMap::new();
    let mut fallback: BTreeMap<MetricId, Observation> = BTreeMap::new();

    for set in sets {
        for (id, observation) in set.iter() {
            let numbered = match observation {
                Observation::Value(MetricScalar::Unsigned(value)) => {
                    Some((*value as f64, ScalarKind::Unsigned))
                }
                Observation::Value(MetricScalar::Signed(value)) => {
                    Some((*value as f64, ScalarKind::Signed))
                }
                Observation::Value(MetricScalar::Float(value)) => Some((*value, ScalarKind::Float)),
                _ => None,
            };
            match numbered {
                Some((value, kind)) => numeric
                    .entry(id.clone())
                    .or_insert(Mean {
                        total: 0.0,
                        count: 0,
                        kind,
                    })
                    .add(value, kind),
                // A name, a state, a boolean, a stale value, or any of the
                // four ways of having no reading: keep the newest as it was.
                None => {
                    fallback.insert(id.clone(), observation.clone());
                }
            }
        }
    }

    let mut merged = MetricSet::new();
    for (id, observation) in fallback {
        // A metric that produced a value in any round is represented by that
        // average, not by the round where it was missing.
        if !numeric.contains_key(&id) {
            merged.insert(id, observation);
        }
    }
    for (id, mean) in numeric {
        merged.insert(id, mean.observation());
    }
    merged
}

/// How many observations of each state a metric produced over a range.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CoverageCounts {
    pub value: u64,
    pub stale: u64,
    pub unknown: u64,
    pub unsupported: u64,
    pub permission_denied: u64,
}

impl CoverageCounts {
    pub fn record(&mut self, state: ObservationState) {
        match state {
            ObservationState::Value => self.value += 1,
            ObservationState::Stale => self.stale += 1,
            ObservationState::Unknown => self.unknown += 1,
            ObservationState::Unsupported => self.unsupported += 1,
            ObservationState::PermissionDenied => self.permission_denied += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.value + self.stale + self.unknown + self.unsupported + self.permission_denied
    }
}

/// Why a stretch of time has no samples in it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum GapReason {
    /// The service was not running. Recorded when it starts and finds the last
    /// sample older than one interval.
    ServiceStopped,
    /// The service was running but a round took longer than the interval, or
    /// the machine was suspended.
    MissedCadence,
    /// Records were dropped from the end of the log because the previous run
    /// was interrupted mid-write.
    InterruptedWrite,
    /// Retention removed the samples that used to cover this stretch.
    Retention,
}

/// A stretch of wall-clock time the history does not cover.
///
/// Gaps are recorded rather than filled, because a chart drawn through a gap
/// says the machine was idle when the truth is that nobody was watching.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    pub from_unix_ms: u64,
    pub to_unix_ms: u64,
    #[serde(flatten)]
    pub reason: GapReason,
}

impl Gap {
    pub fn duration_ms(&self) -> u64 {
        self.to_unix_ms.saturating_sub(self.from_unix_ms)
    }
}

/// What the history log holds, in the order it was written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "record")]
pub enum HistoryRecord {
    Sample(Box<Sample>),
    Gap(Gap),
}

impl HistoryRecord {
    pub fn wall_unix_ms(&self) -> u64 {
        match self {
            HistoryRecord::Sample(sample) => sample.wall_unix_ms,
            HistoryRecord::Gap(gap) => gap.to_unix_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{
        CollectorId, Entity, EntityId, Timestamp, UnknownReason, UnsupportedReason,
    };

    fn id(raw: &str) -> MetricId {
        MetricId::new(raw).unwrap()
    }

    fn process_entity(pid: u32, cpu: f64, resident: u64) -> Entity {
        let mut metrics = MetricSet::new();
        metrics.insert(id("process.name"), Observation::text(format!("app{pid}")));
        metrics.insert(id("process.cpu.utilization"), Observation::float(cpu));
        metrics.insert(
            id("process.memory.resident"),
            Observation::unsigned(resident),
        );
        Entity::new(EntityId::new(EntityKind::Process, pid.to_string()), metrics)
    }

    fn round(at_ms: u64, busy: f64) -> Vec<CollectorReport> {
        let mut cpu = CollectorReport::new(
            CollectorId::new("linux.cpu").unwrap(),
            Timestamp {
                unix_ms: at_ms,
                monotonic_ns: at_ms * 1_000_000,
            },
        );
        cpu.metrics
            .insert(id("cpu.utilization.busy"), Observation::float(busy));
        cpu.metrics.insert(
            id("cpu.temperature"),
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "k10temp exposes Tctl only".into(),
            }),
        );
        let mut pressure_metrics = MetricSet::new();
        pressure_metrics.insert(id("pressure.some.avg10"), Observation::float(busy * 10.0));
        cpu.entities.push(Entity::new(
            EntityId::new(EntityKind::PressureResource, "cpu"),
            pressure_metrics,
        ));

        let mut processes = CollectorReport::new(
            CollectorId::new("linux.process").unwrap(),
            Timestamp {
                unix_ms: at_ms,
                monotonic_ns: at_ms * 1_000_000,
            },
        );
        for pid in 1..=5u32 {
            processes
                .entities
                .push(process_entity(pid, pid as f64 / 10.0, pid as u64 * 1024));
        }
        vec![cpu, processes]
    }

    #[test]
    fn a_sample_keeps_only_the_busiest_processes() {
        let sample = Sample::from_reports(&round(1_000, 0.5), 2);
        assert_eq!(sample.processes.len(), 2);
        assert_eq!(sample.processes[0].pid, 5);
        assert_eq!(sample.processes[1].pid, 4);
        assert_eq!(sample.processes[0].name, "app5");
    }

    #[test]
    fn a_sample_keeps_the_non_process_entities_whole() {
        let sample = Sample::from_reports(&round(1_000, 0.5), 2);
        assert_eq!(sample.entities.len(), 1);
        assert_eq!(sample.entities[0].kind, EntityKind::PressureResource);
        assert_eq!(sample.entities[0].key, "cpu");
    }

    #[test]
    fn a_sample_records_both_clocks_and_every_collector() {
        let sample = Sample::from_reports(&round(1_700, 0.5), 10);
        assert_eq!(sample.wall_unix_ms, 1_700);
        assert_eq!(sample.monotonic_ns, 1_700_000_000);
        assert_eq!(sample.collectors.len(), 2);
        assert_eq!(sample.rounds, 1);
    }

    #[test]
    fn an_unsupported_metric_stays_unsupported_in_the_stored_sample() {
        let sample = Sample::from_reports(&round(1_000, 0.0), 2);
        assert_eq!(sample.value_of(&id("cpu.utilization.busy")), Some(0.0));
        assert_eq!(sample.value_of(&id("cpu.temperature")), None);
        assert!(matches!(
            sample.metrics.get(&id("cpu.temperature")),
            Some(Observation::Unsupported(_))
        ));
    }

    #[test]
    fn a_sample_round_trips_through_json() {
        let sample = Sample::from_reports(&round(1_000, 0.25), 3);
        let encoded = serde_json::to_string(&sample).unwrap();
        let decoded: Sample = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, sample);
    }

    #[test]
    fn downsampling_averages_the_numbers_it_collapses() {
        let mut downsampler = Downsampler::new();
        for (index, busy) in [0.2, 0.4, 0.6].into_iter().enumerate() {
            downsampler.push(Sample::from_reports(
                &round(1_000 + index as u64 * 1_000, busy),
                3,
            ));
        }
        assert_eq!(downsampler.len(), 3);
        let merged = downsampler.take().unwrap();
        assert_eq!(merged.rounds, 3);
        assert_eq!(merged.wall_unix_ms, 3_000);
        let busy = merged.value_of(&id("cpu.utilization.busy")).unwrap();
        assert!((busy - 0.4).abs() < 1e-9, "{busy}");
        let pressure = merged.entities[0]
            .metrics
            .get(&id("pressure.some.avg10"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((pressure - 4.0).abs() < 1e-9, "{pressure}");
        assert!(downsampler.is_empty());
    }

    #[test]
    fn downsampling_never_turns_a_missing_reading_into_a_zero() {
        let mut downsampler = Downsampler::new();
        for index in 0..3 {
            downsampler.push(Sample::from_reports(&round(1_000 * index, 0.5), 3));
        }
        let merged = downsampler.take().unwrap();
        assert!(matches!(
            merged.metrics.get(&id("cpu.temperature")),
            Some(Observation::Unsupported(_))
        ));
        assert_eq!(merged.value_of(&id("cpu.temperature")), None);
    }

    #[test]
    fn a_metric_that_had_a_value_in_one_round_is_averaged_over_the_rounds_that_had_one() {
        let mut downsampler = Downsampler::new();
        let mut first = Sample::from_reports(&round(1_000, 0.5), 1);
        first.metrics.insert(
            id("cpu.load.1m"),
            Observation::Unknown(UnknownReason::NotYetSampled),
        );
        let mut second = Sample::from_reports(&round(2_000, 0.5), 1);
        second
            .metrics
            .insert(id("cpu.load.1m"), Observation::float(2.0));
        downsampler.push(first);
        downsampler.push(second);
        let merged = downsampler.take().unwrap();
        assert_eq!(merged.value_of(&id("cpu.load.1m")), Some(2.0));
    }

    #[test]
    fn an_empty_bucket_produces_nothing_rather_than_an_empty_sample() {
        assert!(Downsampler::new().take().is_none());
    }

    #[test]
    fn coverage_counts_every_state_separately() {
        let mut counts = CoverageCounts::default();
        counts.record(ObservationState::Value);
        counts.record(ObservationState::Value);
        counts.record(ObservationState::Unsupported);
        assert_eq!(counts.value, 2);
        assert_eq!(counts.unsupported, 1);
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn a_gap_round_trips_with_its_reason() {
        let gap = Gap {
            from_unix_ms: 10,
            to_unix_ms: 40,
            reason: GapReason::ServiceStopped,
        };
        let encoded = serde_json::to_string(&gap).unwrap();
        assert!(encoded.contains("service_stopped"));
        assert_eq!(serde_json::from_str::<Gap>(&encoded).unwrap(), gap);
        assert_eq!(gap.duration_ms(), 30);
    }
}
