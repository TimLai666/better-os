# 23 — Monitor Apps and Processes views, safe actions, real Overview

**Epic:** Better Monitor (Issue #16)
**User Story:** A user can see which application is responsible for a slowdown,
understand why its processes were grouped that way, and stop it safely without
the GUI ever running as root.
**Blocked by:** 22-monitor-metric-contracts
**Status:** done

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

- [x] The GPUI application displays real CPU, memory, PSI, process, storage, and
      network data from ticket 22's collectors.
- [x] Apps and Processes are separate views.
- [x] Every app group can state the evidence that produced it.
- [x] Unrelated processes with matching executable names are not merged, proven
      by a test.
- [x] Unknown, unsupported, stale, permission-denied, and zero metrics are
      visually and semantically distinct in the UI.
- [x] Process and app tables are virtualized and stay responsive under the
      documented large-process benchmark.
- [x] Terminate, force stop, pause/resume, and priority change work without the
      GUI running as root.
- [~] Elevated actions go through a narrow reviewed boundary, and cancelling
      before approval mutates nothing. **Partly.** Cancelling before approval
      mutating nothing is met: a destructive action becomes a pending
      confirmation and nothing is sent until it is confirmed. The elevated
      boundary itself is *not* built. Cross-user and privilege-requiring
      actions are refused before anything is attempted, with the owner named
      and the reason shown, and no privileged helper is reached for. Building
      that boundary is follow-up work, recorded below.
- [x] The GUI performs no `/proc` scraping, no shell pipeline, and holds no
      privileged handle.
- [x] The Overview distinguishes high utilization without contention, real
      pressure, throttling, collector failure, and unknown state.
- [x] Collector health and observation coverage are visible in the UI.
- [x] Unsupported pages explain themselves and never show fabricated zeros.
- [x] `zh-TW` and `en-US` layouts pass overflow tests at 100%, 125%, and 150%
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

## What was delivered

- `monitor-core::action` — typed process-control contracts: an intent rather
  than a signal, a closed `SignalKind`, refusals as data, and one shared
  unprivileged policy that both the real controller and the test fake apply.
- `monitor-views` — a new crate with no GPUI dependency, holding the grouping
  engine, the process table model (sort, filter, tree), the Apps model with
  coverage-aware aggregates, the Overview model with its verdicts, and the
  formatting layer where a reading becomes a cell.
- `monitor-actions-linux` — a new crate, the only place in Better Monitor that
  calls `kill`, `setpriority`, `getpriority`, or `geteuid`.
- `monitor-collectors-linux` — per-process storage throughput from
  `/proc/[pid]/io`, so an application's disk activity is attributable.
- `monitor-gui` — the real window: eleven pages, two virtualized tables, live
  sampling on a background task, process detail with confirmed actions, both
  locales with runtime switching, dark-first theming.

## Follow-ups this ticket did not close

- **The elevated-action boundary.** Cross-user and privileged process actions
  are refused with an explanation. A narrow polkit-reviewed helper for them is
  not built, and building one is a decision about privileged surface area that
  belongs with `manager-daemon`'s existing boundary rather than beside it.
- **The grouping-precedence ADR.** The order is configurable and defaults to
  Issue #16's list, with confidence recorded per group. The final precedence
  and the confidence thresholds still need the ADR the specification asks for.
- **Frame time and dropped frames are not measured.** The model-side benchmark
  is published; a rendered-frame harness needs a display server and does not
  exist in this workspace.
- **Application grouping at 10,000 processes costs about one frame** and runs
  on the interface thread when a round is adopted. Moving it to the sampling
  thread is the fix if that scenario becomes real.
- **No desktop-entry names or icons.** Groups are labelled from the cgroup's
  application id. Wiring `app-catalog-core` in would give a translated name and
  an icon; the engine has a place for it and does not depend on it.
- **Two deliverables in the list above were not built:** "open the executable
  or working location" and "copy diagnostic details". Both need a launch or
  clipboard path this ticket did not want to invent beside `app-catalog-platform`'s
  existing launch boundary. The detail panel shows the diagnostic fields; it
  cannot yet put them anywhere.
- **Keyboard-only operation is not verified.** The tables carry
  `gpui-component`'s own row and cell keyboard navigation and every control is
  a focusable widget, but no test or manual pass proves the whole window is
  reachable without a pointer, so it is not claimed.
- **Charts.** There are none yet, so "readable without relying on colour alone"
  and "data-table alternatives to charts" are met trivially: every page is
  already a table or a labelled figure. That stops being true the moment a
  chart is added.
