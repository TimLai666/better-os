# Ticket 138 — The drive the running system lives on is named after that system

**Blocked by:** none

## Why

The owner's system partition has no filesystem label (Ubuntu-based installers,
Zorin's included, never set one), so Better Files names it by size
("263 GB 磁碟"). The owner decided on 2026-11-16: the drive the currently
running Linux lives on always shows the running system's name, whatever
distribution it is.

## What to build

- The `DriveKind::System` drive's name is the running system's name from
  `/etc/os-release` (`NAME`, e.g. "Zorin OS"; `PRETTY_NAME` only if `NAME` is
  missing), read through the one place that reads that file
  (`crates/better-core/src/host.rs`, per AGENTS.md) — no new reading of it.
- It applies to the running system's root drive only, whether or not it has a
  label; other Linux installs on other partitions keep their label or size
  name (their own os-release is not read).
- When `/etc/os-release` gives no name, the drive is named "System drive" /
  「系統磁碟」, never a guessed distribution.
- The same name in the sidebar, This PC tiles, the path bar's root crumb,
  Properties, and Better Manager's Drives section (ticket 127).

## Done when

- Tests: a labelled and an unlabelled root both show the os-release name; a
  missing NAME falls back to the fixed wording; a second Linux partition is
  not renamed.
- Screens in zh-TW and English.

## Notes

- `better_core::host` reads the file (`running_system_name`: `/etc/os-release`,
  else `/usr/lib/os-release`, `NAME` then `PRETTY_NAME`).
  `drive_mounts::listing::Drive::naming` decides every drive's name
  (`DriveName::System`, `UnnamedSystem`, `Label`, `Size`), and each window
  only puts it into its own words, so Better Files (sidebar, This PC tiles,
  the root crumb, unlock dialog and notices) and Better Manager's Drives
  section cannot disagree. The label itself is untouched, because the fstab
  and udev matching reads it.
- Properties: the details pane describes the entry under the cursor, and `/`
  is never an entry of any folder, so no Properties view names the system
  drive today; the spec says so. When a drive Properties view is built it
  takes its name from `Drive::naming` like the rest.
- The fallback wording is proved by tests only; the screenshots show the
  machine's own os-release (`Zorin OS`), since the file is not redirected in
  a sandbox. Screens are in `better-os-evidence/138/`: This PC, the root
  crumb with the sidebar, and Better Manager's Drives, in both languages.
