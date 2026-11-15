//! The daemon's unit opens exactly the paths the daemon writes (ticket 86):
//! `/sys` stays read-only through `ProtectKernelTunables=yes`, and each
//! battery name and fan platform device the daemon recognises has its own
//! `ReadWritePaths=` entry, in both units that write.

use hardware_platform::sysfs::{BATTERY_NAMES, FAN_PLATFORM_DEVICES};

const DAEMON: &str = include_str!("../../../packaging/daemon/better-manager-daemon.service");
const RESTORE: &str = include_str!("../../../packaging/daemon/better-hardware-restore.service");

fn read_write_paths(unit: &str) -> Vec<String> {
    unit.lines()
        .filter_map(|line| line.strip_prefix("ReadWritePaths="))
        .flat_map(|paths| paths.split_whitespace().map(str::to_string))
        .collect()
}

#[test]
fn the_daemon_unit_opens_each_battery_and_fan_device_and_nothing_else() {
    assert!(DAEMON.lines().any(|l| l == "ProtectKernelTunables=yes"));
    let mut wanted: Vec<String> = BATTERY_NAMES
        .iter()
        .map(|n| format!("-/sys/class/power_supply/{n}"))
        .chain(
            FAN_PLATFORM_DEVICES
                .iter()
                .map(|d| format!("-/sys/devices/platform/{d}")),
        )
        .collect();
    let mut got = read_write_paths(DAEMON);
    wanted.sort();
    got.sort();
    assert_eq!(got, wanted);
}

#[test]
fn the_restore_unit_opens_only_the_batteries() {
    assert!(RESTORE.lines().any(|l| l == "ProtectKernelTunables=yes"));
    assert!(RESTORE.lines().any(|l| l == "Type=oneshot"));
    assert!(
        RESTORE
            .lines()
            .any(|l| l == "ExecStart=/usr/libexec/better-manager-daemon --restore-hardware")
    );
    let mut wanted: Vec<String> = BATTERY_NAMES
        .iter()
        .map(|n| format!("-/sys/class/power_supply/{n}"))
        .collect();
    let mut got = read_write_paths(RESTORE);
    wanted.sort();
    got.sort();
    assert_eq!(got, wanted);
}
