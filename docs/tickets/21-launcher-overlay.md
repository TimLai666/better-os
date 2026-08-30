# 21 — Better Launcher overlay: unified screen, activation, gesture adapter seam

**Epic:** Better Launcher (Issue #2)
**User Story:** One overlay opens with the search row focused, shows the whole
application library below it, filters in place as the user types, and returns to
the library when the query is cleared — all without opening a second window.
**Blocked by:** 20-launcher-core
**Status:** todo

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

## Acceptance criteria

- [ ] Opening the launcher shows one unified screen with the search row above
      the application library.
- [ ] The search row receives focus immediately on open.
- [ ] Typing filters and ranks applications in the same screen.
- [ ] Clearing the query restores the application library without closing or
      changing windows.
- [ ] Hidden and desktop-incompatible entries are excluded, inherited from the
      shared catalog rather than re-filtered locally.
- [ ] Keyboard navigation and launching work with no pointer at all.
- [ ] A configurable global keyboard shortcut opens the launcher, and a desktop
      entry does too.
- [ ] Application metadata changes update the visible library through event
      notification, with no idle polling.
- [ ] The GUI uses GPUI, `gpui-component`, and shared `better-ui` primitives.
- [ ] The launcher performs no network request.
- [ ] No privileged input capture is introduced.
- [ ] A typed platform-adapter interface exists and an ADR compares the four
      viable five-finger gesture approaches with their trade-offs.
- [ ] Unsupported touchpads degrade to keyboard and desktop-entry activation
      with no error state.
- [ ] A valid Better OS manifest and a written rollback plan exist for the
      component.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `launcher-gui`
- A manifest validation run through `better-core` against the new manifest
- Overlay-open latency measured warm, and idle CPU and memory recorded
- A test proving no network syscall path exists in the launcher binaries'
  dependency surface
