# Ticket 240 — Mouse in Better Manager

**Spec:** `design/system-settings-spec.md` section 13 (Mouse); ADR 0042 (proposed)
**Blocked by:** 231, 206, and the owner's decision M1

## Why

Mouse settings are COSMIC Settings' in the session and GNOME Settings' under
GNOME; Better Manager has a Touchpad page and nothing for a mouse.

## What to build

- Spec section first, then Bluetooth and devices › Mouse: primary button,
  pointer speed, acceleration, scroll speed, natural scrolling;
  `com.system76.CosmicComp` `input_default` in the session,
  `org.gnome.desktop.peripherals.mouse` under GNOME; no mouse connected is a
  sentence, not a hidden page.

## Done when

- [ ] Tests for both stores and the no-mouse state.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/240/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
