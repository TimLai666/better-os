# 37 — Launcher performance harness

**Epic:** Better Launcher (Issue #2) acceptance closure
**User Story:** The launcher's manifest stops defining benchmarks nothing
runs: warm search update, warm overlay open, application-list update, and idle
overhead all have measured numbers or a written investigation.
**Blocked by:** 21
**Status:** todo

## Goal

Issue #2's acceptance requires "search benchmarks meet the documented target
or include a written investigation". `launcher-core` measured query latency
(p95 1.0 ms), but the manifest's four launcher-level benchmarks have no
harness. Build it.

## What it delivers

- A harness (bench or bin) measuring end-to-end: warm search update through
  the GUI model layer (keystroke → updated result model, p95 vs the 50 ms
  target on 5,000 synthetic records), application-list update after an
  install/removal event through the watcher path (synthetic XDG dir), warm
  overlay open (process start to first renderable model in headless mode —
  document what "open" can mean without a compositor), and idle CPU/memory of
  the overlay process over a measured window.
- Numbers recorded in `docs/` with hardware context; misses get an
  investigation, not silence.
- The manifest's benchmark definitions aligned with what the harness
  actually measures.

## Verification

Workspace gates plus the harness run with recorded output.
