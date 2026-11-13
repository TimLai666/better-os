//! The battery collector against `power_supply` trees recorded on the owner's
//! ASUS Vivobook S 14 (BAT1, a charge-reporting battery, plus a mains adapter
//! and a USB-C port), once charging and once on the battery. Nothing here
//! reads the machine the test runs on.

use monitor_collectors_linux::{BatteryCollector, Roots};
use monitor_core::{CollectorReport, EntityKind, MetricId, MetricScalar, Observation, Timestamp};

/// Recorded on mains power, charging.
const CHARGING: &[(&str, &str)] = &[
    ("BAT1/capacity", "60"),
    ("BAT1/charge_control_end_threshold", "100"),
    ("BAT1/charge_full", "4080000"),
    ("BAT1/charge_full_design", "4680000"),
    ("BAT1/charge_now", "2450000"),
    ("BAT1/current_now", "2596000"),
    ("BAT1/cycle_count", "0"),
    ("BAT1/manufacturer", "ASUS "),
    ("BAT1/model_name", "A32-K55"),
    ("BAT1/present", "1"),
    ("BAT1/status", "Charging"),
    ("BAT1/technology", "Li-ion"),
    ("BAT1/type", "Battery"),
    ("BAT1/voltage_min_design", "16026000"),
    ("BAT1/voltage_now", "16363000"),
    ("ACAD/online", "1"),
    ("ACAD/type", "Mains"),
    ("ucsi-source-psy-USBC000:001/current_max", "4500000"),
    ("ucsi-source-psy-USBC000:001/online", "1"),
    ("ucsi-source-psy-USBC000:001/scope", "Device"),
    ("ucsi-source-psy-USBC000:001/type", "USB"),
    ("ucsi-source-psy-USBC000:001/usb_type", "C [PD] PD_PPS"),
    ("ucsi-source-psy-USBC000:001/voltage_max", "20000000"),
    ("ucsi-source-psy-USBC000:001/voltage_now", "20000000"),
];

/// Recorded on the battery: the changed files only.
const DISCHARGING: &[(&str, &str)] = &[
    ("BAT1/capacity", "56"),
    ("BAT1/charge_now", "2302000"),
    ("BAT1/current_now", "2322000"),
    ("BAT1/status", "Discharging"),
    ("BAT1/voltage_now", "15405000"),
    ("ACAD/online", "0"),
];

fn tree(layers: &[&[(&str, &str)]]) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a temporary directory");
    for layer in layers {
        for (path, content) in *layer {
            let path = root.path().join("sys/class/power_supply").join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("{content}\n")).unwrap();
        }
    }
    root
}

fn sample(root: &tempfile::TempDir) -> CollectorReport {
    let roots = Roots::at(root.path());
    BatteryCollector::new(roots.clone()).sample(
        &roots,
        Timestamp {
            unix_ms: 1_700_000_000_000,
            monotonic_ns: 1,
        },
    )
}

fn entity(
    report: &CollectorReport,
    kind: EntityKind,
    key: &str,
    metric: &str,
) -> Option<Observation> {
    report
        .entities
        .iter()
        .find(|e| e.id.kind == kind && e.id.key == key)?
        .metrics
        .get(&MetricId::new(metric).unwrap())
        .cloned()
}

fn battery(report: &CollectorReport, metric: &str) -> Option<f64> {
    entity(report, EntityKind::Battery, "BAT1", metric).and_then(|o| o.as_f64())
}

fn close(value: Option<f64>, expected: f64) {
    let value = value.expect("a reading");
    assert!((value - expected).abs() < 1e-3, "{value} != {expected}");
}

#[test]
fn a_charge_reporting_battery_is_described_in_watt_hours() {
    let root = tree(&[CHARGING]);
    let report = sample(&root);
    close(battery(&report, "battery.charge"), 0.60);
    // µAh times the 16.026 V design voltage.
    close(battery(&report, "battery.energy.now"), 2.45 * 16.026);
    close(battery(&report, "battery.energy.full"), 4.08 * 16.026);
    close(battery(&report, "battery.energy.design"), 4.68 * 16.026);
    close(battery(&report, "battery.health"), 4.08 / 4.68);
    close(battery(&report, "battery.power"), 2.596 * 16.363);
    close(battery(&report, "battery.voltage"), 16.363);
    // (full - now) over the charging power.
    close(
        battery(&report, "battery.time.to_full"),
        (4.08 - 2.45) * 16.026 / (2.596 * 16.363) * 3600.0,
    );
    assert_eq!(battery(&report, "battery.time.remaining"), None);
    assert_eq!(
        entity(&report, EntityKind::Battery, "BAT1", "battery.status"),
        Some(Observation::text("charging"))
    );
    assert_eq!(
        entity(&report, EntityKind::Battery, "BAT1", "battery.manufacturer"),
        Some(Observation::text("ASUS"))
    );
    // The firmware's 0 is not a count.
    assert!(matches!(
        entity(&report, EntityKind::Battery, "BAT1", "battery.cycles"),
        Some(Observation::Unsupported(_))
    ));
    assert_eq!(
        report
            .metrics
            .get(&MetricId::new("power.on_battery").unwrap()),
        Some(&Observation::Value(MetricScalar::Bool(false)))
    );
}

#[test]
fn the_charger_and_the_usb_c_port_are_power_sources() {
    let root = tree(&[CHARGING]);
    let report = sample(&root);
    let port = "ucsi-source-psy-USBC000:001";
    assert_eq!(
        entity(
            &report,
            EntityKind::PowerSource,
            port,
            "power_source.usb_type"
        ),
        Some(Observation::text("PD"))
    );
    close(
        entity(
            &report,
            EntityKind::PowerSource,
            port,
            "power_source.power.max",
        )
        .and_then(|o| o.as_f64()),
        90.0,
    );
    assert_eq!(
        entity(
            &report,
            EntityKind::PowerSource,
            "ACAD",
            "power_source.online"
        ),
        Some(Observation::boolean(true))
    );
}

#[test]
fn on_the_battery_the_draw_and_the_time_left_are_reported() {
    let root = tree(&[CHARGING, DISCHARGING]);
    let report = sample(&root);
    let draw = 2.322 * 15.405;
    close(battery(&report, "battery.power"), draw);
    close(
        battery(&report, "battery.time.remaining"),
        2.302 * 16.026 / draw * 3600.0,
    );
    assert_eq!(
        report
            .metrics
            .get(&MetricId::new("power.on_battery").unwrap()),
        Some(&Observation::boolean(true))
    );
    close(
        report
            .metrics
            .get(&MetricId::new("power.battery.draw").unwrap())
            .and_then(|o| o.as_f64()),
        draw,
    );
}

#[test]
fn a_desktop_without_a_battery_reports_none_rather_than_an_empty_one() {
    let root = tree(&[&[("ACAD/online", "1"), ("ACAD/type", "Mains")]]);
    let report = sample(&root);
    assert!(report.entities_of(EntityKind::Battery).next().is_none());
    assert!(matches!(
        report
            .metrics
            .get(&MetricId::new("power.on_battery").unwrap()),
        Some(Observation::Unsupported(_))
    ));
}

#[test]
fn an_absent_battery_slot_is_skipped() {
    let root = tree(&[&[("BAT0/type", "Battery"), ("BAT0/present", "0")]]);
    let report = sample(&root);
    assert!(report.entities_of(EntityKind::Battery).next().is_none());
}
