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
