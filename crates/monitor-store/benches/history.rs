//! How fast the interim store is, so the ADR can compare it with something.
//!
//! No benchmark harness crate: the workspace has no network access to add one,
//! and what these numbers have to answer is coarse — can a five-second write
//! keep up, and does a six-hour query answer inside a frame. Each measurement
//! reports the median and the 95th percentile of repeated runs rather than one
//! stopwatch reading.

use std::time::{Duration, Instant};

use monitor_core::{
    CollectorId, CollectorReport, Entity, EntityId, EntityKind, MetricId, MetricSet, Observation,
    Timestamp, UnsupportedReason,
};
use monitor_store::{HistoryStore, RetentionPolicy, Sample, TimeRange};

fn metric(raw: &str) -> MetricId {
    MetricId::new(raw).unwrap()
}

/// A round shaped like a real one: six collectors, thirty-two logical CPUs,
/// three PSI resources, four devices, and four hundred processes.
fn round(at_ms: u64) -> Vec<CollectorReport> {
    let at = Timestamp {
        unix_ms: at_ms,
        monotonic_ns: at_ms * 1_000_000,
    };
    let mut reports = Vec::new();

    let mut cpu = CollectorReport::new(CollectorId::new("linux.cpu").unwrap(), at);
    for name in [
        "cpu.utilization.busy",
        "cpu.utilization.system",
        "cpu.utilization.iowait",
        "cpu.load.1m",
        "cpu.context_switches.rate",
    ] {
        cpu.metrics
            .insert(metric(name), Observation::float((at_ms % 97) as f64 / 97.0));
    }
    cpu.metrics.insert(
        metric("cpu.temperature"),
        Observation::Unsupported(UnsupportedReason::NotReported {
            detail: "k10temp exposes Tctl only".into(),
        }),
    );
    for core in 0..32 {
        let mut metrics = MetricSet::new();
        metrics.insert(
            metric("cpu.utilization.busy"),
            Observation::float((core as f64) / 32.0),
        );
        metrics.insert(
            metric("cpu.frequency.current"),
            Observation::unsigned(3_600_000_000),
        );
        cpu.entities.push(Entity::new(
            EntityId::new(EntityKind::LogicalCpu, core.to_string()),
            metrics,
        ));
    }
    reports.push(cpu);

    let mut memory = CollectorReport::new(CollectorId::new("linux.memory").unwrap(), at);
    for name in [
        "memory.total",
        "memory.available",
        "memory.cached",
        "memory.swap.used",
        "memory.faults.major.rate",
    ] {
        memory
            .metrics
            .insert(metric(name), Observation::unsigned(at_ms % 8_000_000_000));
    }
    reports.push(memory);

    let mut pressure = CollectorReport::new(CollectorId::new("linux.pressure").unwrap(), at);
    for resource in ["cpu", "memory", "io"] {
        let mut metrics = MetricSet::new();
        for name in ["pressure.some.avg10", "pressure.full.avg10"] {
            metrics.insert(metric(name), Observation::float((at_ms % 100) as f64));
        }
        pressure.entities.push(Entity::new(
            EntityId::new(EntityKind::PressureResource, resource),
            metrics,
        ));
    }
    reports.push(pressure);

    let mut storage = CollectorReport::new(CollectorId::new("linux.storage").unwrap(), at);
    for device in ["nvme0n1", "sda", "sdb", "dm-0"] {
        let mut metrics = MetricSet::new();
        for name in ["storage.read.bytes.rate", "storage.write.bytes.rate"] {
            metrics.insert(metric(name), Observation::float(at_ms as f64));
        }
        storage.entities.push(Entity::new(
            EntityId::new(EntityKind::BlockDevice, device),
            metrics,
        ));
    }
    reports.push(storage);

    let mut network = CollectorReport::new(CollectorId::new("linux.network").unwrap(), at);
    for link in ["eth0", "wlan0", "lo"] {
        let mut metrics = MetricSet::new();
        for name in ["network.receive.bytes.rate", "network.transmit.bytes.rate"] {
            metrics.insert(metric(name), Observation::float(at_ms as f64));
        }
        network.entities.push(Entity::new(
            EntityId::new(EntityKind::NetworkInterface, link),
            metrics,
        ));
    }
    reports.push(network);

    let mut processes = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at);
    for pid in 1..=400u32 {
        let mut metrics = MetricSet::new();
        metrics.insert(
            metric("process.name"),
            Observation::text(format!("worker{pid}")),
        );
        metrics.insert(metric("process.user"), Observation::text("tim"));
        metrics.insert(
            metric("process.cpu.utilization"),
            Observation::float((pid % 100) as f64 / 100.0),
        );
        metrics.insert(
            metric("process.memory.resident"),
            Observation::unsigned(pid as u64 * 1_048_576),
        );
        processes.entities.push(Entity::new(
            EntityId::new(EntityKind::Process, pid.to_string()),
            metrics,
        ));
    }
    reports.push(processes);

    reports
}

struct Percentiles {
    median: Duration,
    p95: Duration,
    worst: Duration,
}

fn percentiles(mut samples: Vec<Duration>) -> Percentiles {
    samples.sort();
    let index = |fraction: f64| {
        let position = ((samples.len() as f64 - 1.0) * fraction).round() as usize;
        samples[position]
    };
    Percentiles {
        median: index(0.5),
        p95: index(0.95),
        worst: *samples.last().unwrap(),
    }
}

fn report(label: &str, measured: Percentiles) {
    println!(
        "{label:<44} p50 {:>9.3} ms   p95 {:>9.3} ms   worst {:>9.3} ms",
        measured.median.as_secs_f64() * 1_000.0,
        measured.p95.as_secs_f64() * 1_000.0,
        measured.worst.as_secs_f64() * 1_000.0
    );
}

fn main() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let policy = RetentionPolicy {
        // A whole day, so retention never fires mid-benchmark and the numbers
        // measure the store rather than the compaction schedule.
        window_seconds: 86_400,
        disk_budget_bytes: 512 * 1024 * 1024,
        ..RetentionPolicy::default()
    };

    println!("Better Monitor interim history store");
    println!(
        "round shape: 6 collectors, 32 logical CPUs, 3 PSI resources, 8 devices, 400 processes"
    );

    // Building a stored sample out of a round is on the write path, so it is
    // measured separately from the append.
    let mut build = Vec::new();
    for index in 0..200u64 {
        let reports = round(1_700_000_000_000 + index * 5_000);
        let started = Instant::now();
        let sample = Sample::from_reports(&reports, monitor_store::DEFAULT_TRACKED_PROCESSES);
        build.push(started.elapsed());
        std::hint::black_box(sample);
    }
    report("sample built from one collector round", percentiles(build));

    let mut store = HistoryStore::open(directory.path(), policy).expect("an empty store");

    // Six hours at the default five-second resolution.
    let total_samples = 6 * 60 * 60 / 5;
    let base = 1_700_000_000_000u64;
    let mut writes = Vec::with_capacity(total_samples as usize);
    let started_all = Instant::now();
    for index in 0..total_samples {
        let sample = Sample::from_reports(
            &round(base + index * 5_000),
            monitor_store::DEFAULT_TRACKED_PROCESSES,
        );
        let started = Instant::now();
        store.record_sample(sample).expect("a durable append");
        writes.push(started.elapsed());
    }
    let wall = started_all.elapsed();
    report("durable append of one sample (fsync)", percentiles(writes));
    println!(
        "{:<44} {total_samples} samples in {:.2} s ({:.0} samples/s)",
        "six hours of history written",
        wall.as_secs_f64(),
        total_samples as f64 / wall.as_secs_f64()
    );

    let stats = store.stats();
    println!(
        "{:<44} {:.2} MiB on disk, {} samples retained",
        "store size after six hours",
        stats.bytes_on_disk as f64 / (1024.0 * 1024.0),
        stats.samples
    );

    let newest = stats.newest_sample_unix_ms.unwrap_or(base);
    for (label, seconds) in [
        ("query the last 5 minutes", 300u64),
        ("query the last hour", 3_600),
        ("query the whole six hours", 21_600),
    ] {
        let mut queries = Vec::new();
        for _ in 0..50 {
            let range = TimeRange::last(seconds, newest);
            let started = Instant::now();
            let slice = store.slice(range, usize::MAX);
            queries.push(started.elapsed());
            std::hint::black_box(slice);
        }
        report(label, percentiles(queries));
    }

    let mut coverage = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let counts = store.coverage(TimeRange::all());
        coverage.push(started.elapsed());
        std::hint::black_box(counts);
    }
    report("coverage over the whole six hours", percentiles(coverage));

    // Reopening is what a service restart and every CLI invocation pay.
    let mut reopens = Vec::new();
    drop(store);
    for _ in 0..5 {
        let started = Instant::now();
        let reopened = HistoryStore::open(directory.path(), policy).expect("a reopened store");
        reopens.push(started.elapsed());
        std::hint::black_box(reopened.stats());
    }
    report(
        "reopen and recover six hours of history",
        percentiles(reopens),
    );
}
