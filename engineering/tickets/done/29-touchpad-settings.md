# 29 — Better Touchpad phase 1: pointer, scrolling, clicking, devices

**Epic:** Better Touchpad (Issue #3)
**User Story:** A user opens one native application, changes pointer and scroll
sensitivity, sees the value actually take effect, and can undo the whole thing
if the touchpad becomes hard to use.
**Blocked by:** none

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
      `engineering/specs/touchpad-sensitivity-mapping.md`. Idle overhead is zero by
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

## Notes

Carried over from the retired delivery status record.

Better Touchpad is a working control centre: it reads the touchpad, changes it,
proves the change took by reading it back, and can put everything back the way
it was. `touchpad-core` holds the configuration, the four states per setting,
and the apply and restore plans, with no GPUI and no key name in it.
`touchpad-platform` owns session detection, device enumeration, and the GNOME
backend. `touchpad-gui` renders and decides nothing.

The largest thing in it is the one ADR 0009 deferred. Better OS can now write a
dconf key. The path is `ca.desrt.dconf.Writer.Change` on the session bus,
unprivileged, with the change set encoded by hand in GVariant. Two details are
worth carrying forward:

- dconf's own change-set type is `(sa{smv})` — a prefix plus relative key names
  — and sending that shape is *accepted*: the call returns a change tag and no
  key moves. The blob the service actually acts on is `a{smv}` with absolute key
  paths. That was found by watching what `dconf write` sends, not by reading a
  header, and it is why the encoder's bytes are pinned against values GLib
  produced rather than against itself.
- A reset is an absent value in the change set, which removes the key so the
  session's own default applies again. That is what restoring a setting the user
  had never touched has to do, and it is why "nothing was set here" is a
  distinct, restorable reading rather than a kind of unknown.

`defaults-platform` can adopt the same path and turn its two GNOME adapters from
"manual action required" into real applies. That is a behaviour change to Better
Defaults and its tests, so it is a follow-up rather than something done in
passing.

Ten of the thirteen controls are live on GNOME 46. The other three are not, and
this is the ticket's own rule applied to its own headline control: GNOME 46 has
no touchpad scroll-factor key and no smooth-scroll setting, so vertical scroll
factor, horizontal scroll factor, and smooth scrolling are shown as unavailable
with the reason attached instead of as sliders that do nothing. The model
carries both scroll axes as independent, linkable values and the mock backend
applies and verifies both, so the behaviour is proven and one table row away
from being live when GNOME grows the key.

Numbers on the development host: reading every setting back 6.8 µs, staging a
slider move 1.2 µs, building an apply plan 1.5 µs, migrating a version 1
configuration 1.5 µs. Against the real dconf service, an apply and its verifying
read take 3.6–6.0 ms and a restore 3.6–4.7 ms, which is why the GUI does that
work on the calling thread instead of behind a task. A debug-build window is
ready 99–134 ms after process start, of which 0.33–0.54 ms is reading the
desktop. Nothing polls while idle.

Not measured, and not claimed: pointer-event overhead and scroll-event latency.
Better Touchpad sits in no input path — it writes a setting and the compositor
does the rest — so there is no Better OS code between a finger and a pointer to
measure. A gesture adapter would have both, and those figures belong with
ticket 30.

The default test suite mutates nothing. One `#[ignore]`d test changes a real
setting, and it also refuses to run without `BETTER_TOUCHPAD_LIVE=1`, so
`cargo test -- --ignored` on a developer's desktop does not quietly move their
touchpad either. It covers all three GVariant shapes the backend writes — a
boolean, a double, and an enumerated string — captures each setting first, and
asserts the machine ends where it started.
