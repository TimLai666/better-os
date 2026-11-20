# Ticket 242 — Personalization in Better Manager: appearance and wallpaper

**Spec:** `design/system-settings-spec.md` section 13 (Personalization); ADR 0042 (proposed)
**Blocked by:** 231, 206, and the owner's decision M1

## Why

The desktop's dark or light mode, accent, and wallpaper are COSMIC Settings'
in the session and GNOME Settings' under GNOME; Better Manager's Appearance
card sets only its own windows.

## What to build

- Spec section first, then Personalization: mode, accent where the desktop
  has one (not GNOME 46), wallpaper (picture, colour, fit, per display);
  `CosmicTheme.Mode`, the theme builders, `CosmicBackground` in the session;
  `color-scheme` and the background keys under GNOME.
- The two Settings cards reworded as section 13 says.

## Done when

- [ ] Tests for both stores, a wallpaper file that is gone, a second
      display.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/242/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
