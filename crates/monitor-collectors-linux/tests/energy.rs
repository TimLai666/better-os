//! The battery-share estimate, from a battery report read off a
//! `power_supply` tree recorded on the owner's machine while it ran on its
//! battery, and a process table with known processor and graphics shares.

use monitor_collectors_linux::{BatteryCollector, Roots, energy};
use monitor_core::{
    CollectorId, CollectorReport, Entity, EntityId, EntityKind, MetricId, MetricSet, Observation,
    Timestamp,
};

const ON_BATTERY: &[(&str, &str)] = &[
    ("BAT1/type", "Battery"),
    ("BAT1/present", "1"),
    ("BAT1/status", "Discharging"),
    ("BAT1/capacity", "56"),
    ("BAT1/charge_now", "2302000"),
    ("BAT1/charge_full", "4079000"),
    ("BAT1/charge_full_design", "4680000"),
    ("BAT1/current_now", "2322000"),
    ("BAT1/voltage_now", "15405000"),
    ("BAT1/voltage_min_design", "16026000"),
    ("ACAD/type", "Mains"),
    ("ACAD/online", "0"),
];

fn at() -> Timestamp {
    Timestamp {
        unix_ms: 1_700_000_000_000,
        monotonic_ns: 1,
    }
}

fn process(pid: u32, cpu: f64, gpu: f64) -> Entity {
    let mut metrics = MetricSet::new();
    metrics.insert(
        MetricId::new("process.cpu.utilization").unwrap(),
        Observation::float(cpu),
    );
    metrics.insert(
        MetricId::new("process.gpu.utilization").unwrap(),
        Observation::float(gpu),
    );
    Entity::new(EntityId::new(EntityKind::Process, pid.to_string()), metrics)
}

#[test]
fn the_recorded_draw_is_shared_out_by_processor_and_graphics_time() {
    let root = tempfile::tempdir().unwrap();
    for (path, content) in ON_BATTERY {
        let path = root.path().join("sys/class/power_supply").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    let roots = Roots::at(root.path());
    let battery = BatteryCollector::new(roots.clone()).sample(&roots, at());
    let mut processes = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at());
    processes.entities.push(process(1, 0.75, 0.0));
    processes.entities.push(process(2, 0.0, 0.25));
    energy::estimate(&mut processes, Some(&battery));
    let draw = 2.322 * 15.405;
    let watts = |pid: &str| {
        processes
            .entities
            .iter()
            .find(|e| e.id.key == pid)
            .and_then(|e| {
                e.metrics
                    .get(&MetricId::new("process.energy.power").unwrap())
            })
            .and_then(|o| o.as_f64())
            .unwrap()
    };
    assert!((watts("1") - draw * 0.75).abs() < 1e-6);
    assert!((watts("2") - draw * 0.25).abs() < 1e-6);
}
