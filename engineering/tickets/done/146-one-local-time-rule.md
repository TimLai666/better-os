# Ticket 146 — One place turns a timestamp into local time

**Blocked by:** none

## Why

Ticket 140's worker found three copies of the local-time conversion
(`awake-gui`, `awake-tray`, `manager-gui`), against AGENTS.md's rule that a
rule two places need lives in one place, and `better-manager-cli` can only
print UTC because the CLI has none of them.

## What to build

- One local-time function in a shared crate (`better-core` or `better-ui`'s
  GPUI-free part), tested across DST changes and with no timezone data
  (reports UTC and says so, never a guessed offset).
- Every copy replaced; the CLI prints local time like the windows.
- Search the workspace for further copies (strftime-like formatting, offset
  arithmetic) and fold them in.

## Done when

- One implementation, its tests, and no other conversion left (a test greps
  for the old helpers).

## Notes

- One implementation: `better_core::local_time` (`crates/better-core/src/local_time.rs`).
  It asks the C library (`localtime_r`/`mktime` after `tzset`) for each
  timestamp, so `TZ`, `/etc/localtime`, every TZif version and the zone's
  daylight-saving rules are the system's own, and two times on either side of
  a change each get their own offset. With no zone data (no `TZ` and no
  `/etc/localtime`, or a `TZ` the library could not load) the time is UTC,
  `LocalTime::utc_fallback` is set, and the formatted forms end in ` UTC`.
  `TimeZone::Fixed` is the deterministic zone tests use.
- Replaced: the three TZif readers (`awake-gui/src/localtime.rs`,
  `awake-tray/src/localtime.rs`, `manager-gui/src/pages/keep_awake/localtime.rs`,
  which read only the version 1 block and applied the offset in force "now" to
  every timestamp), `manager_core::login_watch::utc_time`, and the further
  copies the search found: `awake-platform`'s schedule `localtime_r` call,
  `files-operations`' zip timestamp `localtime_r`/`mktime`, `files-gui`'s
  UTC-only modified column, `files-platform`'s `.trashinfo` date (read and
  written as UTC, although the trash specification and every file manager use
  local time), and `git-platform`'s `.trashinfo` date. The keep-awake "until
  07:30" time is now found by reading 07:30 back through the zone for each
  day, not by adding a day of seconds to one offset.
- `better-manager-cli doctor` prints the login screen watch's times on the
  local wall clock, like the window.
- Tests: `crates/better-core/src/local_time.rs` (arithmetic, leap days,
  weekday, the trash stamp, the missing-zone rule) and
  `crates/better-core/tests/local_time.rs` (spring forward, fall back, the
  southern hemisphere, a wall time read back on both sides of a change, a zone
  that cannot be loaded, a zone that is UTC on purpose), all with POSIX `TZ`
  rule strings, so no tzdata is needed. `crates/better-core/tests/one_local_time.rs`
  reads every Rust file and fails on the old helpers (`UtcOffset`,
  `/etc/localtime`, `localtime_r`, `mktime`, `tm_gmtoff`, `civil_from_days`,
  `days_from_civil`, `719_468`, `utc_time`, `clock_time`, `calendar_time`)
  outside the one module.
- Searched and left alone, because they convert nothing: relative ages (the
  Better Git history ages, `manager-gui` "n days ago", `monitor-gui`
  durations), `awake-gui` `pages_rules.rs` formatting a rule's minute of the
  day, `files-preview` media durations, and `better-ui`'s time picker.
- Not looked at on screen: the windows' strings keep their format, and the
  Files modified and deleted columns now show local time; no window was opened
  for this ticket.
