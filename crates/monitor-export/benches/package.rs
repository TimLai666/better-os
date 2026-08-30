//! How long an export takes over a large range, and how big it comes out.
//!
//! The number that matters is whether a user who asks for six hours waits long
//! enough to think it has hung. No harness crate: the workspace has no network
//! access to add one, and the answer is coarse.

use std::time::{Duration, Instant};

use monitor_core::{CollectorHealth, CollectorId, EntityKind, MetricId, MetricSet, Observation};
use monitor_export::{ExportRequest, preview, write_package};
use monitor_store::{
    CollectorState, EntitySample, HistoryStore, Inventory, InventoryEntry, ProcessSample,
    RetentionPolicy, Sample, TimeRange,
};

fn metric(raw: &str) -> MetricId {
    MetricId::new(raw).unwrap()
}

fn sample(at_ms: u64) -> Sample {
    let mut metrics = MetricSet::new();
    for name in [
        "cpu.utilization.busy",
        "cpu.utilization.system",
        "cpu.load.1m",
        "memory.available",
        "memory.swap.used",
        "storage.read.bytes.rate",
        "network.receive.bytes.rate",
    ] {
        metrics.insert(metric(name), Observation::float((at_ms % 1_000) as f64));
    }
    metrics.insert(
        metric("cpu.temperature"),
        Observation::Unsupported(monitor_core::UnsupportedReason::NotReported {
            detail: "k10temp exposes Tctl only".into(),
        }),
    );

    let mut entities = Vec::new();
    for resource in ["cpu", "memory", "io"] {
        let mut set = MetricSet::new();
        set.insert(metric("pressure.some.avg10"), Observation::float(1.5));
        set.insert(metric("pressure.full.avg10"), Observation::float(0.2));
        entities.push(EntitySample {
            kind: EntityKind::PressureResource,
            key: resource.to_string(),
            metrics: set,
        });
    }

    let processes = (0..monitor_store::DEFAULT_TRACKED_PROCESSES)
        .map(|index| ProcessSample {
            pid: 1_000 + index as u32,
            name: format!("worker{index}"),
            command_line: Some(format!(
                "/home/tim/.local/bin/worker{index} --token ghp_S3cretT0kenValue{index:06}"
            )),
            user: Some("tim".into()),
            cpu_utilization: Some(index as f64 / 10.0),
            memory_resident: Some(index as u64 * 1_048_576),
        })
        .collect();

    Sample {
        wall_unix_ms: at_ms,
        monotonic_ns: at_ms * 1_000_000,
        rounds: 5,
        metrics,
        entities,
        processes,
        collectors: [
            "linux.cpu",
            "linux.memory",
            "linux.pressure",
            "linux.process",
        ]
        .into_iter()
        .map(|id| CollectorState {
            collector: CollectorId::new(id).unwrap(),
            health: CollectorHealth::Healthy,
        })
        .collect(),
    }
}

fn inventory(at_ms: u64) -> Inventory {
    let mut inventory = Inventory::new(at_ms);
    inventory
        .insert("os.name", InventoryEntry::public("Zorin OS 18"))
        .insert(
            "kernel.release",
            InventoryEntry::public("6.11.0-19-generic"),
        )
        .insert("session.user", InventoryEntry::personal("tim"))
        .insert("session.home", InventoryEntry::personal("/home/tim"))
        .insert("host.name", InventoryEntry::personal("workshop"))
        .insert(
            "network.eth0.mac",
            InventoryEntry::identifier("aa:bb:cc:dd:ee:ff"),
        );
    inventory
}

fn report(label: &str, samples: Vec<Duration>) {
    let mut samples = samples;
    samples.sort();
    let at = |fraction: f64| samples[((samples.len() as f64 - 1.0) * fraction).round() as usize];
    println!(
        "{label:<44} p50 {:>9.1} ms   p95 {:>9.1} ms",
        at(0.5).as_secs_f64() * 1_000.0,
        at(0.95).as_secs_f64() * 1_000.0
    );
}

fn main() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let policy = RetentionPolicy {
        window_seconds: 86_400,
        disk_budget_bytes: 512 * 1024 * 1024,
        ..RetentionPolicy::default()
    };
    let mut store = HistoryStore::open(directory.path().join("store"), policy).expect("a store");

    let base = 1_700_000_000_000u64;
    let total = 6 * 60 * 60 / 5;
    store
        .record_inventory(inventory(base))
        .expect("an inventory");
    let mut second = inventory(base + 1_000);
    second.insert("kernel.release", InventoryEntry::public("6.14.0-2-generic"));
    store.record_inventory(second).expect("a second inventory");
    for index in 0..total {
        store
            .record_sample(sample(base + index * 5_000))
            .expect("a durable append");
    }
    for index in 0..20u64 {
        let at = base + index * 60_000;
        store
            .record_incident(monitor_store::Incident {
                id: index + 1,
                marked_at_unix_ms: at,
                monotonic_ns: at * 1_000_000,
                note: Some("the system was just slow".into()),
                window: monitor_store::IncidentWindow::default(),
                baseline: Default::default(),
                snapshot: Box::new(sample(at)),
                about_pid: Some(1_001),
            })
            .expect("an incident");
    }

    println!("Better Monitor export package");
    println!("six hours at five-second resolution: {total} samples, 20 incidents");

    let mut previews = Vec::new();
    for _ in 0..5 {
        let target = tempfile::tempdir().expect("a temporary directory");
        let request = ExportRequest::new(TimeRange::all(), target.path().join("package"));
        let started = Instant::now();
        let report = preview(&store, &request, base).expect("a preview");
        previews.push(started.elapsed());
        std::hint::black_box(report);
    }
    report("preview the whole six hours", previews);

    let mut writes = Vec::new();
    let mut bytes = 0u64;
    for _ in 0..5 {
        let target = tempfile::tempdir().expect("a temporary directory");
        let destination = target.path().join("package");
        let request = ExportRequest::new(TimeRange::all(), &destination);
        let started = Instant::now();
        let outcome = write_package(&store, &request, base).expect("a package");
        writes.push(started.elapsed());
        bytes = directory_size(&destination);
        std::hint::black_box(outcome);
    }
    report("write the whole six hours", writes);
    println!(
        "{:<44} {:.2} MiB",
        "package size",
        bytes as f64 / (1024.0 * 1024.0)
    );

    let mut narrow = Vec::new();
    for _ in 0..20 {
        let target = tempfile::tempdir().expect("a temporary directory");
        let request = ExportRequest::new(
            TimeRange::last(900, base + total * 5_000),
            target.path().join("package"),
        );
        let started = Instant::now();
        let outcome = write_package(&store, &request, base).expect("a package");
        narrow.push(started.elapsed());
        std::hint::black_box(outcome);
    }
    report("write the last fifteen minutes", narrow);
}

fn directory_size(path: &std::path::Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(metadata) = entry.metadata() {
                total += metadata.len();
            }
        }
    }
    total
}
