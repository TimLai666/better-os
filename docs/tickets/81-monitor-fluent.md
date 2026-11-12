# Ticket 81 — Better Monitor, rebuilt in Fluent and complete

**Blocked by:** 80 · **Status:** ready

## Why

The owner finds Better Monitor unusable. The 2026-11-12 audit found internal
keys such as `cpu.utilization.busy` and raw ratios on the processor page,
mislabelled rows, text that cannot be read on the applications table, tables
that fill half the window, English process states in the Chinese UI, jargon
on the overview, no charts anywhere, navigation that turns into unlabeled
icons below 800px, and no "follow the system" appearance. The owner's rule:
an app is complete before it is called done.

## What to build

Every page of `crates/monitor-gui` rebuilt on `better_ui::fluent` (ticket 80):
NavigationView with the 48px compact mode, settings cards, details tables,
info bars, the shared `words`, `theme::Appearance` (dark, light, system).

The app is complete when it does what Windows 11 Task Manager does, on Linux:

- Overview: one plain sentence on how the machine is doing, then CPU, memory,
  disk, network, GPU, and battery cards with a live chart each.
- Processes and applications: a sortable, filterable table that fills the
  window, with end task, end process tree, open file location, and details;
  states translated and explained.
- Performance: per-resource pages with history charts (CPU overall and per
  core, memory composition, each disk's activity and throughput, each network
  interface, GPU, temperatures and fan speed from hwmon).
- Startup apps and background services the user session runs, with enable
  and disable where it needs no root.
- History and events as they exist today, in plain words.
- Every number has a unit and a human label; no internal key is ever shown.
- zh-TW and en complete; nothing truncated in either at 960px.

## Acceptance

- [ ] Every finding N1–N11 of the audit is fixed, each shown by a screenshot.
- [ ] Every page in light and dark, English and zh-TW, at 1600 and 960 wide,
      screenshotted and looked at.
- [ ] Charts update live without blocking the UI thread.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
