# 37 — Launcher performance harness

**Epic:** Better Launcher (Issue #2) acceptance closure
**User Story:** The launcher's manifest stops defining benchmarks nothing
runs: warm search update, warm overlay open, application-list update, and idle
overhead all have measured numbers or a written investigation.
**Blocked by:** 21
**Status:** done

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

## What was built

`cargo bench -p launcher-gui --bench launcher_suite` — one command, one summary
table, no benchmark-framework dependency. It writes its own synthetic XDG data
directory of 5,000 generated `.desktop` entries every run, so no figure depends
on what is installed on the machine.

Two of the five measurements run in process. Three need a running
`better-launcher`, so the binary learned to time its own startup: with
`BETTER_LAUNCHER_TRACE_STARTUP=1` it prints two parseable stderr lines,
`shell-ready` and `library-ready`. That follows `better-touchpad`, which already
publishes a startup figure the same way, and it means the open time is measured
from the shipped binary rather than from a benchmark that reimplements the
startup path. Nothing is printed unless the variable is set, so the headless
launch smoke still expects silence.

The manifest was rewritten to name what is actually measured, and the alignment
is now enforced rather than asserted: `launcher_gui::BENCHMARKS` is the single
list of `(name, workload, metric)`, the harness labels its rows from it, and
`launcher-gui/tests/manifest.rs` fails if `better-launcher.yaml` says anything
else. A fifth definition, `idle-memory`, was added because the harness reads both
CPU and resident memory over the idle window and one metric string cannot carry
two numbers.

## Results

Development host, 2026-08-31. Full methodology, hardware, and limits in
`docs/launcher-performance.md`.

| Benchmark | Measurement | Result | Target |
| --- | --- | --- | --- |
| warm-search-update | keystroke to updated result model, p95 | 0.989 ms | 50 ms — met |
| application-list-update | filesystem event to refreshed model, p95 | 151.8 ms | none set |
| warm-overlay-open | spawn to first renderable model, p95 | 206.2 ms | none set |
| idle-overhead | CPU over a 20-second idle window | 0.0000 % | none set |
| idle-memory | resident set after that window | 52,992 kB | none set |

No measurement missed a documented target, so no investigation is owed for a
miss. Three results need reading rather than quoting:

- **150 of the 152 ms of an application-list update is a deliberate wait.**
  `launcher-platform`'s `SETTLE` collapses a burst of filesystem events into one
  reload. The actual work — notice, re-read, rebuild the index, swap it under
  the query already typed — is 1.8 ms, and the harness prints that row
  separately so the two are never confused.
- **`warm-overlay-open` ends at a model, not at a frame.** Under `ZED_HEADLESS=1`
  there is no compositor, no surface, and therefore nothing to present. The
  figure covers fork, exec, dynamic linking, GPUI start, constructing the
  overlay, focusing the search row, and reading and indexing 5,000 entries.
  Compositor handoff, surface allocation, GPU warm-up, and present-to-photon are
  outside it and are not estimated. `main()` to a focused search row is 37.9 ms;
  the rest is the library arriving behind it.
- **Idle CPU is zero in both counters, and the counter is proven live.**
  `/proc/[pid]/schedstat` and `/proc/[pid]/stat` both report no time used over
  the window, and the harness prints how much the same schedstat counter had
  already accumulated during startup (76.5 ms) so the zero is evidence about the
  process rather than about the instrument.

## What is not done

- **Nothing enforces the regression budgets.** The manifest declares a maximum
  regression per benchmark; comparing against one needs a stored baseline and a
  CI job that runs the harness. Neither exists. This run is the first candidate
  baseline and nothing consumes it. Better Files has the identical gap.
- **Headless idle is not session idle.** With no compositor nothing asks the
  window to repaint, and a live session redraws on damage. The 0 % figure is the
  launcher's own idle cost, not a claim about a launcher on a running desktop.
- **No time-to-photon, for this or any component.** Measuring it needs a
  compositor harness that does not exist in this workspace.
- **One machine, warm cache, amd64.** No arm64 run, no cold-cache read, no
  measurement under memory pressure.

## Verification actually run

- `cargo fmt --all -- --check` — clean
- `cargo clippy -p launcher-core -p launcher-platform -p launcher-gui
  --all-targets -- -D warnings` — clean
- `cargo test -p launcher-core -p launcher-platform -p launcher-gui` — 130
  tests, 0 failures, including the 8 existing manifest-validation tests through
  `better-core` and the 2 new ones asserting the manifest and the harness
  describe the same five benchmarks
- `cargo bench -p launcher-gui --bench launcher_suite` — every row above, and 6
  `ZED_HEADLESS=1` launches of `better-launcher` in the course of it, each of
  which reached a focused search row and a fully indexed 5,000-entry library
