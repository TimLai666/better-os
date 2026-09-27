# Ticket 73 — Each touchpad performs its own gesture profile

**Epic:** Better Touchpad (Issue #3) · **User Story:** a person with a laptop
touchpad and an external one gets each pad's own gestures · **Branch:**
`ticket-73` · **Blocked by:** none · **Status:** ready

## What it delivers

Gesture profiles are per device, but `org.betteros.TouchpadAdapter1` reports no
device identity, so both pads perform the selected pad's profile. The owner
decided on 2026-09-27 to widen the adapter interface so each gesture event
names the device that produced it. That changes the GJS exception's bounds, so
ADR 0015 records the widening first, as `AGENTS.md` requires.

## Acceptance criteria

- [ ] ADR 0015 states exactly what the extension may now report about a device
      (an identity the shell already exposes, nothing it computes or stores),
      and `crates/touchpad-session/tests/extension.rs` asserts the new bound.
- [ ] Every swipe and pinch event carries the device identity when the shell
      provides one, and an explicit "unknown device" when it does not.
- [ ] `better-touchpad-gestured` recognizes each event against that device's
      profile, falls back to the global profile for an unknown or unprofiled
      device, and never mixes frames from two devices into one gesture.
- [ ] Tests drive the Rust fake of the adapter with two devices. The live
      extension is checked on a nested GNOME Shell if one can be started, and
      the ticket says which it was.
