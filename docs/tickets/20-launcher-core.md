# 20 — launcher-core: index, matching, deterministic ranking, benchmarks

**Epic:** Better Launcher (Issue #2)
**User Story:** Typing in the launcher produces the same ranked applications
every time, fast enough that the list keeps up with the keystrokes, and that
claim is measured rather than asserted.
**Blocked by:** 18-app-catalog
**Status:** done

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

- [x] The index is built from the shared catalog; `launcher-core` contains no
      desktop-entry parser of its own.
- [x] Matching covers display name, generic name, localized names, keywords, and
      executable name, each with its own test.
- [x] Exact and prefix matches rank above fuzzy matches, and name matches rank
      above secondary metadata.
- [x] The same query against the same index returns identical order across runs,
      proven by a determinism test.
- [x] Browse and search models are produced from one query-driven state, so an
      emptied query returns the browse model without a separate mode.
- [x] Usage signals go through an abstract storage interface that is documented
      and removable, and no ranking depends on it by default.
- [x] `launcher-core` has no GPUI dependency and its benchmarks run without a
      display backend.
- [x] No code path performs a network request.
- [x] Warm search update p95 stays below 50 ms for a 5,000-record synthetic
      index, or the miss is accompanied by a written investigation. The tested
      hardware, dataset, and procedure are recorded either way.

## Design as built

The crate is five modules over one dependency, `app-catalog-core`.

- `text` folds a string once: Unicode case folding, Latin-1 and Latin
  Extended-A diacritic folding, word-start positions, and a 64-bit character
  mask used to reject an application before any comparison runs. CJK passes
  through unchanged, and every ideograph counts as a word start because CJK
  text carries no spaces.
- `matcher` decides what a comparison is worth. Two ordered enums and one
  integer: `MatchKind` (exact, prefix, word prefix, substring, fuzzy),
  `FieldKind` (name, alternate name, generic name, keyword, executable), and a
  detail score. They compose into one `u32` where the match kind dominates the
  field and the field dominates the detail, so no detail score can promote a
  fuzzy match past a substring one.
- The fuzzy scorer is written here rather than taken from a dependency. The
  workspace lockfile carried no fuzzy matcher, and `strsim` is only present
  transitively under `clap`. It is a linear-per-query-character dynamic program
  with bonuses for word starts and adjacency and a linear gap penalty, guarded
  by a greedy subsequence feasibility scan.
- `index` builds the searchable index and ranks. Literal matching runs over
  every field first; fuzzy matching runs only for applications nothing literal
  matched, which is sound because any literal match outranks every possible
  fuzzy one, and is what holds per-keystroke cost down.
- `model` holds the browse model, the search model, and `LauncherState`, which
  is a query and nothing else. `LauncherState::view` returns the library for a
  blank query and ranked results otherwise, so there is no mode to desynchronize.
- `usage` is the abstract storage interface, with a no-op and an in-memory
  implementation. It has no place to put a typed query, which is how "no
  persisted query history" stays true structurally rather than by discipline.

Two decisions worth stating because they could have gone the other way:

- **Match kind outranks field.** Issue #2 requires both "exact and prefix above
  fuzzy" and "name above secondary metadata", and the two can disagree. Kind
  decides first, so typing an application's own keyword exactly does not bury
  it under every name that happens to contain those letters in order. Field
  precedence settles ties between equally strong matches.
- **Usage weighting is a bounded adjustment to the detail term only.** It is
  clamped inside the bucket the match earned, so a frequently launched
  application can be reordered against its equals and can never overtake a
  stronger match. It is off by default, and the deferred question of whether it
  should be on remains open.

The browse model is built once with the index rather than per query, so
clearing the search row is a borrow rather than a rebuild of several thousand
entries.

## Verification actually run

Crate-scoped, on branch `ticket-20`. Workspace-wide gates run downstream after
merge, as the ticket queue expects.

| command | result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo check -p launcher-core --all-targets` | pass |
| `cargo test -p launcher-core` | pass, 53 tests (48 unit, 4 dependency-graph, 1 doc) |
| `cargo clippy -p launcher-core --all-targets -- -D warnings` | pass |
| `cargo bench -p launcher-core` | pass, numbers below |

The GPUI assertion is `crates/launcher-core/tests/dependencies.rs`. It walks
`[dependencies]` and `[build-dependencies]` through the workspace manifests and
hands each external package to `Cargo.lock`, because a lockfile records
development dependencies for workspace members and walking it alone would
charge this crate for its neighbour's benchmark helpers. The same walk asserts
no network client is reachable, and that the closure stays small enough to read.

## Benchmark results

Hardware: AMD Ryzen AI 9 HX 370 (24 threads), 32 GB RAM, Zorin OS 18.1,
rustc 1.97.1, release profile.

Dataset: 5,000 synthetic records built in memory from generated desktop-entry
text — 40 realistic base names with numeric suffixes, `zh_TW` and `de` name
translations, a localized generic name, localized keywords, a category, and a
`TryExec`. Records are built in memory because discovery cost belongs to
`app-catalog-core`'s benchmarks and would hide these numbers.

Procedure: the query script types out seven queries one character at a time and
then backspaces each back down — a multi-word query, a single word, an acronym,
a CJK query, a long specific query, a query only a fuzzy match can answer, and
a query that matches nothing. 2,300 keystrokes across 20 rounds; each keystroke
is one `LauncherState::view` call plus a walk of the results it returned.

| measurement | result |
| --- | --- |
| record preparation (catalog cost, not this crate) | 13.877 ms |
| cold index construction | 17.833 ms |
| warm index load | 20.718 ms |
| browse model, empty query | under 0.001 ms |
| query latency p50 | 0.066 ms |
| **query latency p95** | **1.005 ms** (target: below 50 ms) |
| query latency p99 | 1.870 ms |
| query latency worst sample | 3.350 ms |
| ranking throughput | 4,376 queries/s, 21.9 M record-comparisons/s |
| query latency p95, usage weighting on | 1.275 ms |
| list update after install | 20.788 ms |
| list update after removal | 22.276 ms |

The 50 ms target is met with a factor of about 50 in hand. The cost is
dominated by index construction, not by querying, which is the shape the design
aimed for: a keystroke compares folded slices and a package installation is
what pays for folding them.

## Deferred decisions still open after this ticket

- Whether usage frequency adjusts ranking by default. The interface, the
  bounded adjustment, and the switch all exist; the default stays off.
- The application-library grouping and category presentation. Browse currently
  groups by the freedesktop registered main categories with an `Other` section,
  exposed as index lists a surface may ignore entirely.
- Whether file, settings, action, or clipboard search belongs in a later
  release. Nothing here forecloses it, and nothing here implements it.
