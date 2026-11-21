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
    let attribute = sysfs::charge_limit_attribute(&root).unwrap();
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

/// Ticket 136: the owner's keyboard is `hidraw1`, an I2C LampArray, found by
/// its report descriptor; the touchpad's `hidraw0`, a multitouch descriptor,
/// is not one.
#[test]
fn the_owners_keyboard_is_found_as_a_lamp_array_by_its_descriptor() {
    use hardware_core::lighting::{HintInterface, LightingHint};
    let root = fixture("owner");
    let nodes = sysfs::lamp_array_nodes(&root);
    assert_eq!(
        nodes,
        vec![sysfs::LampArrayNode {
            node: "hidraw1".into(),
            bus: Some(sysfs::BUS_I2C),
            vendor: Some(0x0B05),
            product: Some(0x5570),
        }]
    );
    assert!(nodes[0].built_in());
    assert_eq!(nodes[0].device().label(), "0018:0B05:5570");
    assert_eq!(sysfs::keyboard_rgb_mode_attribute(&root), None);
    assert_eq!(
        sysfs::lighting_hint(&root),
        Presence::Present(LightingHint {
            interface: Some(HintInterface::LampArray {
                node: "hidraw1".into()
            }),
            backlight: true,
        })
    );
    let touchpad = sysfs::report_descriptor(&root, "hidraw0").unwrap();
    assert!(!hardware_core::lamparray::is_lamp_array(&touchpad));
    assert_eq!(sysfs::report_descriptor(&root, "../hidraw1"), None);
    assert_eq!(sysfs::report_descriptor(&root, "hidraw1/../x"), None);
}

#[test]
fn a_tuf_laptop_is_found_by_kbd_rgb_mode_and_a_plain_laptop_by_its_backlight_alone() {
    use hardware_core::lighting::{HintInterface, LightingHint};
    let tuf = fixture("tuf");
    assert!(
        sysfs::keyboard_rgb_mode_attribute(&tuf)
            .unwrap()
            .ends_with("asus::kbd_backlight/kbd_rgb_mode")
    );
    assert_eq!(
        sysfs::lighting_hint(&tuf),
        Presence::Present(LightingHint {
            interface: Some(HintInterface::AsusWmi),
            backlight: true,
        })
    );
    // A backlight and no colour interface: one colour.
    let dir = tempfile::tempdir().unwrap();
    let led = dir.path().join("sys/class/leds/dell::kbd_backlight");
    std::fs::create_dir_all(&led).unwrap();
    std::fs::write(led.join("max_brightness"), "2\n").unwrap();
    assert_eq!(
        sysfs::lighting_hint(&SysRoot::at(dir.path())),
        Presence::Present(LightingHint {
            interface: None,
            backlight: true,
        })
    );
    assert_eq!(sysfs::lighting_hint(&fixture("nothing")), Presence::Absent);
}

#[test]
fn hidraw_nodes_are_judged_by_name_size_and_descriptor_never_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let class = dir.path().join("sys/class/hidraw");
    let lamp = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../hardware-core/tests/fixtures/m5406wa-ite5570.rdesc"),
    )
    .unwrap();
    for (name, descriptor, bus) in [
        // A USB LampArray, names that are not nodes, a descriptor too long,
        // one cut short, and the built-in one.
        ("hidraw7", lamp.clone(), "0003"),
        ("hidraw12345", lamp.clone(), "0018"),
        ("hidrawx", lamp.clone(), "0018"),
        ("hidraw3", [lamp.clone(), vec![0; 4096]].concat(), "0018"),
        ("hidraw4", lamp[..300].to_vec(), "0018"),
        ("hidraw2", lamp.clone(), "0018"),
    ] {
        let device = class.join(name).join("device");
        std::fs::create_dir_all(&device).unwrap();
        std::fs::write(device.join("report_descriptor"), descriptor).unwrap();
        std::fs::write(
            device.join("uevent"),
            format!("HID_ID={bus}:00000B05:00005570\n"),
        )
        .unwrap();
    }
    let nodes: Vec<(String, Option<u16>)> = sysfs::lamp_array_nodes(&SysRoot::at(dir.path()))
        .into_iter()
        .map(|n| (n.node, n.bus))
        .collect();
    // The built-in (I2C) one first.
    assert_eq!(
        nodes,
        vec![
            ("hidraw2".to_string(), Some(0x18)),
            ("hidraw7".to_string(), Some(0x03))
        ]
    );
    // Only the built-in one is ever the keyboard: a USB Dynamic Lighting
    // keyboard is not, even alone.
    let built_in: Vec<String> = sysfs::built_in_lamp_arrays(&SysRoot::at(dir.path()))
        .into_iter()
        .map(|n| n.node)
        .collect();
    assert_eq!(built_in, vec!["hidraw2".to_string()]);
    std::fs::remove_dir_all(class.join("hidraw2")).unwrap();
    assert!(sysfs::built_in_lamp_arrays(&SysRoot::at(dir.path())).is_empty());
    assert_eq!(
        sysfs::lighting_hint(&SysRoot::at(dir.path())),
        Presence::Absent,
        "a USB LampArray alone gives the page no keyboard colour card"
    );
    assert!(sysfs::is_hidraw_name("hidraw0"));
    assert!(!sysfs::is_hidraw_name("hidraw"));
    assert!(!sysfs::is_hidraw_name("hidraw-1"));
    assert!(!sysfs::is_hidraw_name("event3"));
}

/// The owner's machine on 2026-11-21: the charger online, the battery
/// `Not charging` at 80% with the limit at 80 (ticket 170).
#[test]
fn the_owners_machine_held_at_its_limit_reads_as_held() {
    use hardware_core::charge_state::{ChargeLimit, ChargeState, charge_state};
    let root = fixture("held");
    assert_eq!(sysfs::charger_online(&root), Some(true));
    let readings = sysfs::charge_readings(&root).expect("BAT1 is a system battery");
    assert_eq!(readings.batteries.len(), 1);
    let bat = &readings.batteries[0];
    assert_eq!(bat.status, Some(BatteryStatus::NotCharging));
    assert_eq!(bat.percent, Some(80));
    assert_eq!(
        bat.limit,
        ChargeLimit::Percent {
            end: 80,
            start: None
        }
    );
    assert_eq!(
        charge_state(&readings),
        ChargeState::Holding {
            limit: 80,
            percent: Some(80)
        }
    );
}

/// Two batteries with start thresholds: the firmware charges BAT1 while BAT0
/// rests at its limit, which is charging, never a fault; each reports its own
/// window, and the charge is weighted by energy (27.6 Wh of 46).
#[test]
fn a_thinkpad_with_two_batteries_reads_each_with_its_own_window() {
    use hardware_core::charge_state::{ChargeLimit, ChargeState, charge_state};
    let root = fixture("thinkpad");
    let readings = sysfs::charge_readings(&root).expect("two system batteries");
    assert_eq!(readings.charger_online, Some(true));
    assert_eq!(readings.batteries.len(), 2);
    for bat in &readings.batteries {
        assert_eq!(
            bat.limit,
            ChargeLimit::Percent {
                end: 80,
                start: Some(75)
            }
        );
    }
    assert_eq!(readings.percent(), Some(60));
    assert_eq!(charge_state(&readings), ChargeState::Charging);
    // The System page's own card shows the first battery.
    assert!(sysfs::system_battery(&root).unwrap().ends_with("BAT0"));
}

#[test]
fn the_charger_is_offline_on_the_owners_discharging_fixture_and_unknown_without_one() {
    assert_eq!(sysfs::charger_online(&fixture("owner")), Some(false));
    assert_eq!(sysfs::charger_online(&fixture("nothing")), None);
    assert_eq!(sysfs::charge_readings(&fixture("nothing")), None);
}

/// A UPS the kernel lists beside the battery is not a charger, and an
/// unreadable limit is unknown rather than "no limit".
#[test]
fn a_ups_is_not_a_charger_and_an_unreadable_limit_is_unreadable() {
    use hardware_core::charge_state::ChargeLimit;
    let dir = tempfile::tempdir().unwrap();
    let ps = dir.path().join("sys/class/power_supply");
    for (name, files) in [
        ("ups", vec![("type", "UPS"), ("online", "1")]),
        ("AC", vec![("type", "Mains"), ("online", "0")]),
        (
            "BAT0",
            vec![
                ("type", "Battery"),
                ("status", "Not charging"),
                ("capacity", "80"),
                ("charge_control_end_threshold", "garbage"),
            ],
        ),
    ] {
        std::fs::create_dir_all(ps.join(name)).unwrap();
        for (file, text) in files {
            std::fs::write(ps.join(name).join(file), format!("{text}\n")).unwrap();
        }
    }
    let root = SysRoot::at(dir.path());
    assert_eq!(sysfs::charger_online(&root), Some(false));
    let readings = sysfs::charge_readings(&root).unwrap();
    assert_eq!(readings.batteries[0].limit, ChargeLimit::Unreadable);
}
