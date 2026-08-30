# 21 — Better Launcher overlay: unified screen, activation, gesture adapter seam

**Epic:** Better Launcher (Issue #2)
**User Story:** One overlay opens with the search row focused, shows the whole
application library below it, filters in place as the user types, and returns to
the library when the query is cleared — all without opening a second window.
**Blocked by:** 20-launcher-core
**Status:** done

## Goal

Deliver the launcher the user actually sees, plus the platform seam that lets a
gesture open it later without putting launcher logic anywhere near a compositor
extension.

## What it delivers

- `launcher-gui`: one full-screen or near-full-screen GPUI overlay built on
  `better-ui` and `gpui-component`. Search row fixed near the top, application
  area below, one window throughout.
- Query-driven states from ticket 20 rendered in place: empty query shows the
  application library; typing filters and ranks in the same screen; clearing the
  query restores the library without closing or reopening the window.
- Keyboard focus lands in the search row on open. Arrow keys, Enter, mouse, and
  touchpad all select and launch. Escape closes.
- States the issue names: clear keyboard focus indication, selected item,
  application icon and name, empty-result state, loading and index-refresh
  state, and launch failure feedback.
- `launcher-platform`: application launching through the shared catalog path,
  metadata change watching, desktop/session capability detection, and the
  global activation abstraction.
- Activation paths: a desktop entry, and a configurable global keyboard
  shortcut as the required fallback. Both work on hardware with no gesture
  support.
- A gesture adapter interface — typed, narrow, and unimplemented — plus an ADR
  comparing the four integration options Issue #2 names: a minimal GNOME Shell
  adapter, compositor-supported gesture integration, a Rust/libinput session
  service, and any stable desktop portal or standard protocol.
- Better Manager component manifest for `better-launcher`, declaring supported
  releases and architectures, desktop-session requirements, the optional gesture
  adapter, files and settings touched, keyboard shortcut integration, health
  checks, rollback behavior, and benchmark definitions.
- Installing the launcher does not remove the GNOME application overview; the
  original entry path stays recoverable until health checks pass and the user
  explicitly chooses the replacement.

## Out of scope

- A production five-finger gesture adapter. The ADR compares; it does not build.
  The gesture backend itself is chosen and built under ticket 30.
- Index, matching, and ranking (ticket 20).
- Replacing GNOME Shell or changing Mutter.
- Privileged raw-input capture and global input-device grabs.
- Continuous gesture-driven open animation.

## Deferred decisions

Issue #2 requires an ADR or a focused follow-up issue rather than a silent
choice for: the production GNOME/Wayland gesture integration mechanism, whether
a minimal GJS platform adapter is acceptable, the exact keyboard shortcut,
full-screen versus bounded overlay dimensions, and animation style and
gesture-progress mapping. The gesture ADR this ticket delivers covers the first
two; the rest stay open and must not be hard-coded.

How each was kept open in the delivered code:

- **Gesture mechanism, and whether a GJS adapter is acceptable** — ADR 0008
  compares all four paths and adopts none. The adapter boundary is typed and
  the only implementation is a mock.
- **Exact keyboard shortcut** — `launcher_platform::shortcut` names the four
  GNOME settings that carry it and leaves `binding` as `None`. There is a test
  asserting the shortcut is described but not yet applicable, so a default
  cannot be added without deleting the test that says it must not be.
- **Full-screen versus bounded** — one constant, `OVERLAY_COVERAGE`, applied to
  the display's own size. No compositor-specific layer-shell integration was
  built, so moving to true full screen later is a constant, not a rewrite.
- **Animation and gesture-progress mapping** — the recognizer emits
  `GestureOutcome::Progress` and nothing consumes it. There is no animation.
- **Category grouping** — the browse model already carries sections; the
  overlay renders the flat deterministic order and ignores them, so choosing a
  grouping later is a presentation change with the data already present.
- **Whether usage frequency affects ranking** — `RankingOptions::default()`
  leaves usage weighting off and the overlay never turns it on, so no usage
  store is created and nothing is recorded.

One decision this ticket took that the issue did not name: the overlay is
transient, so the process ends when it closes and a forwarded toggle quits it
rather than hiding a window. Whether the launcher should stay resident is the
same open question as its dimensions and is recorded here rather than settled.

## Acceptance criteria

- [x] Opening the launcher shows one unified screen with the search row above
      the application library.
- [x] The search row receives focus immediately on open.
- [x] Typing filters and ranks applications in the same screen.
- [x] Clearing the query restores the application library without closing or
      changing windows.
- [x] Hidden and desktop-incompatible entries are excluded, inherited from the
      shared catalog rather than re-filtered locally.
- [x] Keyboard navigation and launching work with no pointer at all. Asserted
      at the model level and wired through a capture-phase key handler so the
      arrow keys reach the grid before the search row consumes them. Not
      verified by hand in a live desktop session.
- [~] A configurable global keyboard shortcut opens the launcher, and a desktop
      entry does too. The desktop entry exists as packaging data and works when
      installed by hand; `packaging/build-deb.sh` does not yet build a
      `better-launcher` package. The shortcut is described down to the exact
      GNOME settings and is deliberately not applied: the key combination is a
      deferred decision and writing the setting is Better Defaults' job.
- [x] Application metadata changes update the visible library through event
      notification, with no idle polling. The watcher reports the backend it
      actually got rather than claiming to be event-driven.
- [x] The GUI uses GPUI, `gpui-component`, and shared `better-ui` primitives.
- [~] The launcher performs no network request. No launcher crate reaches an
      HTTP client, a TLS stack, or a resolver, and that is a test. The binary
      still links one, because `gpui-component-assets` depends on
      `zed-reqwest`; that is true of every Better OS desktop binary and is not
      something this ticket introduced or can remove.
- [x] No privileged input capture is introduced. Nothing opens an input device,
      nothing grabs input, and no process runs as root.
- [x] A typed platform-adapter interface exists and an ADR compares the four
      viable five-finger gesture approaches with their trade-offs. See
      [ADR 0008](../decisions/0008-launcher-gesture-integration.md).
- [x] Unsupported touchpads degrade to keyboard and desktop-entry activation
      with no error state.
- [x] A valid Better OS manifest and a written rollback plan exist for the
      component.

## Rollback plan

Better Launcher is an enhancement, never a replacement. Installing it does not
touch, hide, or remove the GNOME application overview, so the original entry
path is available at every moment, including while the launcher is installed
and healthy. There is nothing to restore in order to get it back.

Rolling back the component therefore has three steps, and each is reversible on
its own:

1. Remove the package, which removes `/usr/bin/better-launcher` and the desktop
   entry. Any running overlay is a transient process; it ends on its own.
2. Drop this component's own custom-keybinding path from
   `org.gnome.settings-daemon.plugins.media-keys custom-keybindings` and clear
   the three keys under it. Better Defaults wrote them and owns undoing them,
   from the snapshot it captured before the first change rather than from a
   guessed factory default. Nothing else in that list is touched.
3. Nothing else. The launcher writes no state of its own: no usage history, no
   query history, no cache, no configuration file. Usage-weighted ranking
   exists in `launcher-core` and is switched off, so there is no store to
   delete.

The failure case that matters is a health check failing after install. The
component's own health checks are the desktop entry being present and readable,
the index building over the real catalog, and the activation name being
claimable. If any fails, the overview is still the working entry path and step
1 is sufficient.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `launcher-gui`
- A manifest validation run through `better-core` against the new manifest
- Overlay-open latency measured warm, and idle CPU and memory recorded — **not
  done**. The benchmarks are defined in the manifest and no harness runs them
  yet.
- A test proving no network syscall path exists in the launcher binaries'
  dependency surface — done for every launcher crate, and honestly qualified
  for the toolkit. See `crates/launcher-gui/tests/dependencies.rs`.
