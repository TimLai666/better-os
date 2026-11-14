//! The sensors collector against the hwmon tree recorded on the owner's ASUS
//! Vivobook S 14 (amdgpu, k10temp, asus, nvme, BAT1). Nothing here reads the
//! machine the test runs on: the tree is written into a temporary directory
//! and the collector is pointed at it through `Roots::at`.

use monitor_collectors_linux::{Roots, SensorsCollector};
use monitor_core::{CollectorHealth, EntityKind, MetricId, Observation, Timestamp};
use std::path::Path;

/// Recorded with `cat /sys/class/hwmon/hwmon*/{name,*_input,*_label,*_max,*_crit,*_average}`.
const VIVOBOOK_HWMON: &[(&str, &str)] = &[
    ("hwmon0/name", "ACAD"),
    ("hwmon1/fan1_input", "6091"),
    ("hwmon1/name", "acpi_fan"),
    ("hwmon10/freq1_input", "1220000000"),
    ("hwmon10/freq1_label", "sclk"),
    ("hwmon10/in0_input", "0"),
    ("hwmon10/in0_label", "vddgfx"),
    ("hwmon10/in1_input", "0"),
    ("hwmon10/in1_label", "vddnb"),
    ("hwmon10/name", "amdgpu"),
    ("hwmon10/power1_average", "27407000"),
    ("hwmon10/power1_input", "27407000"),
    ("hwmon10/power1_label", "PPT"),
    ("hwmon10/temp1_input", "63000"),
    ("hwmon10/temp1_label", "edge"),
    ("hwmon2/name", "acpitz"),
    ("hwmon2/temp1_input", "67000"),
    ("hwmon3/curr1_input", "1718"),
    ("hwmon3/in0_input", "15842"),
    ("hwmon3/name", "BAT1"),
    ("hwmon4/name", "nvme"),
    ("hwmon4/temp1_crit", "89850"),
    ("hwmon4/temp1_input", "60850"),
    ("hwmon4/temp1_label", "Composite"),
    ("hwmon4/temp1_max", "86850"),
    ("hwmon4/temp2_input", "60850"),
    ("hwmon4/temp2_label", "Sensor 1"),
    ("hwmon4/temp2_max", "65261850"),
    ("hwmon5/curr1_input", "4500"),
    ("hwmon5/in0_input", "20000"),
    ("hwmon5/name", "ucsi_source_psy_USBC000:001"),
    ("hwmon7/name", "k10temp"),
    ("hwmon7/temp1_input", "77750"),
    ("hwmon7/temp1_label", "Tctl"),
    ("hwmon8/fan1_input", "6000"),
    ("hwmon8/fan1_label", "cpu_fan"),
    ("hwmon8/name", "asus"),
    ("hwmon9/name", "mt7921_phy0"),
    ("hwmon9/temp1_input", "60000"),
];

fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a temporary directory");
    for (path, content) in files {
        let path = root.path().join("sys/class/hwmon").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{content}\n")).unwrap();
    }
    root
}

fn at() -> Timestamp {
    Timestamp {
        unix_ms: 1_700_000_000_000,
        monotonic_ns: 1,
    }
}

fn value(report: &monitor_core::CollectorReport, key: &str, metric: &str) -> Option<Observation> {
    report
        .entities
        .iter()
        .find(|e| e.id.kind == EntityKind::Sensor && e.id.key == key)?
        .metrics
        .get(&MetricId::new(metric).unwrap())
        .cloned()
}

#[test]
fn every_recorded_sensor_becomes_a_reading_in_plain_units() {
    let root = tree(VIVOBOOK_HWMON);
    let roots = Roots::at(root.path());
    let report = SensorsCollector::new(roots.clone()).sample(&roots, at());
    assert_eq!(report.health, CollectorHealth::Healthy);
    let float = |key, metric| value(&report, key, metric).and_then(|o| o.as_f64());
    assert_eq!(float("k10temp/temp1", "sensor.temperature"), Some(77.75));
    assert_eq!(float("amdgpu/temp1", "sensor.temperature"), Some(63.0));
    assert_eq!(float("amdgpu/power1", "sensor.power"), Some(27.407));
    assert_eq!(float("amdgpu/freq1", "sensor.frequency"), Some(1.22e9));
    assert_eq!(float("asus/fan1", "sensor.fan.speed"), Some(6000.0));
    assert_eq!(float("acpi_fan/fan1", "sensor.fan.speed"), Some(6091.0));
    assert_eq!(float("BAT1/in0", "sensor.voltage"), Some(15.842));
    assert_eq!(float("BAT1/curr1", "sensor.current"), Some(1.718));
    assert_eq!(
        float("nvme/temp1", "sensor.temperature.critical"),
        Some(89.85)
    );
    assert_eq!(float("nvme/temp1", "sensor.temperature.high"), Some(86.85));
    // 65261.85 °C is the driver's "not set", not a threshold.
    assert_eq!(float("nvme/temp2", "sensor.temperature.high"), None);
    assert_eq!(
        value(&report, "asus/fan1", "sensor.label").and_then(|o| o.as_text().map(str::to_string)),
        Some("cpu_fan".to_string())
    );
    // A chip with no inputs (the mains adapter) has no reading, and the power
    // input read once even though the driver offers both forms.
    assert!(!report.entities.iter().any(|e| e.id.key.starts_with("ACAD")));
    assert_eq!(
        report
            .entities
            .iter()
            .filter(|e| e.id.key == "amdgpu/power1")
            .count(),
        1
    );
}

#[test]
fn two_chips_with_one_name_are_told_apart_by_their_device() {
    let root = tree(&[
        ("hwmon0/name", "nvme"),
        ("hwmon0/temp1_input", "40000"),
        ("hwmon1/name", "nvme"),
        ("hwmon1/temp1_input", "50000"),
    ]);
    let hwmon = root.path().join("sys/class/hwmon");
    std::fs::create_dir_all(root.path().join("devices/nvme0")).unwrap();
    std::fs::create_dir_all(root.path().join("devices/nvme1")).unwrap();
    std::os::unix::fs::symlink(
        root.path().join("devices/nvme0"),
        hwmon.join("hwmon0/device"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        root.path().join("devices/nvme1"),
        hwmon.join("hwmon1/device"),
    )
    .unwrap();
    let roots = Roots::at(root.path());
    let report = SensorsCollector::new(roots.clone()).sample(&roots, at());
    let keys: Vec<&str> = report.entities.iter().map(|e| e.id.key.as_str()).collect();
    assert_eq!(keys, vec!["nvme@nvme0/temp1", "nvme@nvme1/temp1"]);
}

#[test]
fn an_input_that_cannot_be_read_is_unknown_not_zero() {
    let root = tree(&[
        ("hwmon0/name", "acpi_fan"),
        ("hwmon0/fan1_input", "garbage"),
    ]);
    let roots = Roots::at(root.path());
    let report = SensorsCollector::new(roots.clone()).sample(&roots, at());
    let fan = value(&report, "acpi_fan/fan1", "sensor.fan.speed").unwrap();
    assert!(matches!(fan, Observation::Unknown(_)), "{fan:?}");
    assert!(matches!(report.health, CollectorHealth::Degraded { .. }));
}

#[test]
fn a_machine_without_hwmon_shows_nothing_rather_than_a_broken_reading() {
    let root = tempfile::tempdir().unwrap();
    assert!(!Path::new(&root.path().join("sys/class/hwmon")).exists());
    let roots = Roots::at(root.path());
    let report = SensorsCollector::new(roots.clone()).sample(&roots, at());
    assert!(report.entities.is_empty());
    assert!(matches!(report.health, CollectorHealth::Unsupported(_)));
}
