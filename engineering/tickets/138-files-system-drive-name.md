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
