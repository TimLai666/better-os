# 19 — Better App Chooser: MIME ranking, Open Once, Always Use, Choose Executable

**Epic:** Shared application catalog and Better App Chooser (Issue #4)
**User Story:** Choosing what opens a file shows the applications that actually
declare support for it, and making that choice permanent changes exactly one
MIME association and leaves a record that can undo it.
**Blocked by:** 18-app-catalog

## Goal

Build the reusable chooser on top of the shared catalog: the compatibility and
ranking logic in a core crate, the surface in a GPUI crate that Better Files
(ticket 35) and any later Better OS surface can embed or open as a window.

## What it delivers

- `app-chooser-core`: MIME compatibility and ranking for one selected file or
  URI, producing three sections — recommended applications that explicitly
  declare the MIME type, other compatible or previously used applications, and
  all applications behind an explicit expansion.
- `AppSelection` result model: `desktop_id`, optional `action_id`,
  `association_mode` of once or default, and `executable_path` populated only in
  executable-selection mode. The result is an application identity, never a path
  inside a virtual Applications location.
- Open Once: launch the selected file with the selected application through the
  ticket 18 launch path, changing nothing persistent.
- Always Use: write the per-user `mimeapps.list` association for the intended
  MIME type only. Unrelated associations are read and written back byte-identical
  or left untouched. Every persistent change writes a rollback record that can
  restore the previous state.
- Choose Executable as a separate, separately named mode: returns a real path
  only when one resolves safely, warns when the application is Flatpak, Snap,
  D-Bus-activated, wrapper-based, or has complex launch arguments, and falls
  back to a normal file picker over `/usr/bin`, `/usr/local/bin`, and user-local
  executable paths.
- `app-chooser-gui`: the reusable GPUI surface built on `better-ui` and
  `gpui-component`, sharing the primitives Issue #4 names — application tile,
  application list row, source badge, MIME compatibility badge, selected state,
  search field, empty state, loading and refresh state, launch failure state.
- The UI explains, in the user's own words, when a chosen application does not
  declare support for the selected file type.
- No custom MIME database. Standard per-user association mechanisms only.

## Out of scope

- Catalog discovery, normalization, and launching (ticket 18).
- The Applications virtual location and the Open With entry point inside Better
  Files (ticket 35).
- An XDG portal backend, and becoming the desktop-wide default chooser.
- Multi-file and multi-MIME selection; the first implementation covers one
  selected file per Issue #4's scope line.
- Replacing third-party toolkit file choosers.

## Deferred decisions

Issue #4 requires an ADR or a focused follow-up issue instead of a silent
choice for: the exact portal integration mechanism, whether Better App Chooser
becomes the desktop-wide default chooser, and whether package-source badges are
shown by default. Keep the selection result and section model free of any
assumption that settles these.

## Acceptance criteria

- [x] The chooser shows MIME-compatible applications first, then other
      compatible or previously used, then all applications behind an explicit
      expansion.
- [x] Open Once launches the selected file with the selected application and
      writes nothing persistent.
- [x] Always Use updates only the intended MIME association; a test asserts that
      every unrelated association in the file is unchanged afterwards.
- [x] Every Always Use change produces a rollback record that restores the exact
      previous association.
- [x] Removing or clearing Better OS state does not destroy MIME associations it
      did not create or change.
- [x] Choose Executable returns a real path only when one resolves safely, and
      warns for Flatpak, Snap, D-Bus-activated, wrapper, and complex-argument
      applications instead of inventing one.
- [x] A desktop-application selection is never silently converted into an
      executable path.
- [x] The GUI uses GPUI, `gpui-component`, and shared `better-ui` primitives.
- [x] MIME compatibility filtering runs off the render thread and is benchmarked
      against the 5,000-record synthetic catalog.
- [x] The selection result model carries no virtual filesystem path.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench -p app-chooser-core` for MIME compatibility filtering latency
- A `mimeapps.list` fixture test: apply Always Use, diff the whole file, assert
  a single-line change, then roll back and assert byte equality with the
  original
- `ZED_HEADLESS=1` launch smoke of the chooser surface

## Result

All verification commands ran locally and passed: `cargo fmt --all -- --check`,
`cargo check --workspace`, `cargo test --workspace`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo bench -p app-chooser-core`, and an
8-second `ZED_HEADLESS=1` launch of `app-chooser-gui` in both Open With and
Choose Executable mode.

Two criteria are met by construction rather than by an automated assertion, and
are recorded here rather than claimed as tested:

- Ranking runs on a background thread because `AppChooser` reads the catalog
  through `cx.background_spawn`. Nothing asserts that a frame never ranks; the
  core crate has no GPUI dependency, which is what makes the claim checkable at
  all.
- Open Once launching is exercised by the headless smoke and by
  `app-catalog-platform`'s own launch tests. The chooser's own tests assert that
  an Open Once selection writes nothing, not that a process started.

## Notes

Carried over from the retired delivery status record.

Ticket 19 landed the Better App Chooser: `app-chooser-core` (MIME section
ranking, the `AppSelection` result model, the `mimeapps.list` editor and its
rollback records, and executable-mode refusals) and `app-chooser-gui` (the
reusable GPUI surface plus a standalone window for testing it). Neither crate
parses a `.desktop` file; both read ticket 18's records.

The three invariants this ticket adds. First, an Always Use changes exactly one
line of the user's `mimeapps.list`. The file is parsed into lines that are kept
verbatim — comments, unknown groups, repeated keys, CRLF endings, a missing
final newline — so everything except the one association is written back byte
for byte. Second, the rollback record is written and flushed before the file is
opened for writing, so a crash between the two leaves a record of a change that
never happened rather than a change with no record. Third, the executable mode
refuses by default: a Flatpak, Snap, AppImage, wrapper, D-Bus-activated, or
own-arguments entry is told why no path is offered instead of being handed the
program its `Exec` line happens to name.

MIME type relationships come from the installed `shared-mime-info` data files —
`aliases`, `subclasses`, and `globs2` — read only. There is no Better OS MIME
database, and an absent `shared-mime-info` degrades to "no known relationships"
rather than to a guessed hierarchy.

95 tests were added: 64 core unit tests, 8 fixture-file tests over six real
`mimeapps.list` shapes (hand edited with comments, groups in an unexpected order
with `[Default Applications]` opened twice, CRLF, no final newline, empty, and
comment-bearing), 13 GUI locale and layout-policy tests, plus the ranking,
selection, and executable-refusal coverage inside the core suites. The fixture
tests assert a single-line diff, that every unrelated association survives, and
byte equality after a rollback for every fixture.

Local `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo test
--workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo bench -p app-chooser-core` all passed. Ranking 5,000 synthetic records
takes 1.46 ms per pass, and it runs through `cx.background_spawn` rather than on
the render thread. An 8-second `ZED_HEADLESS=1` launch of `app-chooser-gui`
stayed alive in both Open With and Choose Executable mode.

Three things are known and not done. A `[Removed Associations]` line covering
the same type and application is reported as a warning rather than edited, so a
pre-existing removal can still override a new default; fixing it silently would
mean touching a second line the user wrote. Usage history is currently inferred
from the associations file, because no shared favorites or history model exists
yet — Issue #4 defers how that is shared with Better Launcher. And the chooser
handles one selected file; multi-file selection is out of scope for this ticket.
