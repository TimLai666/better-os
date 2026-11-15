//! The fixture trees: the owner's machine as read on 2026-11-12 (spec 11.1),
//! a machine with nothing, and an Intel laptop.

use std::path::PathBuf;

use hardware_core::Presence;
use hardware_core::battery::BatteryStatus;
use hardware_core::fan::{FanMode, FanState};
use hardware_core::sensors::{SensorKind, SensorLabel, rows};
use hardware_platform::sysfs::{self, SysRoot};

fn fixture(name: &str) -> SysRoot {
    SysRoot::at(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
}

#[test]
fn the_owners_machine_has_a_battery_a_limit_and_a_fan_switch() {
    let root = fixture("owner");
    let Presence::Present(battery) = sysfs::battery(&root) else {
        panic!("BAT1 is a system battery")
    };
    assert_eq!(battery.capacity, Some(87));
    assert_eq!(battery.status, BatteryStatus::Discharging);
    assert_eq!(battery.health_percent(), Some(88));
    assert_eq!(battery.cycles, Some(0));
    assert_eq!(battery.model.as_deref(), Some("A32-K55"));
    assert_eq!(sysfs::charge_limit(&root), Presence::Present(100));
    let attribute = sysfs::writable_charge_limit_attribute(&root).unwrap();
    assert!(attribute.ends_with("power_supply/BAT1/charge_control_end_threshold"));
    assert_eq!(
        sysfs::fan_mode(&root),
        Presence::Present(FanState::Mode(FanMode::Auto))
    );
    assert!(
        sysfs::fan_mode_attribute(&root)
            .unwrap()
            .ends_with("hwmon8/pwm1_enable")
    );
    assert!(sysfs::keyboard_led(&root).is_some());
    let about = sysfs::about(&root);
    assert_eq!(
        about.product_name.as_deref(),
        Some("ASUS Vivobook S 14 M5406WA_M5406WA")
    );
    assert_eq!(about.kernel.as_deref(), Some("7.0.0-43-generic"));
}

#[test]
fn the_owners_sensors_read_once_per_fan_with_their_labels() {
    let rows = rows(&sysfs::sensors(&fixture("owner")));
    let labels: Vec<&SensorLabel> = rows.iter().map(|r| &r.label).collect();
    for wanted in [
        SensorLabel::Processor,
        SensorLabel::Graphics,
        SensorLabel::Ssd,
        SensorLabel::Motherboard,
        SensorLabel::WiFi,
        SensorLabel::CpuFan,
    ] {
        assert!(labels.contains(&&wanted), "{wanted:?} in {labels:?}");
    }
    let fans: Vec<_> = rows.iter().filter(|r| r.kind == SensorKind::Fan).collect();
    assert_eq!(fans.len(), 1, "acpi_fan is the same fan as asus cpu_fan");
    assert_eq!(fans[0].value, Some(4_800.0));
    let processor = rows
        .iter()
        .find(|r| r.label == SensorLabel::Processor)
        .unwrap();
    assert_eq!(processor.value, Some(52.375));
}

#[test]
fn a_machine_with_nothing_reads_nothing() {
    let root = fixture("nothing");
    assert_eq!(sysfs::battery(&root), Presence::Absent);
    assert_eq!(sysfs::charge_limit(&root), Presence::Absent);
    assert_eq!(sysfs::fan_mode(&root), Presence::Absent);
    assert!(sysfs::sensors(&root).is_empty());
    assert!(sysfs::keyboard_led(&root).is_none());
}

#[test]
fn an_intel_laptop_reads_energy_and_skips_the_mouse() {
    let root = fixture("intel");
    let Presence::Present(battery) = sysfs::battery(&root) else {
        panic!("BAT0")
    };
    assert_eq!(battery.capacity, Some(54));
    assert_eq!(battery.health_percent(), Some(84));
    assert_eq!(battery.cycles, Some(212));
    assert_eq!(sysfs::charge_limit(&root), Presence::Absent);
    assert_eq!(sysfs::fan_mode(&root), Presence::Absent);
    let rows = rows(&sysfs::sensors(&root));
    assert_eq!(rows[0].label, SensorLabel::Processor);
    assert!(rows.iter().any(|r| r.label == SensorLabel::WiFi));
    assert!(
        rows.iter()
            .any(|r| r.label == SensorLabel::Raw("coretemp Core 0".into()))
    );
}

#[test]
fn an_unreadable_limit_and_fan_are_present_and_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let sys = dir.path().join("sys");
    let bat = sys.join("class/power_supply/BAT0");
    std::fs::create_dir_all(&bat).unwrap();
    std::fs::write(bat.join("type"), "Battery\n").unwrap();
    std::fs::write(bat.join("capacity"), "50\n").unwrap();
    // A directory where the file should be: present, and reading fails.
    std::fs::create_dir_all(bat.join("charge_control_end_threshold")).unwrap();
    let platform = sys.join("devices/platform/asus-nb-wmi");
    let chip = platform.join("hwmon/hwmon3");
    std::fs::create_dir_all(&chip).unwrap();
    std::fs::write(chip.join("name"), "asus\n").unwrap();
    std::fs::write(chip.join("pwm1_enable"), "garbage\n").unwrap();
    std::os::unix::fs::symlink(&platform, chip.join("device")).unwrap();
    std::fs::create_dir_all(sys.join("class/hwmon")).unwrap();
    std::os::unix::fs::symlink(&chip, sys.join("class/hwmon/hwmon3")).unwrap();
    let root = SysRoot::at(dir.path());
    assert!(matches!(
        sysfs::charge_limit(&root),
        Presence::Unreadable(_)
    ));
    assert!(matches!(sysfs::fan_mode(&root), Presence::Unreadable(_)));
}

#[test]
fn a_fan_with_a_writable_pwm_is_not_a_mode_switch_and_an_unknown_device_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let sys = dir.path().join("sys");
    for (n, device) in [(1, "asus-nb-wmi"), (2, "thinkpad_hwmon")] {
        let platform = sys.join(format!("devices/platform/{device}"));
        let chip = platform.join(format!("hwmon/hwmon{n}"));
        std::fs::create_dir_all(&chip).unwrap();
        std::fs::write(chip.join("name"), "fan\n").unwrap();
        std::fs::write(chip.join("pwm1_enable"), "2\n").unwrap();
        std::os::unix::fs::symlink(&platform, chip.join("device")).unwrap();
        std::fs::create_dir_all(sys.join("class/hwmon")).unwrap();
        std::os::unix::fs::symlink(&chip, sys.join(format!("class/hwmon/hwmon{n}"))).unwrap();
    }
    let root = SysRoot::at(dir.path());
    assert!(sysfs::fan_mode_attribute(&root).is_some());
    let asus = sys.join("devices/platform/asus-nb-wmi/hwmon/hwmon1/pwm1");
    std::fs::write(&asus, "128\n").unwrap();
    assert_eq!(sysfs::fan_mode_attribute(&root), None);
}

#[test]
fn a_battery_under_a_name_the_unit_does_not_open_is_read_and_not_writable() {
    let dir = tempfile::tempdir().unwrap();
    let bat = dir.path().join("sys/class/power_supply/CMB0");
    std::fs::create_dir_all(&bat).unwrap();
    std::fs::write(bat.join("type"), "Battery\n").unwrap();
    std::fs::write(bat.join("capacity"), "50\n").unwrap();
    std::fs::write(bat.join("charge_control_end_threshold"), "80\n").unwrap();
    let root = SysRoot::at(dir.path());
    assert_eq!(sysfs::charge_limit(&root), Presence::Present(80));
    assert_eq!(sysfs::writable_charge_limit_attribute(&root), None);
}
