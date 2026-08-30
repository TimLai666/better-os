# 23 — Monitor Apps and Processes views, safe actions, real Overview

**Epic:** Better Monitor (Issue #16)
**User Story:** A user can see which application is responsible for a slowdown,
understand why its processes were grouped that way, and stop it safely without
the GUI ever running as root.
**Blocked by:** 22-monitor-metric-contracts
**Status:** todo

## Goal

Turn the real collectors into the task manager: two separate views with
explainable grouping, a process detail with bounded actions, and an Overview
that distinguishes busy from stuck from unobservable.

## What it delivers

- Application grouping from evidence, in the priority order Issue #16 sets:
  cgroup v2 and systemd user-unit membership, Flatpak identity, Snap or
  application metadata, desktop application id and launch metadata, explicit
  parent/child relationships, and executable identity only as a fallback.
- Each group records why it was grouped, and the UI can show it. Processes are
  never merged solely because their executable names match.
- Apps view: total CPU, memory, disk, and network usage per app where
  attributable; expand an app to its processes; background services shown
  separately when no user-facing app identity is justified.
- Processes view: the columns ticket 22 collects, sortable and filterable, with
  the command and executable path behind privacy controls.
- Both tables virtualized, and responsive under the documented large-process
  benchmark.
- Process detail plus safe actions: terminate gracefully, force stop,
  pause/resume where supported, change priority within the allowed privilege
  boundary, open the executable or working location where meaningful, copy
  diagnostic details.
- Actions affecting another user or requiring elevation go through a narrow
  polkit-reviewed boundary. The GPUI process never runs as root and never
  retains a privileged handle, scrapes `/proc` itself, or executes a shell
  pipeline.
- Real Overview replacing the mock one: CPU utilization and pressure; memory,
  swap, reclaim, and pressure; active storage and I/O pressure; network
  throughput; top resource-consuming apps; observation health and missing
  collectors. It distinguishes high utilization without contention, actual
  pressure or waiting, thermal or power throttling, collector failure, and
  unknown or unsupported state.
- Navigation shell for the hardware pages, with a page that is unsupported on
  the current hardware explaining itself rather than rendering fabricated zeros.
- Collector-health and observation-coverage diagnostics surfaced in the UI.
- `zh-TW`, `en-US`, and system language with runtime switching; 100%, 125%, and
  150% scaling; keyboard-only operation; data-table alternatives to charts;
  charts readable without relying on color alone; the user can pause visual
  updates.

## Out of scope

- The history service, store, retention, incident marking, export, and CLI
  (ticket 24).
- GPU, Energy, and the richer Storage and Network pages beyond the collectors
  ticket 22 delivers.
- Anomaly and regression detection.
- Automatic process termination and any automatic optimization.
- Per-process network accounting.

## Deferred decisions

Issue #16 requires an ADR or a focused follow-up issue rather than a silent
choice for: the final application-grouping precedence and confidence thresholds,
the exact process memory model shown by default, and whether any GNOME Resources
or Mission Center code is directly reused. The grouping evidence order is
implemented as configurable precedence with recorded confidence, not as a
hard-coded final answer.

## Acceptance criteria

- [ ] The GPUI application displays real CPU, memory, PSI, process, storage, and
      network data from ticket 22's collectors.
- [ ] Apps and Processes are separate views.
- [ ] Every app group can state the evidence that produced it.
- [ ] Unrelated processes with matching executable names are not merged, proven
      by a test.
- [ ] Unknown, unsupported, stale, permission-denied, and zero metrics are
      visually and semantically distinct in the UI.
- [ ] Process and app tables are virtualized and stay responsive under the
      documented large-process benchmark.
- [ ] Terminate, force stop, pause/resume, and priority change work without the
      GUI running as root.
- [ ] Elevated actions go through a narrow reviewed boundary, and cancelling
      before approval mutates nothing.
- [ ] The GUI performs no `/proc` scraping, no shell pipeline, and holds no
      privileged handle.
- [ ] The Overview distinguishes high utilization without contention, real
      pressure, throttling, collector failure, and unknown state.
- [ ] Collector health and observation coverage are visible in the UI.
- [ ] Unsupported pages explain themselves and never show fabricated zeros.
- [ ] `zh-TW` and `en-US` layouts pass overflow tests at 100%, 125%, and 150%
      scaling.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `monitor-gui`
- Large-process benchmark at 100, 1,000, and 10,000 processes in a synthetic or
  containerized scenario, recording table scroll frame time and dropped frames
- A locale and scaling overflow test pass for `zh-TW` and `en-US` at 100/125/150%
- A process-action test suite driven against fixture processes, covering each
  action and each refusal
