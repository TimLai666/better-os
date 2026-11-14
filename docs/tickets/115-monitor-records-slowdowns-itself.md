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
explanation from the same values). Pressure stall information has two lines
per resource: `some` is the share of time at least one program waited, `full`
the share of time every program with work to do waited at once.

| Kind | Enters at | Holds while | Onset |
| --- | --- | --- | --- |
| Waiting for memory | memory `some` avg10 ≥ 20% or `full` ≥ 10% | half of either | 10 s |
| Everything waiting for the disk | io `full` ≥ 20% (`some` is not used) | half | 10 s |
| Waiting for the processor | cpu `some` ≥ 80% **and** 1-minute load ≥ 2 × logical CPUs | half of both | 30 s |
| Memory nearly gone | available ≤ 5% and swap in+out ≥ 100 pages/s | ≤ 10% and ≥ 50 pages/s | 10 s |
| A program ended for memory | `memory.oom.kill.rate` > 0 | the round it is seen | none |
| Too hot | `cpu.thermal_throttle.rate` > 0, each event holding 10 s | same | 10 s |
| The computer froze | the service's own 1 s round ≥ 30 s late on both clocks | that gap | none |

Why the lines are where they are: one large copy to a slow stick holds io
`some` at 80-90% while the desktop is fine, so the disk is judged on `full`;
a build on every core holds cpu `some` above 50% with about one runnable task
per core, so the processor needs 80%, a run queue twice the core count, and
30 seconds. Synthetic tests hold both (`a_big_copy_to_a_slow_stick_...`,
`a_build_on_every_core_...`) and the real stalls still record.

Clocks. Every duration is measured on the sample's monotonic clock, which
stops while the machine sleeps. A wall-clock gap more than 10 s larger than
the monotonic one is a suspend: the open slowdown ends where it was last seen
and nothing is counted across it. A gap on both clocks is the machine stuck:
it is the slowdown going on (held time includes it), and 30 s or more of it
is a slowdown by itself, `Stalled`. Samples 40 s apart under memory pressure
are one slowdown (`samples_forty_seconds_apart_under_memory_pressure_...`).

One slowdown is one incident: recorded when recognized, dated from when it
began, brought up to date every 5 minutes while it lasts and once when it
ends, which is after 60 seconds with nothing holding. A query answers the
slowdown going on as it stands that second, and the window counts an ongoing
one to now. A service that starts and finds a record still marked as going on
closes it. At most 24 are recorded in any 24 hours, counted from the store so
a restart does not reset it; slowdowns the cap kept out are written down too
and the page says how many in the last 24 hours. The store's bound of 200
incidents still applies. The window kept with a slowdown covers all of it
however long it lasted.

The process list is ranked by the leading cause's resource (memory, processor
time, disk bytes per second; processor time for a freeze), names and numbers
only. On an out-of-memory kill the list taken while memory was short is kept,
or else it is ranked from the round before the kill, so the program that was
using the memory is in it. systemd-oomd (Ubuntu's default) ends programs
without the kernel's `oom_kill` counter going up, and nothing an unprivileged
reader can see records it (its cgroup kill bumps no `memory.events` counter;
the user manager's unit results and the journal are the only traces), so it is
not detected: the rules say so, and a memory-pressure record at ≥ 50% for 20 s
or more (oomd's default for a session) says oomd may have closed programs that
the record cannot see.

`cpu.thermal_throttle.rate` reads `core_throttle_count` once per physical
core and `package_throttle_count` once per package (by `topology/core_id` and
`physical_package_id`), from paths worked out when the set of online CPUs
changes. Only counters read on both rounds are compared and a counter that
went down adds nothing, so a CPU going offline or online is no jump. AMD
processors and most virtual machines publish no counters; the metric is then
unsupported and the page says only some processors can be watched.

Cost, measured in a release build: the detector takes 67 ns per round when
nothing is happening and about 100 ns during a slowdown (it allocates nothing
per round; metric identifiers are made once); ranking 1,000 processes when a
slowdown begins or gets worse takes 16 µs; reading the throttle counters of a
24-CPU, 12-core machine takes 21 µs per round (13 files), and nothing on a
machine without them.

Storage. A slowdown's changes are appended to `slowdowns.log` beside the
other logs (one small record each; compacted on open to the latest per
incident), applied to the store in memory at once, and written to disk after
the engine's lock is let go (`spawn_blocking`), so an I/O stall during
`sync_data` holds up no sampling or query. Recording a new slowdown still
appends its first record to `incidents.log` under the lock, as every sample
append is. The store schema stays 2. A Better Monitor from before ticket 115
ignores `slowdowns.log`; when it rewrites `incidents.log` (at its 200-record
bound) and drops the `detection` it cannot read, the next open of this
version puts it back from `slowdowns.log`. The fixtures for that are what
v0.2.13's own store code wrote and rewrote, built from `git archive v0.2.13`
and run on a store this version wrote (`history.rs` tests).

`MarkIncident` still parses, so a window from before this change gets
`monitor.ipc.error.manual_marking_removed`; that window words an unknown
refusal as "Better Monitor didn't answer", which only an update fixes (and an
update restarts the window). `better-monitor-cli mark` is kept hidden and only
says that slowdowns are recorded automatically.

Screenshots in the ticket worktree's `.shots/`, from a headless GNOME Shell
46: `events-recorded-{en,zh}{,-2,-3,-4}.png` (a live slowdown still going
on, from this build's service reading a captured machine with memory stalled;
the daily-cap line; a freeze; the oomd sentence; a record marked by hand) and
`events-none-{en,zh}.png`.
