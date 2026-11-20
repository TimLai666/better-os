# Ticket 171 — Better Files never offers Eject or Format on the drive the system runs from

**Blocked by:** none

## Why

Found on 2026-11-21 while reviewing ticket 138. When the running system is on
a removable USB drive, UDisks2 does not flag it as a system device, so the
storage service lists it and Better Files pairs it with the system drive's
sidebar row. The menu for a device row hard-codes `ejectable: true,
formattable: true` (`crates/files-gui/src/render.rs`, the `NavTarget::Device`
arm, about line 403), so the drive the system is running from is offered
Eject and Format.

## What to build

- Whether a drive can be ejected or formatted comes from what is known about
  it (its kind, whether anything on it is the running system's root or another
  mounted system path, what the storage service reports), decided in one
  GPUI-free place, never from a constant.
- The system drive, and any drive holding a mounted system path (`/`, `/boot`,
  `/boot/efi`, `/home`, swap), offers neither; the menu says why in one line.
- The daemon and the storage service refuse the same requests if they are
  sent anyway.

## Done when

- Tests: the running system on a USB drive, a second partition of the system
  disk, an ordinary USB stick, a drive with an unknown state (shown as
  unknown, not offered).
- Seen in a sandbox with a fixture listing whose system drive is removable.

## Notes

- 2026-11-21, delivered. The rule is `storage_platform::system_use`
  (`SystemDrives`, `SystemUse`), fed by `/proc/self/mounts`, `/proc/swaps`
  and `/sys/class/block/*/slaves`; the spec is "Nothing the running system
  uses is ejected or formatted" in `engineering/specs/files-gui-policy.md`.
  It decides the drive listing's `ejectable` and `system_use`
  (`drive-mounts`), the storage service's report and its refusals
  (`storage.system.*` for eject, format and image writes), Better Files'
  menus (`context_menu::device_row_kind`, `drive_row_kind`), row eject
  buttons, session refusals, Write to Drive's targets and the direct UDisks2
  eject and unmount, and `manager-daemon`'s `SetMountAutomatically`
  (`system_drive`).
- Sandbox screenshots (fixture listing, wayhand headless sway) are in
  `/home/tim/Developer/better-os-evidence/171/`: the removable system drive's
  menu in English and zh-TW, an unknown drive's, and an ordinary stick's.
  With no storage service in the sandbox those rows are the drive listing's
  own; the storage-service row paired with the system drive is covered by
  `tests/context_menu.rs` and `integration_tests.rs`, not drawn.
- Known limits, in the spec: a system path on ZFS or bcachefs makes every
  drive unknown; a swap file is not traced.
- 2026-11-21, review fixes (items 1 to 6): the storage service's reports
  carry no new field (a released Better Files reads them strictly; reports a
  client reads are no longer `deny_unknown_fields` from now on, and
  `crates/storage-service/tests/released_client.rs` reads the replies with
  v0.2.16's own copied types); Better Files judges the service's rows by its
  own scan of the same rule, by drive object. The rule is the right way
  round (`is_person_media`): anything mounted outside `/media`, `/run/media`
  or `/mnt/better-os`, a foreign fstab line (`storage.system.table`), swap
  partitions and swap files, loop devices through their backing file, ZFS
  pools by `zfs_member` label, bcachefs members, and sources found by device
  number (`/dev/root`). Better Manager's Drives section reads the same rule
  and mounts or unmounts only after the daemon made the change. Format and
  Write to Drive carry the identity confirmed: the dialog closes when its
  device goes or is replaced, the session compares again, and the service
  refuses `storage.erase.device_changed` (a service older than the field is
  asked again without it). Tests for each; the running host's own tables
  read every held mount (`--ignored` test, run once).
- Checks after the review round, on the final tree: `cargo fmt --all
  --check`, `cargo +stable clippy -D warnings --all-targets` and `cargo test
  --no-fail-fast` for every crate changed and those depending on them (1840
  passed, 0 failed). The sandbox screenshots of the first round stand: the
  fixture listing's menus go through `drive_row_kind`, which this round did
  not change.
