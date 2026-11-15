//! `/sys` is read-only to the daemon and to the boot-time restore (ADR 0029):
//! both units bind it read-only (`BindReadOnlyPaths=/sys`, the one form
//! systemd 255 does not drop beside the API file systems that
//! `ProtectKernelTunables=` and `ProtectControlGroups=` imply), keep
//! `ProtectKernelTunables=` for `/proc/sys`, and open nothing under `/sys`.

const DAEMON: &str = include_str!("../../../packaging/daemon/better-manager-daemon.service");
const RESTORE: &str = include_str!("../../../packaging/daemon/better-hardware-restore.service");

fn values(unit: &str, key: &str) -> Vec<String> {
    unit.lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .flat_map(|paths| paths.split_whitespace().map(str::to_string))
        .collect()
}

#[test]
fn both_units_keep_sys_read_only_and_open_nothing_under_it() {
    for unit in [DAEMON, RESTORE] {
        assert_eq!(values(unit, "BindReadOnlyPaths"), ["/sys"]);
        assert!(unit.lines().any(|l| l == "ProtectKernelTunables=yes"));
        assert!(values(unit, "ReadWritePaths").is_empty());
        assert!(values(unit, "BindPaths").is_empty());
    }
}

#[test]
fn the_restore_unit_runs_the_daemon_once_after_the_bus() {
    assert!(RESTORE.lines().any(|l| l == "Type=oneshot"));
    assert!(
        RESTORE
            .lines()
            .any(|l| l == "ExecStart=/usr/libexec/better-manager-daemon --restore-hardware")
    );
    assert!(RESTORE.contains("dbus.service"));
}
