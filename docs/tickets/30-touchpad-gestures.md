# 30 — Better Touchpad phase 2: Mac-style gestures, typed actions, test mode

**Epic:** Better Touchpad (Issue #3)
**User Story:** A user turns on the Mac-style preset, sees which GNOME gestures
it would conflict with before anything changes, and can watch the recognizer
work in a test mode that performs no system action.
**Blocked by:** 29-touchpad-settings
**Status:** done

## Goal

Deliver Issue #3's Phase 2: the gesture model, the shipped preset, the typed
action catalog, conflict detection, and a mock session adapter — plus the ADR
that chooses the production gesture backend so Phase 3 can be built on a
recorded decision instead of whichever path was easiest.

## What it delivers

- `touchpad-gestures`: the gesture model with, per gesture, an enabled state,
  gesture shape (swipe, pinch, spread, hold, tap, or another supported
  primitive), required contact count, direction where relevant, assigned action,
  activation threshold, cancellation threshold, cooldown, animation-progress
  capability, active backend, conflict state, and last verification result.
- The first-party **Mac-style gestures** preset with Issue #3's mapping: thumb
  plus three fingers pinch inward opens Better Launcher; thumb plus three
  fingers spread outward shows the desktop; four fingers up opens the workspace
  overview; four fingers down shows the current application's windows; four
  fingers left and right switch workspaces. The two thumb-plus-three gestures
  use four contact points. Two-finger horizontal swipe, pinch, and rotate map to
  application back/forward, zoom, and rotate where the application and backend
  support them. Direction is configurable.
- Five-finger mappings remain available as custom gestures and are not the
  default for Launcher or Show Desktop.
- `better-actions`: the typed desktop action catalog shared with Launcher and
  Manager, covering at minimum Better Launcher open/close, Show Desktop,
  Overview, current-application windows, next/previous workspace, next/previous
  application where the desktop can support it safely, media play/pause, volume
  up/down/mute, custom keyboard shortcut, and disabled/no action. Arbitrary
  shell command execution is not in the catalog and cannot be expressed.
- Conflict detection against existing GNOME gestures, run before a preset is
  applied. A conflicting built-in gesture is disabled, remapped, or retained only
  after preview and explicit confirmation.
- `touchpad-session`: the adapter boundary that invokes typed actions and
  receives gesture progress, with capability reporting — and a mock session
  adapter as its only implementation in this ticket. It never accepts an
  arbitrary shell string from configuration.
- Test gestures mode: recognition progress is visualized and no system action is
  performed unless the user explicitly enables live testing.
- The Gestures screen in `touchpad-gui`: the preset presented as the recommended
  configuration, then each gesture as an editable row showing a compact direction
  diagram, contact count, assigned action, enabled state, conflict or
  compatibility status, and test and edit actions. Diagnostics gains recognized
  gesture events and conflict results, without exposing raw input data by
  default.
- An ADR choosing the production gesture backend, comparing the four options
  Issue #3 names: a minimal GNOME Shell adapter receiving compositor gesture
  progress, Mutter or compositor-supported gesture interfaces, a Rust/libinput
  user-session service, and compatibility with an existing gesture project after
  license, architecture, and security review.
- `zh-TW` and `en-US` for this half, using Issue #3's fixed terms: `手勢`,
  `顯示桌面`, `顯示所有 App`, `工作區總覽`.

## Out of scope

- Implementing the production gesture backend and the real session adapter.
  This ticket chooses it in an ADR; Phase 3 builds it.
- Continuous gesture-driven animation on a real compositor.
- Issue #3's Phase 4 advanced customization.
- Direct `/dev/input` access without a separate security review, root execution,
  and any permanent session injection.

## Deferred decisions

Issue #3 requires an ADR comparing viable options rather than a silent choice
for: whether the narrow GNOME adapter uses a minimal GJS extension, the exact
gesture thresholds and velocity curves, the support depth for two-finger
application navigation, zoom, and rotation, and the exact mapping for
next/previous application. The backend ADR this ticket delivers covers the
adapter question; the thresholds ship as recorded starting values.

## Acceptance criteria

- [x] The built-in Mac-style preset maps thumb plus three fingers inward to
      Better Launcher and outward to Show Desktop.
- [x] Four-finger up, down, left, and right map to the documented desktop
      actions.
- [x] Gesture contact count, direction, action, thresholds, cooldown, and
      enabled state are all configurable per gesture.
- [x] Five-finger mappings are supported as custom gestures and are not the
      default for Launcher or Show Desktop.
- [x] The typed action catalog contains every listed action and cannot express
      an arbitrary shell command.
- [x] Existing GNOME gesture conflicts are detected before a preset is applied,
      and a conflicting gesture is only replaced after preview and explicit
      confirmation.
- [x] Test mode visualizes recognition without triggering system actions by
      default.
- [x] Applying the preset captures the previous gesture configuration first and
      can restore it.
- [x] Unsupported hardware or sessions show explicit explanations.
- [x] A failing gesture adapter is disabled automatically and does not break
      pointer movement or two-finger scrolling.
- [x] An ADR compares all four production gesture backend options and records
      the choice with its rejected alternatives.
- [x] Keyboard alternatives remain available when gesture support is absent.
- [x] `zh-TW` and `en-US` layouts pass overflow tests at 100%, 125%, and 150%
      scaling.

## What actually shipped

Three new crates: `better-actions` (14 tests), `touchpad-session` (15), and
`touchpad-gestures` (74), plus the Gestures screen in `touchpad-gui` (66 tests
in that crate) and [ADR 0012](../decisions/0012-touchpad-gesture-backend.md).

Four things are worth carrying forward.

**No gesture backend ships, and the screen says so.** The ADR chooses the
minimal GNOME Shell adapter, conditional on a language-policy exception nobody
has granted yet. Until then `MockSessionAdapter` is the only adapter, it reports
`performs_system_actions: false`, the live-testing switch is disabled while that
is true, and every unsupported action carries its reason on the row.

**The launcher action has a real route already.**
`touchpad_session::LauncherActivationAdapter` sends `Activate` and
`ActivateAction("close")` through `launcher-platform`'s own `NameRegistry`,
which is the path a second launch and a dock already use. It reinvents nothing,
reports every other action unsupported, and is tested against a fake registry so
no test needs a bus.

**The two-finger rows have no support anywhere.** Application back, forward,
zoom, and rotate are in the preset because Issue #3's table puts them there, and
every adapter reports them unsupported, so they arrive in the plan preview as
unsupported rather than as bindings that quietly do nothing. How deep that
support goes is one of Issue #3's explicitly deferred decisions.

**The GNOME conflict model is a static model, not a probe.** GNOME's swipe
trackers are compiled into the shell and expose nothing to read, so
`GNOME_46_GESTURES` is a dated claim this repository makes: both trackers accept
three *and* four contacts, which is why the Mac-style preset collides with four
of them. Re-checking it is part of supporting a newer GNOME.

Measured on the development host: 104–126 ns to recognize one frame, and
1.1–2.1 µs for a whole four-finger swipe from begin to complete. Dropped and reordered frames
are counted rather than assumed — a reordered frame is dropped instead of
dragging a gesture backwards — and both are asserted against streams the
benchmark deliberately damages. All of it is over replayed synthetic frames,
because no backend in this build produces real ones.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `touchpad-gui` reaching the Gestures screen
- A recognizer test suite over replayed gesture event sequences: activation,
  cancellation before threshold, cooldown, and each preset gesture
- A conflict-detection test against a fixture GNOME gesture configuration
- A test asserting no configuration path can produce a shell string for the
  session adapter
- Gesture-recognition latency benchmarked, and dropped or reordered gesture
  frames counted
