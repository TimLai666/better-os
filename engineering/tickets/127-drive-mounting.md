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
