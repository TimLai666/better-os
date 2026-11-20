# Ticket 235 — Displays in Better Manager, and the session's brightness

**Spec:** `design/system-settings-spec.md` section 9, F6; ADR 0042 (proposed)
**Blocked by:** 231, 200, 206, the owner's decision M4, and the owner's decision M1

## Why

Scale, resolution, refresh rate, and arrangement are reached in the session
only through COSMIC Settings; night light is not reachable at all there (F4).
Better Manager's brightness card is never drawn in the session, because it
reads `gsd-power`, which only GNOME runs (F6).

## What to build

- `display-core` and `display-platform` (section 3.1): the session's
  `zwlr_output_manager_v1` client with COSMIC's `zcosmic_output_manager_v1`
  extension, Mutter's `org.gnome.Mutter.DisplayConfig` under GNOME
  (`ApplyMonitorsConfig` method 1 for the trial, 2 to keep), modes, scales and
  the recommended ones, arrangement rules (no gap, no overlap), mirroring,
  the 15-second revert of section 5.3 with a test clock.
- The page of section 9: arrangement with Identify, the display selector,
  every card and state, fractional scaling under GNOME through
  `org.gnome.mutter experimental-features`, night light through
  `org.gnome.settings-daemon.plugins.color` under GNOME and the session's
  "cannot yet" state.
- The brightness card's session backend: `com.system76.CosmicSettingsDaemon`
  `DisplayBrightness` and `MaxDisplayBrightness`, chosen by `CurrentDesktop`
  in `hardware-platform`.
- First read whether `cosmic-comp` 1.11 has a primary output and VRR through
  the protocol; draw those cards only where it does.
- Kit: the arrangement canvas.

## Done when

- [ ] `display-core` tests: every apply path, the revert when the dialog is
      ignored, Escape, the window closing, the compositor refusing a mode, the
      last display left on, a display unplugged during the countdown.
- [ ] Tests against a fake Mutter `DisplayConfig` on a private bus and a fake
      output-management compositor.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/235/`. With a second virtual display: extend, duplicate, only one, and a refused mode reverting.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
