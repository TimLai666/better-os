//! Every battery icon and battery line in Better OS asks
//! `hardware_core::charge_state::battery_look` (ticket 170). Before it, each
//! window kept its own mapping from the kernel's status to a picture and to
//! words, and every one of them drew a battery held at its charge limit as a
//! battery running the machine. This test reads the source of every crate
//! and fails when such a mapping comes back.
//!
//! What it looks for is the shape those mappings had: a match arm on the
//! kernel's status or on Better Monitor's `battery.status` key, a match arm
//! that picks a battery level or state picture, and the per-window copy
//! fields and functions that held the words.

use std::path::{Path, PathBuf};

/// The patterns, each with what it was.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "BatteryStatus::Charging =>",
        "a match on the kernel's status",
    ),
    (
        "BatteryStatus::Discharging =>",
        "a match on the kernel's status",
    ),
    ("BatteryStatus::Full =>", "a match on the kernel's status"),
    (
        "BatteryStatus::NotCharging =>",
        "a match on the kernel's status",
    ),
    (r#"Some("charging") =>"#, "a match on battery.status"),
    (r#"Some("discharging") =>"#, "a match on battery.status"),
    (r#"Some("full") =>"#, "a match on battery.status"),
    (r#"Some("not_charging") =>"#, "a match on battery.status"),
    (
        "=> Picture::BatteryFull",
        "a battery picture chosen per window",
    ),
    (
        "=> Picture::BatteryMedium",
        "a battery picture chosen per window",
    ),
    (
        "=> Picture::BatteryLow",
        "a battery picture chosen per window",
    ),
    (
        "=> Picture::BatteryCharging",
        "a battery picture chosen per window",
    ),
    (
        "=> Picture::BatteryPlugged",
        "a battery picture chosen per window",
    ),
    (
        "=> Picture::BatteryFullPlugged",
        "a battery picture chosen per window",
    ),
    ("battery_charging:", "a window's own battery words"),
    ("battery_discharging:", "a window's own battery words"),
    ("battery_not_charging:", "a window's own battery words"),
    ("fn battery_status(", "a window's own battery words"),
    ("fn battery_words(", "a window's own battery words"),
];

/// The crates that own the rule or read the kernel: the rule itself, and
/// the collector that parses `status` and computes time left from it.
const OWNERS: &[&str] = &["hardware-core", "monitor-collectors-linux"];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_window_keeps_a_battery_mapping_of_its_own() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();
    let mut checked = 0;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("crates/").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if OWNERS.contains(&name.as_str()) {
            continue;
        }
        let mut files = Vec::new();
        sources(&entry.path(), &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            checked += 1;
            for (number, line) in text.lines().enumerate() {
                for (pattern, what) in FORBIDDEN {
                    if line.contains(pattern) {
                        found.push(format!(
                            "{}:{}: {what}: {}",
                            file.strip_prefix(&crates).unwrap_or(&file).display(),
                            number + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }
    }
    assert!(checked > 100, "only {checked} source files were read");
    assert!(
        found.is_empty(),
        "use hardware_core::charge_state::battery_look instead:\n{}",
        found.join("\n")
    );
}

/// The test itself would catch a mapping: a line written the old way trips
/// it.
#[test]
fn the_patterns_catch_the_old_mappings() {
    let old = [
        "                home::BatteryStatus::Charging => Picture::BatteryCharging,",
        r#"        Some("not_charging") => c.battery_not_charging,"#,
        "                            Some(p) if p >= 90 => Picture::BatteryFull,",
        "    battery_not_charging: \"已接上電源，未充電\",",
    ];
    for line in old {
        assert!(
            FORBIDDEN.iter().any(|(pattern, _)| line.contains(pattern)),
            "{line}"
        );
    }
}
