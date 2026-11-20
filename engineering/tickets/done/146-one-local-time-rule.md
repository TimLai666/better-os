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
  It asks the C library's `localtime_r` for each timestamp, so `TZ`,
  `/etc/localtime`, every TZif version and the zone's daylight-saving rules
  are the system's own, and two times on either side of a change each get
  their own offset. With no zone data (no `TZ` and no `/etc/localtime`, or a
  `TZ` the library could not load) the time is UTC, `LocalTime::utc_fallback`
  is set, and the formatted forms end in ` UTC`. `TimeZone::Fixed` is the
  deterministic zone tests use.
- The way back is `TimeZone::resolve`, built on `localtime_r` alone (no
  `mktime`, whose answer for a repeated hour depends on the conversion before
  it). It returns `WallTime::Once`, `Twice { earlier, later }` for a time a
  change repeats, or `Skipped { after_gap }` for one it skips (02:30 becomes
  03:30). `TimeZone::next_wall_time` is the keep-awake "until 07:30" rule: the
  first reading after now (in a repeated hour, the first one still to come),
  otherwise tomorrow's, read on tomorrow's clock; nothing falls back to "now".
- `tzset` runs at most once a second (`ZONE_RECHECK`), not per timestamp: it
  costs 926 ns against `localtime_r`'s 73 ns (the review's measurement), and
  Better Files converts per row. A timezone changed while a program runs shows
  in its next conversion at most one second later; `reload_zone` forces it
  (tests use it after setting `TZ`). The bench was not run, because the call
  is no longer on the per-row path.
- Threads and `TZ`: the C library reads `TZ` with `getenv`, which races with
  `setenv` on another thread. No Better OS program sets an environment
  variable after it starts: the workspace's only `set_var` calls are in tests
  (`touchpad-scroll/src/notify.rs` is one, and converts no time). Every test
  binary that sets `TZ` holds one lock around every conversion it makes
  (`better-core/tests/local_time.rs`, `awake-platform`'s schedule tests, whose
  sampling test now takes the lock too) or is a single test
  (`files-platform/tests/trash_local_time.rs`,
  `git-platform/tests/trash_local_time.rs`). The invariant is in the
  module documentation.
- Replaced: the three TZif readers (`awake-gui/src/localtime.rs`,
  `awake-tray/src/localtime.rs`, `manager-gui/src/pages/keep_awake/localtime.rs`,
  which read only the version 1 block and applied the offset in force "now" to
  every timestamp), `manager_core::login_watch::utc_time`, and the further
  copies the search found: `awake-platform`'s schedule `localtime_r` call,
  `files-operations`' zip timestamp `localtime_r`/`mktime`, `files-gui`'s
  UTC-only modified column, `files-platform`'s `.trashinfo` date (read and
  written as UTC, although the trash specification and every file manager use
  local time), `git-platform`'s `.trashinfo` date, and Better Git's
  licence year (`git-gui/src/publish.rs` `current_year`, which divided by an
  average year in UTC and was wrong around every New Year: the first eight
  hours of 1 January in Taipei, and even in UTC it called the last second of
  2025 2026 and the first of 2024 2023).
- `better-manager-cli doctor` prints the login screen watch's times on the
  local wall clock, like the window.
- Better Awake's schedule provider no longer runs on UTC when the machine has
  no timezone data: it reports itself unavailable with
  `awake.provider.no_time_zone`, shown as "This computer has no time zone
  set, so the local time a schedule needs is not known." / 「這台電腦沒有設定
  時區，所以不知道排程需要的當地時間。」
- Every provider reason key is a sentence now, in one table both windows use
  (`awake_ipc::reasons`, used by `awake-gui` and the Keep awake page; the tray
  shows no reasons). A test collects every `awake.provider.*` key from
  `awake-platform` and `awake-core` and fails when one lacks either sentence;
  an unknown key reads "Unknown reason (key)" / 「原因不明（key）」.
- Known effect (also in `engineering/specs/files-gui-policy.md`): an item
  Better Files or Better Git 0.2.16 and older trashed carries a UTC
  `DeletionDate` and now reads as local time, so its deleted time shows
  shifted by the UTC offset. Nothing is lost; an old stamp cannot be told from
  a new one, so none is corrected.
- Tests: `crates/better-core/src/local_time.rs` (arithmetic, leap days,
  weekday, the trash stamp, the missing-zone rule) and
  `crates/better-core/tests/local_time.rs` (spring forward, fall back, the
  southern hemisphere, a wall time read back on both sides of a change, a zone
  that cannot be loaded, a zone that is UTC on purpose, a skipped and a
  repeated wall time whatever was converted before, "until" across spring
  forward and fall back, a zone change while running), all with POSIX `TZ`
  rule strings, so no tzdata is needed. The `.trashinfo` writers are tested
  whole under `TZ=CST-8` (`files-platform/tests/trash_local_time.rs`,
  `git-platform/tests/trash_local_time.rs`), so a writer that went back to UTC
  fails. `crates/better-core/tests/one_local_time.rs` reads every Rust file and
  fails on the old helpers and their kin outside the one module: `UtcOffset`,
  `/etc/localtime`, `localtime_r`, `localtime(`, `mktime`, `gmtime`, `timegm`,
  `strftime`, `tm_gmtoff`, `fn tzset`, `civil_from_days`, `days_from_civil`,
  `719_468`/`719468`, `31_556_952`/`31556952`, `365.2425`,
  `div_euclid(86_400)`, `rem_euclid(86_400)`, `utc_time`, `clock_time`,
  `calendar_time`.
- Searched again after review for average year or month lengths
  (`31_556_952`, `365.24`, `2_629_746`, `30.44`), day arithmetic
  (`/ 86_400`, `div_euclid`/`rem_euclid` by a day, `% 86_400`, `1970 +`), and
  `% 7` weekdays across `crates/`. Besides the licence year, every hit is a
  relative age or a duration and converts nothing: `manager-gui` `model.rs`
  (catalog age), `home.rs` and `default_apps/model.rs` ("n days ago"),
  `monitor-gui` `i18n.rs` (uptime and durations), the Better Git history ages
  (`history.rs`, months as 30 days on purpose for "3mo ago"), `awake-core`
  `rules.rs` (`Weekday::previous`), a test loop in `git-core/tests/lanes.rs`, a
  benchmark in `launcher-core`; also `awake-gui` `pages_rules.rs` formatting a
  rule's minute of the day, `files-preview` media durations, and `better-ui`'s
  time picker.
- Not looked at on screen: the windows' strings keep their format, and the
  Files modified and deleted columns now show local time; no window was opened
  for this ticket.
