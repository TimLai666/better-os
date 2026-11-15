# Ticket 127 — Drives the person chose are mounted; the rest are not shown

**Blocked by:** none

## Why

The owner asked on 2026-11-15: every drive Better Files shows should already
be mounted, the person should be able to choose which drives are mounted, and
a drive that is not mounted should not appear in Better Files' sidebar.
Today ticket 122 lists internal partitions whether mounted or not.

## What to build

- **A per-drive choice "Mount automatically" (自動掛載)**, offered for every
  drive ticket 122 would list, in two places that share one setting: the
  drive's context menu in Better Files and a Drives section in Better
  Manager's System page. Defaults: removable drives and the system drive on;
  other internal partitions as GNOME already treats them (mounted at login
  only if the system mounts them today).
- **Mounted at login and on arrival, with no password each time.** A removable
  drive mounts through UDisks2 as the active session already may. An internal
  partition needs a persistent system entry: the choice is written by
  `manager-daemon` over D-Bus, authorized by polkit once, as a managed
  `/etc/fstab` entry (`UUID=`, `nofail`, `x-gvfs-show`, `x-udisks-auth` as
  needed, under `/mnt` or `/media/<user>`), marked as Better OS's and never
  touching a line Better OS did not write; turning the choice off removes the
  entry and unmounts the drive. Record the design in an ADR (fstab vs a
  udisks2 configuration file vs a systemd mount unit) before building it.
- **Not mounted means not shown.** The sidebar's Devices section lists only
  mounted drives; a drive switched off is reachable from Better Manager's
  Drives section (and a "Show other drives" entry in Better Files' Settings)
  so it can be turned back on. A drive whose mount failed is shown with the
  reason and a Retry, never hidden silently.
- **Uninstall** of the component that owns this removes every entry it wrote
  and restores what was there.

## Done when

- Tests for the fstab writer (only its own marked lines, atomic replace,
  refusal on a malformed file, removal), the daemon method's validation and
  polkit action, the sidebar filter, and the default choices.
- The container end-to-end check writes and removes an entry through the
  daemon and `findmnt --verify` accepts the file.
- Screens in zh-TW and English, light and dark, at 1600 and 960.

## Acceptance

- [x] Tests for the fstab writer (only its own marked lines, atomic replace,
      refusal on a malformed file, removal): `drive-mounts` `fstab` and
      `rules`, `manager-daemon` `drives::tests`.
- [x] The daemon method's validation and polkit action:
      `crates/manager-daemon/tests/drives.rs`, the real daemon service on a
      private bus with a fake UDisks2 answering `ResolveDevice`.
- [x] The sidebar filter and the default choices: `drive-mounts` `choice`,
      `files-gui` `drives_bus_tests` (a stick turned off, shown again with
      "Show other drives", turned on and mounted), the context menu tests.
- [x] The container end-to-end check writes and removes an entry through the
      daemon and runs `findmnt --verify` (`packaging/test-daemon-e2e.sh`, with
      the `e2e_udisks` stand-in). CI-only: this machine has no Docker daemon.
- [x] Screens in zh-TW and English, light and dark, at 1600 and 960, from a
      headless GNOME Shell 46 sandbox with the daemon's Drives1 on a private
      system bus writing fixture files.

## What was built

- **The decision** is ADR 0032: internal partitions are lines in a marked
  block of `/etc/fstab` (`UUID=… /mnt/better-os/<uuid> <type>
  nofail,x-gvfs-show,x-systemd.device-timeout=10s[,uid,gid] 0 0`), started
  and stopped as systemd's generated mount unit; a removable drive turned off
  is a `UDISKS_AUTO=0` line in `/etc/udev/rules.d/90-better-os-drives.rules`.
- **`drive-mounts`** (new crate) holds the drive listing moved out of Better
  Files (so Better Manager lists the same drives), the fstab block and rules
  file parsers and writers, the choice and sidebar rules, and the windows'
  calls to the daemon and to systemd.
- **`org.betteros.Drives1.SetMountAutomatically(uuid, on)`** in
  `manager-daemon`, polkit action `org.betteros.manager.drive-mounting`
  (`auth_admin_keep`). Only a hex-and-dash UUID crosses; the daemon asks
  UDisks2 what the drive is, refuses the system drive, a drive another fstab
  line names, and anything not a filesystem, replaces the file atomically
  only while it still holds what was read, and keeps one change at a time.
  The package's prerm on removal runs `--remove-drive-entries`.
- **Better Files**: the Devices section and This PC list only mounted drives,
  an internal drive that is on and did not mount (systemd's reason and
  Retry), and drives that are on and waiting (a stick before the desktop
  mounts it, a locked encrypted drive). The drive menu has "Mount
  automatically" (locked where the system decides) and Retry. "Show other
  drives" in the `…` menu, where Better Files keeps its settings, lists every
  drive and is remembered.
- **Better Manager**: System page, Drives section, one switch per drive with
  its state, a locked switch with the reason for the system drive and for a
  drive the system's own fstab line names, and Retry.

## Notes

- Better Files has no separate Settings page; "Show other drives" is in the
  `…` menu beside Language and Appearance, which is where its settings live.
- Nothing has mounted a real drive: the daemon's writes, the polkit gate and
  `findmnt --verify` run in the container (CI) and on private buses; the
  windows were driven against a stand-in daemon writing fixture files.
