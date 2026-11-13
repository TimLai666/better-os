//! The NPU collector against the `accel` tree recorded on the owner's ASUS
//! Vivobook S 14 (AMD XDNA, `amdxdna`, `RyzenAI-npu4`, firmware 1.0.0.63,
//! asleep), and against an Intel NPU whose driver publishes a busy counter.
//! Nothing here reads the machine the test runs on.

use monitor_collectors_linux::{NpuCollector, ProcessUsageCollector, Roots};
use monitor_core::{CollectorReport, EntityKind, MetricId, Observation, Timestamp};
use std::os::unix::fs::symlink;
use std::path::Path;

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("{content}\n")).unwrap();
}

fn accel(root: &Path, address: &str, driver: &str, files: &[(&str, &str)]) {
    let device = root.join(format!("sys/devices/pci0000:00/{address}"));
    std::fs::create_dir_all(&device).unwrap();
    std::fs::create_dir_all(root.join("sys/class/accel/accel0")).unwrap();
    symlink(&device, root.join("sys/class/accel/accel0/device")).unwrap();
    write(
        root,
        &format!("sys/devices/pci0000:00/{address}/uevent"),
        &format!("DRIVER={driver}\nPCI_SLOT_NAME={address}"),
    );
    for (path, content) in files {
        write(
            root,
            &format!("sys/devices/pci0000:00/{address}/{path}"),
            content,
        );
    }
}

/// Recorded from `/sys/class/accel/accel0/device` on the owner's machine.
const XDNA: &[(&str, &str)] = &[
    ("vbnv", "RyzenAI-npu4"),
    ("fw_version", "1.0.0.63"),
    ("power_state", "D3hot"),
    ("power/runtime_status", "suspended"),
    ("power/runtime_active_time", "12035"),
    ("vendor", "0x1022"),
];

fn at(seconds: u64) -> Timestamp {
    Timestamp {
        unix_ms: 1_700_000_000_000 + seconds * 1000,
        monotonic_ns: seconds * 1_000_000_000,
    }
}

fn npu(report: &CollectorReport, key: &str, metric: &str) -> Option<Observation> {
    report
        .entities
        .iter()
        .find(|e| e.id.kind == EntityKind::Npu && e.id.key == key)?
        .metrics
        .get(&MetricId::new(metric).unwrap())
        .cloned()
}

#[test]
fn the_xdna_npu_is_named_with_its_firmware_and_power_state() {
    let root = tempfile::tempdir().unwrap();
    accel(root.path(), "0000:64:00.1", "amdxdna", XDNA);
    std::fs::create_dir_all(root.path().join("proc")).unwrap();
    let roots = Roots::at(root.path());
    let mut usage = ProcessUsageCollector::new(roots.clone());
    let mut collector = NpuCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[]);
    collector.sample_with(&roots, at(1), Some(usage.latest()));
    usage.sample_for(&roots, at(2), &[]);
    let report = collector.sample_with(&roots, at(2), Some(usage.latest()));
    let key = "0000:64:00.1";
    assert_eq!(
        npu(&report, key, "npu.model"),
        Some(Observation::text("RyzenAI-npu4"))
    );
    assert_eq!(
        npu(&report, key, "npu.firmware"),
        Some(Observation::text("1.0.0.63"))
    );
    assert_eq!(
        npu(&report, key, "npu.power_state"),
        Some(Observation::text("D3hot"))
    );
    assert_eq!(
        npu(&report, key, "npu.runtime_status"),
        Some(Observation::text("suspended"))
    );
    assert_eq!(
        npu(&report, key, "npu.driver"),
        Some(Observation::text("amdxdna"))
    );
    assert_eq!(
        npu(&report, key, "npu.vendor"),
        Some(Observation::text("AMD"))
    );
    // Asleep and with no program holding it, it is not busy and was not awake.
    assert_eq!(
        npu(&report, key, "npu.utilization"),
        Some(Observation::float(0.0))
    );
    assert_eq!(
        npu(&report, key, "npu.awake"),
        Some(Observation::float(0.0))
    );
    assert_eq!(
        npu(&report, key, "npu.clients"),
        Some(Observation::unsigned(0))
    );
}

#[test]
fn an_npu_whose_client_reports_no_busy_time_says_so() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    accel(r, "0000:64:00.1", "amdxdna", XDNA);
    std::fs::create_dir_all(r.join("proc/77/fd")).unwrap();
    symlink("/dev/accel/accel0", r.join("proc/77/fd/9")).unwrap();
    write(
        r,
        "proc/77/fdinfo/9",
        "drm-driver:\tamdxdna\ndrm-client-id:\t1\ndrm-pdev:\t0000:64:00.1\ndrm-resident-memory:\t2048 KiB",
    );
    let roots = Roots::at(r);
    let mut usage = ProcessUsageCollector::new(roots.clone());
    usage.sample_for(&roots, at(1), &[77]);
    usage.sample_for(&roots, at(2), &[77]);
    let report = NpuCollector::new(roots.clone()).sample_with(&roots, at(2), Some(usage.latest()));
    assert_eq!(
        npu(&report, "0000:64:00.1", "npu.clients"),
        Some(Observation::unsigned(1))
    );
    assert!(matches!(
        npu(&report, "0000:64:00.1", "npu.utilization"),
        Some(Observation::Unsupported(_))
    ));
    assert_eq!(
        usage.latest().processes[&77].npu["0000:64:00.1"].shared_bytes,
        2048 * 1024
    );
}

#[test]
fn an_intel_npu_is_as_busy_as_its_busy_counter_says() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    accel(
        r,
        "0000:00:0b.0",
        "intel_vpu",
        &[("npu_busy_time_us", "1000000"), ("vendor", "0x8086")],
    );
    let roots = Roots::at(r);
    let mut collector = NpuCollector::new(roots.clone());
    collector.sample_with(&roots, at(1), None);
    write(
        r,
        "sys/devices/pci0000:00/0000:00:0b.0/npu_busy_time_us",
        "1400000",
    );
    let report = collector.sample_with(&roots, at(2), None);
    let busy = npu(&report, "0000:00:0b.0", "npu.utilization").and_then(|o| o.as_f64());
    assert!((busy.unwrap() - 0.4).abs() < 1e-9);
    assert_eq!(
        npu(&report, "0000:00:0b.0", "npu.utilization.source"),
        Some(Observation::text("driver"))
    );
}

#[test]
fn a_machine_without_an_npu_shows_none() {
    let root = tempfile::tempdir().unwrap();
    let roots = Roots::at(root.path());
    let report = NpuCollector::new(roots.clone()).sample_with(&roots, at(1), None);
    assert!(report.entities.is_empty());
}
