# 20 — launcher-core: index, matching, deterministic ranking, benchmarks

**Epic:** Better Launcher (Issue #2)
**User Story:** Typing in the launcher produces the same ranked applications
every time, fast enough that the list keeps up with the keystrokes, and that
claim is measured rather than asserted.
**Blocked by:** 18-app-catalog
**Status:** todo

## Goal

Build the launcher's search brain as a GPUI-free crate that can be benchmarked
without starting a window. Issue #2 requires exactly that isolation: "The search
engine must be isolated in `launcher-core` and benchmarkable without starting
the GUI."

## What it delivers

- A searchable index built over the shared catalog from ticket 18. No second
  desktop-entry scanner and no invented package database.
- Matching inputs: application display name, generic name, localized names,
  `.desktop` keywords, and executable name.
- Deterministic ranking: exact and prefix matches rank above fuzzy matches;
  the application name ranks above secondary metadata; equal scores break by a
  stable rule so repeated runs produce identical order.
- Browse and search models as separate outputs of one state driven by the
  current query, so the GUI (ticket 21) switches between them without changing
  windows or modes.
- Usage signals recorded through an abstract storage interface. The interface
  exists and is documented and removable; whether usage frequency adjusts
  ranking by default is not decided here.
- No network requests, no AI dependency, no typed-query history persisted by
  default, and no indexing of running-process command-line arguments.
- Benchmarks over a 5,000-record synthetic index: cold index construction, warm
  index load, query latency, ranking throughput, and list update after an
  application is installed or removed.

## Out of scope

- The GPUI overlay, activation, and metadata watching (ticket 21).
- File, settings, action, clipboard, calculation, and command search sources.
  The result model may stay extensible; these sources are not implemented.
- Gesture integration of any kind.
- Any catalog discovery or normalization work, which belongs to ticket 18.

## Deferred decisions

Issue #2 requires an ADR or a focused follow-up issue rather than a silent
choice for: whether usage frequency affects ranking by default, the exact
application-library grouping and category presentation, and whether file,
settings, action, or clipboard search belongs in later releases. Keep the
usage-signal interface abstract so none of these is settled by the storage
shape.

## Acceptance criteria

- [ ] The index is built from the shared catalog; `launcher-core` contains no
      desktop-entry parser of its own.
- [ ] Matching covers display name, generic name, localized names, keywords, and
      executable name, each with its own test.
- [ ] Exact and prefix matches rank above fuzzy matches, and name matches rank
      above secondary metadata.
- [ ] The same query against the same index returns identical order across runs,
      proven by a determinism test.
- [ ] Browse and search models are produced from one query-driven state, so an
      emptied query returns the browse model without a separate mode.
- [ ] Usage signals go through an abstract storage interface that is documented
      and removable, and no ranking depends on it by default.
- [ ] `launcher-core` has no GPUI dependency and its benchmarks run without a
      display backend.
- [ ] No code path performs a network request.
- [ ] Warm search update p95 stays below 50 ms for a 5,000-record synthetic
      index, or the miss is accompanied by a written investigation. The tested
      hardware, dataset, and procedure are recorded either way.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench -p launcher-core`: cold index construction, warm index load,
  query latency p50/p95, ranking throughput, and post-install list update,
  results recorded with hardware and dataset
- A test asserting the crate's dependency graph contains no GPUI crate
