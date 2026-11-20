# Ticket 219 — Better Touchpad's settings and gesture page work in the session

**Spec:** better-os-session.md S5, S6 (Better Touchpad, scroll pass-through, and gesture rows)
**Blocked by:** 206

## Why

In the session the touchpad's settings are `cosmic-comp`'s `input_touchpad`
and per-device `input_devices` (state, acceleration profile and speed, click
method, disable while typing, left-handed, middle-button emulation, scroll
method, natural scroll, scroll factor, tap, tap button map, drag, drag lock:
`cosmic-comp-config/src/input.rs`, ADR 0038), not GNOME's nine keys. And until
ticket 223, the released compositor keeps every three- and four-finger swipe
and sends pinches to the focused window, so the gesture page must say what
the person actually has.

## What to build

- **`CosmicBackend`** in `touchpad-platform` beside `GnomeBackend`, chosen by
  `better_core::desktop`, writing `com.system76.CosmicComp` through ticket
  206's adapter; per-device profiles map to `input_devices` natively; every
  control's apply and read-back through the production write-then-read path
  with only the write and read replaced, as the GNOME backend's tests do.
- **Scroll speed**: one factor for both axes written as
  `scroll_config.scroll_factor`; the scroll pass-through's evdev grab only when
  the person sets different horizontal and vertical factors (ADR 0030 and
  0031 amended by a note).
- **The gesture page and service in the released compositor**: the service
  in the session with no gesture protocol stays resident and idle, its
  `Status()` saying "released compositor"; the page shows
  四指左右滑動切換工作區（跟著手指移動）。其他手勢需要 Better OS 的合成器。 /
  "Four fingers left and right switch workspaces, following your fingers.
  Other gestures need Better OS's compositor." and the preset's rows that do
  not reach Better OS marked unavailable with that reason; nothing suppressed,
  because nothing GNOME's is there.

## Done when

- [ ] Unit tests for every control's mapping, per-device profiles, the scroll
      factor choice, and the page model in both languages.
- [ ] In the VM session: each control changes `cosmic-comp`'s behaviour
      (read back from its configuration and, for tap and natural scroll, by a
      replayed libinput recording); the page's released-compositor state
      screenshotted in `/home/tim/Developer/better-os-evidence/219/`.
- [ ] fmt, clippy on stable, and the crates' tests pass.
