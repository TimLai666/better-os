# ADR 0008: Better Monitor history storage — interim engine and retention

## Status

Accepted as an **interim** decision. This is explicitly not the final storage
engine choice that Issue #16 requires.

## Context

Issue #16 says the store must support fixed-resolution recent data,
downsampled longer-term data, an explicit retention and disk-budget policy,
monotonic and wall-clock timestamps, restart-safe writes, schema migrations,
corrupted-tail recovery, and per-metric coverage metadata. It also says, in as
many words, that the exact database and time-series format require an ADR after
benchmarks, and that SQLite is a candidate rather than a silent default.

Ticket 24 needs a store now: the service, the incident marker, the export, and
the CLI all read and write history, and none of them can be built against a
decision that has not been made. `AGENTS.md` carries the same constraint from
the other direction — the storage engine and retention policy must be decided
in an ADR before any collector output is persisted.

There is a further constraint that shaped this decision rather than merely
delayed it: this environment has no network access to crates.io. `rusqlite` is
not in the lockfile and not in the local cargo cache, so SQLite could not be
benchmarked here even if the decision were ready to be made. That is a fact
about the measurement, and it is why this ADR does not claim to have compared
two implementations.

## Decision

Ship an append-only log of length-and-checksum framed JSON records as the
interim engine, and treat it as replaceable.

- Three files under `$XDG_STATE_HOME/better-monitor`: `history.log` (samples
  and gaps), `incidents.log`, and `inventory.log`. Separate because they have
  different lifetimes — a retention pass that drops yesterday's samples must
  not drop the incident somebody marked yesterday — and because a torn write in
  one must not cost the other two.
- Each file is a 16-byte header (magic, framing version, schema version)
  followed by records of `length | crc32 | payload`. Reading stops at the first
  frame that does not verify and truncates the file back to the last one that
  did. A half-written record at the end is the expected outcome of a power
  loss, and it is recovered rather than reported as corruption of the file.
- A downsampler sits in front of the log: the service samples every second and
  writes one stored sample per resolution period, averaging numbers and keeping
  the newest identity, stale, and non-value readings unchanged.
- Retention runs behind it: records outside the window are dropped, then the
  oldest are dropped until the file fits its budget, and what was dropped is
  recorded as a gap so a chart cannot draw a line across it.
- The retained window is also held in memory, which is what lets a query answer
  without re-reading the log.

### Defaults

| Setting | Value | Why |
| --- | --- | --- |
| Retention window | 6 hours | Short on purpose. The promise is "explain the slowdown you had this afternoon", not "keep a month of telemetry". A short default is the version of that promise a user does not have to opt out of. |
| Disk budget | 64 MiB | Measured six hours costs 35.7 MiB, so the budget holds the whole default window with room for a busier machine, and is small enough that a forgotten service cannot fill a disk. |
| Resolution | 5 seconds | One fifth of the collection rate. Fast enough to see a stall begin, slow enough that the disk sees a fifth of the writes the CPU does. |
| Tracked processes per sample | 10 | Bounded by policy. Issue #16 forbids unbounded per-process history by default, and the busiest few are what explain a slowdown. |
| Incidents kept | 200 | Bookkeeping, not history. |
| Inventory records kept | 64 | Written only when the machine actually changed. |

## Measurements

`cargo bench -p monitor-store` and `cargo bench -p monitor-export` on the
development machine (Zorin OS 18, NVMe, release profile). The synthetic round
is shaped like a real one: six collectors, 32 logical CPUs, three PSI
resources, eight devices, 400 processes.

| Measurement | p50 | p95 | Worst |
| --- | --- | --- | --- |
| Build a stored sample from one collector round | 0.091 ms | 0.124 ms | 0.324 ms |
| Durable append of one sample, including `fsync` | 1.139 ms | 1.909 ms | 65.1 ms |
| Query the last 5 minutes | 0.311 ms | 0.446 ms | 1.117 ms |
| Query the last hour | 7.63 ms | 11.23 ms | 13.08 ms |
| Query the whole six hours | 50.07 ms | 68.23 ms | 71.21 ms |
| Per-metric coverage over six hours | 41.66 ms | 47.50 ms | 59.23 ms |
| Reopen and recover six hours of history | 576 ms | 601 ms | 601 ms |
| Preview an export of six hours | 86.5 ms | 95.5 ms | — |
| Write an export of six hours | 95.4 ms | 107.0 ms | — |
| Write an export of fifteen minutes | 3.5 ms | 3.9 ms | — |

Sizes: six hours of history is **4,320 samples in 35.71 MiB** on disk, written
at about 574 samples per second. The export package for the same six hours is
**11.43 MiB**.

### What the numbers say

The write path is comfortable. One durable append every five seconds against a
1.1 ms median leaves three orders of magnitude of headroom, and the 65 ms worst
case is a filesystem flush stall rather than a cost that scales with the store.

The read path is where this engine will run out of road. A five-minute query is
0.3 ms and a six-hour query is 50 ms — linear in the retained window, because
answering means walking every retained record. Fifty milliseconds is three
dropped frames if it ever ran on the interface thread; it does not, but it is
also the number that would grow to half a second if the retention window grew
to two days.

Reopening is the weakest result: 576 ms to parse and verify six hours of JSON.
The service pays it once at login, which is acceptable, and every CLI
invocation pays it too, which is merely tolerable. It is linear in the window
for the same reason.

## Alternatives

**SQLite.** The candidate Issue #16 names. It would answer a range query with
an index rather than a scan, and would not re-parse the whole store to open it.
It could not be measured here, because crates.io is unreachable from this
environment and `rusqlite` is in neither the lockfile nor the local cargo cache;
it would also be the project's first C dependency, which needs its own licence
and packaging review. Nothing in these measurements argues against SQLite. What
they establish is a baseline it would have to beat, and where it would win:
open time and long-range queries, not append throughput.

**A binary columnar format.** Smaller and faster to scan than JSON, and a
natural fit for fixed-resolution numeric series. It is a poor fit for the thing
this store is actually built around: an observation is a five-way state, not a
number, and the reasons attached to `unsupported` and `permission_denied` are
variable-length text this project refuses to drop.

**Keep everything in memory and write on shutdown.** Simplest of all, and wrong
for the same reason the service exists: a machine that locked up hard is
exactly the case a user wants explained, and it is the case where no clean
shutdown happens.

## Consequences

- The store stays replaceable. Everything above it reads `HistoryStore`,
  `Sample`, `Gap`, `Incident`, and `Inventory`; nothing above it knows there is
  a log. Swapping the engine is a change inside `monitor-store`.
- The schema stamp in each file header is the migration seam, and it already
  refuses a file a newer Better Monitor wrote rather than overwriting it.
- Retention is bounded by measurement rather than by hope, and the numbers that
  set the defaults are in this document rather than only in a commit message.
- The final decision is still owed. It should be made after SQLite can actually
  be benchmarked in a network-capable environment, and it should be judged on
  open time and long-range query cost, because those are the two places this
  engine is weak.

## Follow-ups

- Benchmark SQLite against these numbers in an environment that can fetch it,
  and record the licence and packaging review a C dependency needs.
- Decide whether the retention window should be user-configurable, and whether
  a longer window implies downsampling a second time rather than a larger file.
- Reopen time is linear in the retained window. If the window grows, the store
  needs either an index or a snapshot of the retained set.
