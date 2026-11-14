# Ticket 115 — Better Monitor records slowdowns by itself

**Blocked by:** none · **Status:** implemented, not merged

## Why

The owner decided on 2026-11-14: the slowdown record ("變慢記錄") should be
written automatically, and the person should not be able to add one by hand.
A manual "剛剛變慢了" (Just slowed down) button can be pressed by accident,
and a person presses it after the fact, so the moment it marks is late; the
service sees the slowdown as it happens.

Today an incident exists only when someone presses the button in the window
(`crates/monitor-gui/src/render.rs` ~248, `app.rs` `mark_incident`) or runs
`better-monitor mark` (`crates/monitor-cli/src/lib.rs` ~343), through the
`MarkIncident` request (`crates/monitor-ipc/src/lib.rs` ~115). The service
already samples what a detector needs, including pressure stall information
(`crates/monitor-service/src/inventory.rs`, `crates/monitor-store/src/sample.rs`,
`incident.rs`), and the window already words the state as "Your computer is
slowing down: programs are waiting for …" (`monitor-gui/src/i18n.rs` ~1150).

## What to build

- The service detects a slowdown itself and records an incident with the
  samples around it, with no window open: sustained pressure (CPU, memory or
  I/O "some"/"full" stall above a threshold for several seconds), memory
  nearly exhausted with swap activity, an OOM kill, or thermal throttling,
  using signals the service already reads or can read without privileges.
  Thresholds have hysteresis: one incident per slowdown, extended while it
  lasts, with a quiet period before a new one; a brief spike is not one.
- Each incident says in plain words what it was (which resource programs were
  waiting for, how long, how bad) and which processes were using the most of
  that resource at the time.
- The manual button and its intro copy are removed from the window, the tray
  and any menu; the Events page says the records are made automatically and
  what counts as a slowdown. `better-monitor mark` is removed too, and the
  `MarkIncident` request is no longer accepted from clients (an older window
  that sends it gets a clear refusal, not a crash). Records already made by
  hand are kept and shown, labelled as marked by hand.
- Storage stays bounded: the existing retention applies, plus a cap on
  incidents per day so a machine that is always under pressure does not fill
  the store.
- The detector costs nothing measurable: it runs on the samples the service
  already takes.
- English and Traditional Chinese (Taiwan usage).

## Acceptance

- [x] Unit tests on synthetic sample streams: sustained pressure makes one
      incident; a spike makes none; a long slowdown is one incident, not many;
      two slowdowns separated by quiet make two; the daily cap holds; each
      trigger kind is named correctly.
- [x] An integration test with the service on a private bus and fake
      readings: an incident appears without any client asking; `MarkIncident`
      is refused.
- [x] Screenshots of the Events page with automatic records and with none, in
      both languages, from a headless sandbox with a fixture store.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

What counts, in `monitor_store::SlowdownRules` (the Events page writes its
explanation from the same values):

| Kind | Enters at | Holds while |
| --- | --- | --- |
| Waiting for memory | PSI memory `some` avg10 ≥ 20% or `full` ≥ 10% | half of either |
| Waiting for the disk | PSI io `some` ≥ 50% or `full` ≥ 20% | half of either |
| Waiting for the processor | PSI cpu `some` ≥ 50% (`full` is undefined system-wide) | half |
| Memory nearly gone | available ≤ 5% of total and swap in+out ≥ 100 pages/s | ≤ 10% and ≥ 50 pages/s |
| A program ended for memory | `memory.oom.kill.rate` > 0 | the round it is seen |
| Too hot | `cpu.thermal_throttle.rate` > 0, each increase holding 10 s | same |

A kind has to stay entered for 10 seconds before it is a slowdown (the kernel's
avg10 is already a ten-second average, so a 3-second full stall stays under
the line); an out-of-memory kill counts at once. One slowdown is one incident:
recorded when recognized, dated from when it began, rewritten every 5 minutes
while it lasts and once when it ends, which is after 60 seconds with nothing
holding. A hole of more than 30 seconds between samples (suspend, stopped
service) ends it where watching stopped; a service that starts and finds a
record still marked as going on closes it. At most 24 are recorded in any 24
hours, counted from the store so a restart does not reset it; the store's
existing bound of 200 incidents still applies. The detector reads only the
round the service already took; it costs about 0.7 µs per round (release
build, synthetic samples).

`cpu.thermal_throttle.rate` is new: the sum of the world-readable
`core_throttle_count` and `package_throttle_count` under
`/sys/devices/system/cpu/cpuN/thermal_throttle`, which Intel processors
publish. AMD processors and most virtual machines have no such counters, so
the metric is unsupported there and thermal throttling is not detected; the
page says so.

Compatibility: `Incident` gained an optional `detection` and `ProcessSample`
an optional `disk_bytes_rate`, both skipped when absent, so the store schema
stays 2. The released 0.2.13 service was started in the sandbox on a store
this build wrote and listed all five incidents with nothing set aside. If an
older build rewrites the incidents log (it does when it trims to 200), the
detection it does not know is dropped and those records read as marked by
hand afterwards. `MarkIncident` still parses, so a window from before this
change gets `monitor.ipc.error.manual_marking_removed` instead of a parse
error; that window words an unknown refusal as "Better Monitor didn't
answer", which only an update fixes (and an update restarts the window).

Screenshots: `.shots/events-recorded-{en,zh}{,-2,-3}.png` and
`.shots/events-none-{en,zh}.png` in the ticket worktree, from a headless
GNOME Shell 46 with a fixture store; the service was this build, so the
fixture record left "still going" was closed on start.
