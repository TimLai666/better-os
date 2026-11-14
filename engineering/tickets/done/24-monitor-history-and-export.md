# 24 — Monitor service, history store, incidents, redacted export, CLI

**Epic:** Better Monitor (Issue #16)
**User Story:** A user closes the monitor window, hits a slowdown an hour later,
marks it, and can still see what the machine was doing around that moment — then
export a redacted package of it without leaking a path or a token.
**Blocked by:** 22-monitor-metric-contracts

## Goal

Move collection out of the GUI's lifetime and into a service, store a short
window of history, let the user mark an incident, and export a self-describing
redacted package. Issue #16 states the ownership rule directly: the service, not
the GUI window, owns historical collection.

## What it delivers

- `monitor-service`: long-running collection and coordination that starts with
  the user session and keeps sampling after the GUI closes. All high-cost
  collection is cancellable and bounded.
- `monitor-ipc`: the typed local protocol between service, GUI, and CLI. The GUI
  reads through it and never collects directly.
- `monitor-store`: versioned, migratable storage with fixed-resolution recent
  data, an explicit short retention window and disk budget, monotonic and
  wall-clock timestamps, restart-safe writes, corrupted-tail recovery, and
  per-metric coverage metadata. No unbounded per-process history by default.
- Incident marking: a timestamp, an optional note, a configurable window before
  and after the marker, the app/process/resource summary at that moment, the
  active pressure, thermal, storage, service, and collector states, and changes
  from the recent baseline. The action is reachable quickly from the main window.
- History and Inventory pages in the GUI, with a versioned diffable inventory of
  OS, kernel, desktop session, compositor, display protocol, hardware
  identities, filesystems and mounts, Better OS component versions, and the
  observation capabilities that are unavailable.
- `monitor-export`: a bounded, explicitly user-triggered export package with a
  selectable time range and data classes, containing at minimum manifest,
  schema, inventory, samples, incidents, coverage, and a redaction report.
  Redaction covers tokens, sensitive command arguments, personal paths,
  addresses, and identifiers, and the result is previewable before it is
  written. Observation gaps are included so missing data is never read as zero.
  No automatic upload.
- `monitor-cli`: `inspect`, `record`, `mark`, `export`, and `doctor`, sharing the
  same core contracts as the GUI.

## Out of scope

- Collectors and metric contracts (ticket 22).
- Apps, Processes, and Overview views (ticket 23).
- Anomaly and regression detection, and the anomalies file in the export.
- Deep profiling, the profiles directory, perf, and eBPF.
- Long-term retention, downsampling, and inventory diffs beyond the short
  retention window.
- Any network upload path.

## Deferred decisions

Issue #16 requires an ADR or a focused follow-up issue rather than a silent
choice for: the final storage engine and downsampling format — SQLite is a
candidate, not a default — the default retention duration and disk budget, and
whether the historical service and the GUI ship in one package or several. The
store must stay replaceable until the storage ADR lands, matching the existing
`engineering/architecture.md` migration note.

## Acceptance criteria

- [x] Historical collection continues after the GUI closes, proven by a test
      that starts the service, closes the GUI, and finds later samples.
      `collection_continues_after_every_client_has_disconnected` drives the
      engine directly and
      `collection_continues_after_a_bus_client_disconnects` does it over a
      private session bus, connecting, reading, dropping the connection, and
      asserting both the round counter and the store kept growing with no gap
      recorded across the disconnect.
- [x] The GUI reads history through the typed IPC and never collects directly.
      `monitor-gui` has no sampler of its own any more: `src/link.rs` reaches
      `monitor-service` over `org.betteros.Monitor1`, and when there is no
      service it starts an embedded `MonitorEngine` — the same engine, in this
      process, with a banner saying recording stops when the window closes.
- [x] The store is versioned and migratable, and a corrupted tail is recovered
      rather than discarding the file. Each log carries a schema stamp, a newer
      one is refused and kept, and a truncated final record is dropped with the
      hole recorded as an `interrupted_write` gap.
- [x] Retention and disk budget are explicit and enforced; per-process history
      is bounded by default. Six hours, 64 MiB, ten processes per sample, all
      in `RetentionPolicy` with the measured numbers behind them in ADR 0011.
- [x] A user can mark a slowdown incident and inspect the interval around it.
      The Incidents page's button sends the marker; the service captures the
      snapshot, the collector states, the bounded process list, and the shift
      from the preceding baseline.
- [x] An export contains schema, inventory, samples, incidents, coverage, and a
      redaction report.
- [x] An export contains no known test secret and no unapproved sensitive
      command argument, proven by a seeded-secret test. The secret is planted
      in a process command line and in an incident note, and every byte of
      every file in the package is searched.
- [x] Observation gaps appear in the export so missing data cannot be mistaken
      for zero. `coverage.json` carries the gap list, the samples present
      against the samples the resolution implies, and per-metric counts of all
      five observation states.
- [x] Export requires an explicit user action and never uploads anything. There
      is no network code in `monitor-export` and no request that reaches one.
- [x] The CLI provides `inspect`, `record`, `mark`, `export`, and `doctor`, and
      shares the same core contracts as the GUI. Each says whether it answered
      from the service or from the store on disk.
- [x] History and Inventory pages render, and `zh-TW` and `en-US` pass overflow
      tests at 100%, 125%, and 150% scaling. Rendering is proved by an
      8-second `ZED_HEADLESS=1` launch rather than by a screenshot; the
      overflow rules are asserted directly.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- A GUI-closed collection smoke: start the service, close the GUI, confirm later
  samples exist and the timeline is unbroken
- A seeded-secret export test asserting the redaction boundary holds
- `cargo bench -p monitor-store`: history write and query p50/p95 latency,
  database growth over the retention window, and export creation over a large
  time range
- CLI smoke covering all five subcommands against a disposable store


## What was built

`monitor-store` — three framed append logs with a downsampler in front and a
compaction pass behind. `monitor-ipc` — JSON documents over a session D-Bus
interface, matching `awake-ipc`'s choice and ADR 0007's shape.
`monitor-service` — the engine that owns the collectors, the two observation
layers, and the store. `monitor-export` — the self-describing redacted package.
`monitor-cli` — five subcommands over the same contracts.

The GUI gained History, Incidents, and Inventory pages and lost its sampler.

## Interim decisions recorded

ADR 0011 records the append-log store as the interim engine with the measured
numbers behind it, and states plainly that the final engine choice is still
owed. SQLite could not be benchmarked here: crates.io is unreachable from this
environment and `rusqlite` is in neither the lockfile nor the local cargo cache.

## Not done, and named rather than assumed

- Anomaly and regression detection, `anomalies.json`, deep profiling, and the
  `profiles/` directory are out of scope and absent, not stubbed.
- The export is a directory rather than an archive. There is no compression
  dependency in the workspace.
- Export is reachable from the CLI but not from a button in the GUI.
- An export's progress is reported as completed or failed. Nothing streams a
  percentage, because the work runs to completion before the reply is sent.
- Packaging — a systemd user unit for the service, a desktop entry, and a
  Better Manager manifest — is not written.
- The service and the GUI can both write the default store if the service is
  started while a fallback window is open. The rule is one writer, and both the
  CLI and the window check for the service first, but nothing enforces it with
  a lock.

## Notes

Carried over from the retired delivery status record.

Five new crates are on branch `ticket-24`: `monitor-store`, `monitor-ipc`,
`monitor-service`, `monitor-export`, and `monitor-cli`. `monitor-gui` changed
substantially and nothing else outside those six did, apart from the workspace
member list, `engineering/architecture.md`, `AGENTS.md`, this file, the ticket, and the new ADR.

Collection no longer belongs to the window. `monitor-gui` has no sampler:
`src/link.rs` reaches `monitor-service` over the session bus name
`org.betteros.Monitor1`, and where there is no service it starts the same
`MonitorEngine` inside the window process and says so in a banner on every
stored-data page. Either way the collectors live behind the bridge and the
render thread never waits on a bus or a disk. The old `sampling.rs` is deleted
rather than left as a second path that could drift.

The milestone signal is proved twice: once against the engine directly and once
over a private session bus, where a client connects, reads, drops its
connection, and a second client opening later finds the round counter and the
store both grew with no gap recorded across the disconnect. The 8-second
headless launch also proves it from the other end — the window's fallback
engine wrote 34 KiB of history and captured an inventory in those 8 seconds.

The store is an append log of length-and-checksum framed JSON records, three
files with different lifetimes, a downsampler in front and a compaction pass
behind. A torn final record is truncated and the hole is recorded as a gap; a
file a newer Better Monitor wrote is refused and kept. ADR 0011 records this as
the **interim** engine with the measured numbers behind it and states plainly
that the final choice is still owed. SQLite could not be benchmarked: crates.io
is unreachable from this environment and `rusqlite` is in neither the lockfile
nor the local cargo cache, so the ADR records a baseline SQLite would have to
beat rather than a comparison it did not run.

Measured, on this machine, release profile: a durable append is 1.1 ms at p50
and six hours of history is 4,320 samples in 35.7 MiB, which is where the
64 MiB default budget came from. A five-minute query is 0.3 ms, a six-hour
query is 50 ms, and reopening six hours takes 576 ms — the last is the weakest
number and the one the final engine decision should be judged on. Writing an
export of six hours takes 95 ms and produces 11.4 MiB.

Redaction is a boundary rather than a filter. Command-line arguments are
dropped whole rather than scanned, because arguments are where credentials
live. The seeded-secret test plants a token in a process command line and in an
incident note and searches every byte of every file in the produced package,
including the schemas and the README.

Gates that actually ran on this branch, all passing. Crate-scoped first,
because a workspace-wide run rebuilds the GPUI world for each command:
`cargo clippy --all-targets -- -D warnings` and `cargo test` for
`monitor-store`, `monitor-ipc`, `monitor-export`, `monitor-service`,
`monitor-cli`, `monitor-gui`, `monitor-core`, `monitor-collectors-linux`, and
`monitor-views` — 426 tests across those nine, of which 166 are new.

The full gate then ran as well, once the GPUI world was already built:
`cargo fmt --all -- --check`, `cargo check --workspace`,
`cargo test --workspace` (987 tests, all passing; 821 on `main` before this
ticket), and `cargo clippy --workspace --all-targets -- -D warnings`.

Also run: `cargo build -p monitor-gui`; an 8-second `ZED_HEADLESS=1` launch that
stayed alive with no output and left 34 KiB of history and an inventory capture
behind it, which is the fallback engine proving itself; `cargo bench -p
monitor-store`; and `cargo bench -p monitor-export`.

What ticket 24 did not do, and should not be assumed done: anomaly and
regression detection, deep profiling, and the `anomalies.json` and `profiles/`
parts of the export are out of scope and absent rather than stubbed. The export
is a directory, not an archive, because the workspace has no compression
dependency. Export is reachable from the CLI but there is no button for it in
the window. Export progress is reported as completed or failed; nothing streams
a percentage, because the work runs to completion before the reply is sent.
Packaging is not written: no systemd user unit for the service, no desktop
entry, no Better Manager manifest. Nothing enforces the single-writer rule with
a lock — the CLI and the window both check for the service first, but starting
the service while a fallback window is open would put two writers on one store.
The GUI's command-line privacy toggle only takes effect in the fallback engine;
against a running service it is refused with a stable key, because a window
must not change what the session service is collecting.
