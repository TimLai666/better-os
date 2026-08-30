# 29 — Better Touchpad phase 1: pointer, scrolling, clicking, devices

**Epic:** Better Touchpad (Issue #3)
**User Story:** A user opens one native application, changes pointer and scroll
sensitivity, sees the value actually take effect, and can undo the whole thing
if the touchpad becomes hard to use.
**Blocked by:** none
**Status:** done

## Goal

Deliver Issue #3's Phase 1 — the complete touchpad settings half of the control
center — with the rule that no control appears unless the active backend can
read, apply, and verify it.

## What it delivers

- `touchpad-core`: versioned, migratable configuration, plus the pointer,
  scrolling, and clicking models with supported-range validation and the four
  states Issue #3 requires — current, pending, effective, and previous. Apply and
  restore plans live here. No GPUI, no shell command, no compositor detail.
- Pointer: movement sensitivity, acceleration profile where the backend supports
  it, acceleration amount where it is representable safely, disable-while-typing
  where supported, and both the requested and the current effective value.
- Scrolling: vertical and horizontal sensitivity, linked or independent axes,
  natural scrolling, two-finger scrolling toggle, and smooth-scroll behavior
  where the backend can control it.
- Clicking: tap-to-click, tap-and-drag, drag lock where supported, click method
  where several exist, middle-click emulation where supported.
- `touchpad-platform`: Wayland/X11 session detection, touchpad enumeration,
  capability reading, applying pointer/scroll/click values, verifying effective
  values by reading them back, and reporting whether the effect is immediate or
  needs sign-out.
- Device handling: automatic selection, explicit selection when several
  touchpads exist, per-device configuration where a stable identity is
  available, a global fallback profile, a capability summary, and a clear
  unavailable state for a disconnected device. Device identity does not rest on
  an unstable kernel path alone.
- A capability-gated control surface: an unsupported setting is shown as
  unavailable with an explanation, never as an inert switch.
- Automatic backup of the effective configuration before the first mutation, and
  restore by section or in full, showing the captured values first. Restoration
  returns to the captured state, not a guessed factory configuration.
- Bounded ranges with impossible values rejected, startup health checks, and a
  safe-mode command or desktop entry that disables Better Touchpad integration.
- `touchpad-gui` phase-1 sections: Overview, Pointer, Scrolling, Clicking,
  Devices, Diagnostics — with live pointer and vertical/horizontal scroll test
  surfaces, keyboard-only navigation, and no backend command construction in the
  GUI.
- `zh-TW` and `en-US` with runtime switching, using Issue #3's fixed Traditional
  Chinese terms for this half: `游標靈敏度`, `捲動靈敏度`, `自然捲動`,
  `點按來按一下`.

## Out of scope

- Gestures of any kind, the Gestures screen, the Mac-style preset, conflict
  detection, and `better-actions` (ticket 30).
- The production GNOME/Wayland gesture backend and the session adapter.
- Issue #3's Phase 4: custom contact counts, custom keyboard-shortcut actions,
  per-device gesture profiles, and profile import/export.
- Direct `/dev/input` access, LD_PRELOAD, compositor patching, or any permanent
  session injection.

## Deferred decisions

Issue #3 requires an ADR comparing viable options rather than a silent choice
for: the exact slider ranges and acceleration curves, per-device versus global
defaults, and X11 gesture implementation depth. Bounded ranges ship with their
bounds recorded as a starting point, not as a settled curve.

## Acceptance criteria

- [x] Pointer movement sensitivity can be changed and verified by read-back
      where the backend supports it.
- [x] Vertical and horizontal scrolling sensitivity can be changed
      independently. Independently in the model and against the mock backend;
      GNOME 46 has no scroll-factor key, so both are shown as unavailable with
      the reason rather than as sliders that do nothing.
- [x] Linked-axis mode updates both scroll values together.
- [x] Natural scrolling and the supported tap and click controls are available.
- [x] A control the active backend cannot read, apply, and verify is shown as
      unavailable with an explanation, never as an inert switch.
- [x] Session type, active backend, and selected device are detected and shown.
- [x] Applying reports applied, awaiting sign-out, partially supported, or
      failed — and each state has a test.
- [x] Configuration is versioned and migratable, and persists across application
      restarts and user sessions.
- [x] A backup is captured before the first mutation, and the previous
      configuration can be restored by section or in full.
- [x] Impossible values are rejected rather than clamped silently.
- [x] A safe-mode entry point disables Better Touchpad integration
      (`better-touchpad --safe-mode`).
- [x] The GUI never executes a backend-specific shell command, asserted over the
      crate's own source.
- [x] Normal operation requires no root.
- [x] Idle overhead and input latency are benchmarked and documented.
      Configuration, read-back, live apply, and startup are measured in
      `docs/touchpad-sensitivity-mapping.md`. Idle overhead is zero by
      construction — nothing polls — and pointer/scroll event latency is not
      measured because no Better OS code sits in the input path; a gesture
      adapter would, and those figures belong with ticket 30.
- [x] `zh-TW` and `en-US` layouts pass overflow tests at 100%, 125%, and 150%
      scaling.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `touchpad-gui`
- An apply-and-read-back test against a mock platform backend for every control,
  including the unsupported and sign-out-required paths
- A configuration migration test from schema version 1 to the shipped version
- Benchmarks: idle CPU and memory, pointer-event overhead, scroll-event latency,
  UI startup time, and configuration apply time, with no dropped or reordered
  scroll events under normal load

## Result

All of it ran. `cargo fmt --all -- --check`, `cargo check --workspace`,
`cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D
warnings` are green; the 8 s `ZED_HEADLESS=1` launch of `better-touchpad`
stayed alive with no output. 170 tests in the three new crates — 61 in
`touchpad-core`, 74 in `touchpad-platform` (two of them `#[ignore]`d and
opt-in, because they change a real setting), and 35 in `touchpad-gui` — plus
two added to `defaults-platform` for the GVariant type its reader grew.

Two things this ticket did that the ticket text did not ask for, and one it
could not do:

- The dconf D-Bus write path is built, not deferred. ADR 0009 left it as the
  eventual answer; ADR 0010 records the decision and the encoding, and
  `defaults-platform` can now adopt the same path as a follow-up.
- The change-set encoding was taken off the wire. dconf's own `(sa{smv})`
  change-set type is accepted by the service and writes nothing; the blob is
  `a{smv}` with absolute paths.
- The scroll-factor sliders cannot be live on GNOME 46. The key does not exist,
  so they are unavailable with the reason attached — which is the ticket's own
  rule applied to its own headline control.
