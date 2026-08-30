# 24 — Monitor service, history store, incidents, redacted export, CLI

**Epic:** Better Monitor (Issue #16)
**User Story:** A user closes the monitor window, hits a slowdown an hour later,
marks it, and can still see what the machine was doing around that moment — then
export a redacted package of it without leaking a path or a token.
**Blocked by:** 22-monitor-metric-contracts
**Status:** todo

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
ENG.md migration note.

## Acceptance criteria

- [ ] Historical collection continues after the GUI closes, proven by a test
      that starts the service, closes the GUI, and finds later samples.
- [ ] The GUI reads history through the typed IPC and never collects directly.
- [ ] The store is versioned and migratable, and a corrupted tail is recovered
      rather than discarding the file.
- [ ] Retention and disk budget are explicit and enforced; per-process history is
      bounded by default.
- [ ] A user can mark a slowdown incident and inspect the interval around it.
- [ ] An export contains schema, inventory, samples, incidents, coverage, and a
      redaction report.
- [ ] An export contains no known test secret and no unapproved sensitive
      command argument, proven by a seeded-secret test.
- [ ] Observation gaps appear in the export so missing data cannot be mistaken
      for zero.
- [ ] Export requires an explicit user action and never uploads anything.
- [ ] The CLI provides `inspect`, `record`, `mark`, `export`, and `doctor`, and
      shares the same core contracts as the GUI.
- [ ] History and Inventory pages render, and `zh-TW` and `en-US` pass overflow
      tests at 100%, 125%, and 150% scaling.

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
