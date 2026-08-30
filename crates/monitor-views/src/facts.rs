//! What one process looks like once a report has been read.
//!
//! Every view above this line works from `ProcessFacts` rather than from a
//! `MetricSet`, so the metric names appear exactly once in the codebase above
//! the collectors. A renamed metric breaks here, loudly, instead of silently
//! turning a column blank.

use crate::field::{self, Field};
use monitor_core::{CollectorReport, Entity, EntityKind, MetricId, ProcessTarget, UnknownReason};

fn id(raw: &str) -> MetricId {
    MetricId::new(raw).expect("a view metric id must be well formed")
}

/// One process, with every column already narrowed and every non-value
/// preserved as the reason it has none.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessFacts {
    pub pid: u32,
    pub parent_pid: Field<u64>,
    pub name: Field<String>,
    pub state: Field<String>,
    pub user: Field<String>,
    pub uid: Field<u64>,
    pub cpu_utilization: Field<f64>,
    pub cpu_time_total: Field<f64>,
    pub memory_resident: Field<u64>,
    pub memory_swap: Field<u64>,
    pub memory_virtual: Field<u64>,
    pub read_rate: Field<f64>,
    pub write_rate: Field<f64>,
    pub threads: Field<u64>,
    pub file_descriptors: Field<u64>,
    pub start_time: Field<f64>,
    pub runtime: Field<f64>,
    pub nice: Field<i64>,
    pub priority: Field<i64>,
    pub cgroup: Field<String>,
    pub command_line: Field<String>,
}

impl ProcessFacts {
    /// Read one process entity. Returns `None` for an entity whose key is not
    /// a PID, which would mean the report was not a process report.
    pub fn from_entity(entity: &Entity) -> Option<Self> {
        if entity.id.kind != EntityKind::Process {
            return None;
        }
        let pid = entity.id.key.parse::<u32>().ok()?;
        let metrics = &entity.metrics;
        Some(Self {
            pid,
            parent_pid: field::unsigned(metrics, &id("process.parent.pid")),
            name: field::text(metrics, &id("process.name")),
            state: field::text(metrics, &id("process.state")),
            user: field::text(metrics, &id("process.user")),
            uid: field::unsigned(metrics, &id("process.uid")),
            cpu_utilization: field::number(metrics, &id("process.cpu.utilization")),
            cpu_time_total: field::number(metrics, &id("process.cpu.time.total")),
            memory_resident: field::unsigned(metrics, &id("process.memory.resident")),
            memory_swap: field::unsigned(metrics, &id("process.memory.swap")),
            memory_virtual: field::unsigned(metrics, &id("process.memory.virtual")),
            read_rate: field::number(metrics, &id("process.io.read.bytes.rate")),
            write_rate: field::number(metrics, &id("process.io.write.bytes.rate")),
            threads: field::unsigned(metrics, &id("process.threads")),
            file_descriptors: field::unsigned(metrics, &id("process.file_descriptors")),
            start_time: field::number(metrics, &id("process.start_time")),
            runtime: field::number(metrics, &id("process.runtime")),
            nice: field::signed(metrics, &id("process.nice")),
            priority: field::signed(metrics, &id("process.priority")),
            cgroup: field::text(metrics, &id("process.cgroup")),
            command_line: field::text(metrics, &id("process.command_line")),
        })
    }

    /// Every process in a collector report, in the order the collector
    /// enumerated them.
    pub fn from_report(report: &CollectorReport) -> Vec<Self> {
        report
            .entities_of(EntityKind::Process)
            .filter_map(Self::from_entity)
            .collect()
    }

    /// The display name, falling back to the PID when the kernel's name was
    /// not readable. A blank cell would be worse than an honest `[1234]`.
    pub fn display_name(&self) -> String {
        match self.name.any_value() {
            Some(name) if !name.trim().is_empty() => name.clone(),
            _ => format!("[{}]", self.pid),
        }
    }

    pub fn parent(&self) -> Option<u32> {
        self.parent_pid
            .copied()
            .and_then(|pid| u32::try_from(pid).ok())
    }

    /// The target a process action would apply to.
    ///
    /// The start time doubles as the reuse token. It is in seconds and is
    /// rounded to hundredths, which is the resolution `/proc` reports it at,
    /// so two different processes on one PID cannot round to the same token
    /// unless they started within a scheduler tick of each other.
    pub fn action_target(&self) -> ProcessTarget {
        let mut target = ProcessTarget::new(self.pid, self.display_name());
        if let Some(uid) = self.uid.copied().and_then(|uid| u32::try_from(uid).ok()) {
            target = target.owned_by(uid);
        }
        if let Some(start) = self.start_time.copied() {
            target = target.started_at((start * 100.0) as u64);
        }
        if let Some(nice) = self.nice.copied().and_then(|nice| i32::try_from(nice).ok()) {
            target = target.with_nice(nice);
        }
        target
    }

    /// Whether the process is stopped, which is what decides between offering
    /// pause and offering resume.
    pub fn is_paused(&self) -> bool {
        matches!(
            self.state.any_value().map(String::as_str),
            Some("stopped") | Some("tracing-stop")
        )
    }

    /// A synthetic process, for tests and benchmarks that need a table
    /// without a kernel underneath it.
    pub fn synthetic(pid: u32, name: &str) -> Self {
        Self {
            pid,
            parent_pid: Field::Value(1),
            name: Field::Value(name.to_string()),
            state: Field::Value("sleeping".into()),
            user: Field::Value("tim".into()),
            uid: Field::Value(1000),
            cpu_utilization: Field::Value(0.0),
            cpu_time_total: Field::Value(0.0),
            memory_resident: Field::Value(0),
            memory_swap: Field::Value(0),
            memory_virtual: Field::Value(0),
            read_rate: Field::Value(0.0),
            write_rate: Field::Value(0.0),
            threads: Field::Value(1),
            file_descriptors: Field::Value(3),
            start_time: Field::Value(1_700_000_000.0),
            runtime: Field::Value(1.0),
            nice: Field::Value(0),
            priority: Field::Value(20),
            cgroup: Field::Unknown(UnknownReason::NotYetSampled),
            command_line: Field::NotCollected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{
        CollectorId, EntityId, MetricScalar, MetricSet, Observation, Timestamp, UnsupportedReason,
    };

    fn entity(pid: u32) -> Entity {
        let mut metrics = MetricSet::new();
        metrics.insert(id("process.pid"), Observation::unsigned(pid as u64));
        metrics.insert(id("process.parent.pid"), Observation::unsigned(1));
        metrics.insert(id("process.name"), Observation::text("gedit"));
        metrics.insert(id("process.state"), Observation::text("sleeping"));
        metrics.insert(id("process.uid"), Observation::unsigned(1000));
        metrics.insert(id("process.user"), Observation::text("tim"));
        metrics.insert(
            id("process.start_time"),
            Observation::float(1_700_000_000.5),
        );
        metrics.insert(
            id("process.nice"),
            Observation::Value(MetricScalar::Signed(5)),
        );
        metrics.insert(
            id("process.file_descriptors"),
            Observation::PermissionDenied {
                path: format!("/proc/{pid}/fd"),
            },
        );
        metrics.insert(
            id("process.command_line"),
            Observation::Unsupported(UnsupportedReason::PolicyWithheld {
                policy: "command lines".into(),
            }),
        );
        Entity::new(EntityId::new(EntityKind::Process, pid.to_string()), metrics)
    }

    #[test]
    fn a_process_entity_becomes_typed_columns_without_losing_a_refusal() {
        let facts = ProcessFacts::from_entity(&entity(4242)).expect("a process entity");
        assert_eq!(facts.pid, 4242);
        assert_eq!(facts.display_name(), "gedit");
        assert_eq!(facts.parent(), Some(1));
        assert_eq!(facts.nice, Field::Value(5));
        assert!(matches!(
            facts.file_descriptors,
            Field::PermissionDenied { .. }
        ));
        assert!(matches!(facts.command_line, Field::Unsupported(_)));
        // Never collected at all, as opposed to collected and unreadable.
        assert_eq!(facts.threads, Field::NotCollected);
    }

    #[test]
    fn an_unnamed_process_is_shown_by_pid_rather_than_as_a_blank_row() {
        let mut raw = entity(77);
        raw.metrics.insert(
            id("process.name"),
            Observation::Unknown(UnknownReason::EntityDisappeared),
        );
        let facts = ProcessFacts::from_entity(&raw).unwrap();
        assert_eq!(facts.display_name(), "[77]");
    }

    #[test]
    fn an_action_target_carries_ownership_and_a_reuse_token() {
        let facts = ProcessFacts::from_entity(&entity(4242)).unwrap();
        let target = facts.action_target();
        assert_eq!(target.pid, 4242);
        assert_eq!(target.owner_uid, Some(1000));
        assert_eq!(target.current_nice, Some(5));
        assert_eq!(target.start_token, Some(170_000_000_050));
    }

    #[test]
    fn a_report_of_another_kind_yields_no_processes() {
        let mut report = CollectorReport::new(
            CollectorId::new("linux.cpu").unwrap(),
            Timestamp {
                unix_ms: 1,
                monotonic_ns: 0,
            },
        );
        report.entities.push(Entity::new(
            EntityId::new(EntityKind::LogicalCpu, "0"),
            MetricSet::new(),
        ));
        assert!(ProcessFacts::from_report(&report).is_empty());
    }

    #[test]
    fn a_stopped_process_is_offered_resume_rather_than_pause() {
        let mut raw = entity(9);
        raw.metrics
            .insert(id("process.state"), Observation::text("stopped"));
        assert!(ProcessFacts::from_entity(&raw).unwrap().is_paused());
        assert!(!ProcessFacts::from_entity(&entity(9)).unwrap().is_paused());
    }
}
