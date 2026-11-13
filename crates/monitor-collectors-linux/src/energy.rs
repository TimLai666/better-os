//! What each process costs the battery, estimated.
//!
//! Linux has no per-process energy counter an unprivileged program can read.
//! What it does have is the battery's own draw (`power_now`, or current times
//! voltage) and each process's processor and graphics time. While the machine
//! runs on its battery, the draw is shared out in proportion to that time:
//! a process that used a third of all the processor and graphics time used in
//! the interval is given a third of the draw.
//!
//! That is an estimate and is labelled as one. It gives no share of the
//! screen's or the radios' power to anyone in particular, so it answers
//! "which program is making the battery go down fastest", not "how many watts
//! does this program draw". On mains power the battery's draw says nothing
//! about the machine's, so nothing is estimated and the reading says why.

use crate::catalog::{derived_source, gauge, metric_id};
use monitor_core::{
    CollectorReport, EntityKind, MetricDescriptor, MetricId, Observation, Unit, UnknownReason,
    UnsupportedReason,
};

pub fn descriptors() -> Vec<MetricDescriptor> {
    vec![gauge(
        "process.energy.power",
        Unit::Watts,
        derived_source("battery draw shared out by each process's processor and graphics time"),
        "estimated share of the battery draw, only while on battery",
    )]
}

/// The weight a process carries: processor time as a fraction of one logical
/// processor plus graphics busy share.
fn weight(metrics: &monitor_core::MetricSet) -> Option<f64> {
    let cpu = metrics
        .get(&metric_id("process.cpu.utilization"))
        .and_then(Observation::as_f64)?;
    let gpu = metrics
        .get(&metric_id("process.gpu.utilization"))
        .and_then(Observation::as_f64)
        .unwrap_or(0.0);
    Some(cpu.max(0.0) + gpu.max(0.0))
}

/// Write `process.energy.power` into every process of the round.
pub fn estimate(processes: &mut CollectorReport, battery: Option<&CollectorReport>) {
    let id: MetricId = metric_id("process.energy.power");
    let on_battery = battery.and_then(|report| {
        report
            .metrics
            .get(&metric_id("power.on_battery"))
            .and_then(Observation::value)
            .and_then(|value| match value {
                monitor_core::MetricScalar::Bool(on) => Some(*on),
                _ => None,
            })
    });
    let draw = battery.and_then(|report| {
        report
            .metrics
            .get(&metric_id("power.battery.draw"))
            .and_then(Observation::as_f64)
    });
    let reason = match (on_battery, draw) {
        (None, _) => Some(Observation::Unsupported(UnsupportedReason::NotReported {
            detail: "this machine has no battery".into(),
        })),
        (Some(false), _) => Some(Observation::Unsupported(UnsupportedReason::NotReported {
            detail: "estimated only while running on the battery".into(),
        })),
        (Some(true), None) => Some(Observation::Unsupported(UnsupportedReason::NotReported {
            detail: "the battery does not report its draw".into(),
        })),
        (Some(true), Some(_)) => None,
    };
    if let Some(reason) = reason {
        for entity in processes
            .entities
            .iter_mut()
            .filter(|e| e.id.kind == EntityKind::Process)
        {
            entity.metrics.insert(id.clone(), reason.clone());
        }
        return;
    }
    let draw = draw.unwrap_or(0.0);
    let total: f64 = processes
        .entities
        .iter()
        .filter(|e| e.id.kind == EntityKind::Process)
        .filter_map(|e| weight(&e.metrics))
        .sum();
    for entity in processes
        .entities
        .iter_mut()
        .filter(|e| e.id.kind == EntityKind::Process)
    {
        let value = match weight(&entity.metrics) {
            Some(own) if total > 0.0 => Observation::float(draw * own / total),
            Some(_) => Observation::float(0.0),
            None => Observation::Unknown(UnknownReason::NotYetSampled),
        };
        entity.metrics.insert(id.clone(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{CollectorId, Entity, EntityId, MetricSet, Timestamp};

    fn at() -> Timestamp {
        Timestamp {
            unix_ms: 1,
            monotonic_ns: 1,
        }
    }

    fn process(pid: u32, cpu: f64, gpu: Option<f64>) -> Entity {
        let mut m = MetricSet::new();
        m.insert(
            metric_id("process.cpu.utilization"),
            Observation::float(cpu),
        );
        if let Some(gpu) = gpu {
            m.insert(
                metric_id("process.gpu.utilization"),
                Observation::float(gpu),
            );
        }
        Entity::new(EntityId::new(EntityKind::Process, pid.to_string()), m)
    }

    fn battery(on: bool, draw: f64) -> CollectorReport {
        let mut report = CollectorReport::new(CollectorId::new("linux.battery").unwrap(), at());
        report
            .metrics
            .insert(metric_id("power.on_battery"), Observation::boolean(on));
        report
            .metrics
            .insert(metric_id("power.battery.draw"), Observation::float(draw));
        report
    }

    fn power(report: &CollectorReport, pid: &str) -> Observation {
        report
            .entities
            .iter()
            .find(|e| e.id.key == pid)
            .unwrap()
            .metrics
            .get(&metric_id("process.energy.power"))
            .unwrap()
            .clone()
    }

    #[test]
    fn the_draw_is_shared_by_processor_and_graphics_time() {
        let mut processes = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at());
        processes.entities.push(process(1, 0.5, None));
        processes.entities.push(process(2, 0.25, Some(0.25)));
        processes.entities.push(process(3, 0.0, Some(0.0)));
        estimate(&mut processes, Some(&battery(true, 12.0)));
        assert_eq!(power(&processes, "1"), Observation::float(6.0));
        assert_eq!(power(&processes, "2"), Observation::float(6.0));
        assert_eq!(power(&processes, "3"), Observation::float(0.0));
    }

    #[test]
    fn nothing_is_estimated_on_mains_power_or_without_a_battery() {
        let mut processes = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at());
        processes.entities.push(process(1, 0.5, None));
        estimate(&mut processes, Some(&battery(false, 0.0)));
        assert!(matches!(
            power(&processes, "1"),
            Observation::Unsupported(_)
        ));
        estimate(&mut processes, None);
        assert!(matches!(
            power(&processes, "1"),
            Observation::Unsupported(_)
        ));
    }
}
