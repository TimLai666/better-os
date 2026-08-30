# 22 — Monitor metric contracts and real Linux collectors

**Epic:** Better Monitor (Issue #16)
**User Story:** Better Monitor reports what the machine is actually doing, from
authoritative kernel interfaces, and says plainly which numbers it cannot get
instead of showing a zero.
**Blocked by:** none
**Status:** todo

## Goal

Replace the mock `Sample` model with typed metric and capability contracts, then
land the first real Linux collectors on top of them. Issue #16 is explicit that
the existing public types are not the final telemetry schema and may be replaced
or migrated.

## What it delivers

- Typed metric identity contracts in `monitor-core` replacing the mock `Sample`
  model. Every metric definition carries unit, semantic type, source, support
  state, and sampling behavior.
- Unknown, unsupported, permission denied, stale, and zero are five distinct
  states in the type system, not one nullable number.
- `monitor-collectors-linux` with the first production collectors:
  - CPU: total and per-logical-CPU utilization; user, system, idle, I/O wait,
    IRQ, soft IRQ, steal, and guest categories where available; load average;
    current/base/max clock and package/core temperature where available.
  - Memory: total, available, used, cache, buffers, reclaimable, and slab; swap
    and zswap/zram where available; page-in/page-out and major-fault rates.
  - PSI from `/proc/pressure/*` for CPU, memory, and I/O.
  - Processes: identity, PID and parent PID, user, state, CPU usage and
    accumulated time, RSS and the available memory estimates, swap, read/write
    throughput and totals, threads, file-descriptor count, start time and
    runtime, command and executable path, cgroup/unit/container/sandbox
    identity, priority and nice value.
  - Storage throughput at the device level, kept separate from filesystem
    capacity.
  - Network interfaces: per-interface throughput and totals, packets, errors,
    drops, link speed and connection type.
- Collectors read `/proc`, `/sys`, cgroup v2, and PSI directly. No scraping of
  human-formatted CLI output where a structured interface exists.
- Sampling is time-correct: irregular intervals are recorded as irregular, never
  treated as evenly spaced.
- Collector fixtures: recorded `/proc` and `/sys` trees that let every collector
  be tested for semantic correctness without the host's live state.
- Per-collector overhead benchmarks and the benchmark harness the later monitor
  tickets extend.
- A source traceability record per collector, in the form Issue #16 mandates:
  feature id, upstream project or platform specification, pinned commit or
  documentation version, file and line range when source was studied, adoption
  mode, license and attribution status, known semantic differences, and the
  tests proving Better Monitor's interpretation.
- Collectors carry no GPUI dependency.

## Out of scope

- Apps and Processes views, grouping, and process actions (ticket 23).
- The service, history store, incidents, export, and CLI (ticket 24).
- GPU collectors and the adapter framework.
- Energy and battery collectors, SMART/health adapters, per-process network
  attribution, and per-process GPU attribution.
- Deep profiling, perf, and eBPF.
- Anomaly and regression detection.

## Deferred decisions

Issue #16 requires an ADR or a focused follow-up issue rather than a silent
choice for: default sampling intervals and adaptive policy, the exact production
GPU adapter set, the exact process memory model shown by default, the
per-process network attribution mechanism, and perf/eBPF integration and its
privilege model. Whether `sysinfo` becomes a direct dependency or individual
metrics are read directly also needs a recorded decision with its reasoning, not
an import.

## Acceptance criteria

- [ ] The mock `Sample` model is replaced by typed metric and capability
      contracts carrying unit, semantic type, source, support state, and
      sampling behavior.
- [ ] Unknown, unsupported, permission-denied, stale, and zero are distinct and
      each has its own test.
- [ ] Real CPU, memory, PSI, process, storage-throughput, and network-interface
      data is collected from authoritative Linux interfaces.
- [ ] No collector claims a metric is available when the interface cannot
      provide or verify it.
- [ ] Collector fixtures reproduce each collector's parsing and semantics
      without depending on the host's live state.
- [ ] Sample timestamps carry real intervals; a test proves an irregular
      interval is not normalized into an even one.
- [ ] Every production collector has a source traceability record with all eight
      fields Issue #16 lists.
- [ ] Every production collector has an overhead measurement on recorded
      reference hardware.
- [ ] Collectors have no GPUI dependency.
- [ ] Any adapted `bottom` code or new direct dependency is attributed and
      audited, and no GNOME Resources or Mission Center code or asset is copied.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p monitor-collectors-linux` against the recorded fixture trees
- `cargo bench -p monitor-collectors-linux`: idle background collection cost per
  collector, and collection at 100, 1,000, and 10,000 processes
- A collection-accuracy check comparing collector output against the
  authoritative counter it derives from
