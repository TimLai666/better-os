# Ticket 239 — Keyboard in Better Manager: layouts, typing, shortcuts

**Spec:** `design/system-settings-spec.md` section 13 (Keyboard); ADR 0042 (proposed)
**Blocked by:** 231, 206, 221, and the owner's decision M1

## Why

Keyboard layouts and shortcuts are COSMIC Settings' in the session and GNOME
Settings' under GNOME. In the session they interact with Fcitx 5 (C4) and
Better Input (tickets 180–191, 221).

## What to build

- Write the page's section into the spec first (every state, both
  languages, mockups), then build Bluetooth and devices › Keyboard: layouts,
  the switch key, compose and caps lock, repeat, num lock, shortcuts with
  conflict checks, and today's backlight and colour cards; the session's
  `xkb_config`, `keyboard_config`, and `com.system76.CosmicSettings.Shortcuts`
  kept in step with Fcitx 5's layouts; GNOME's input sources and keybindings.
- Input methods link to Better Input's page; nothing here edits them.

## Done when

- [ ] The spec section exists before code; tests for both stores, a shortcut
      taken by another binding, a layout Fcitx 5 does not know.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/239/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
