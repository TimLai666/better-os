# Ticket 250 — The device name in System › About

**Spec:** `design/system-settings-spec.md` section 13 (About); ADR 0042 (proposed)
**Blocked by:** 231, the owner's decision M6, and the owner's decision M1

## Why

The name other devices see is hostnamed's; Better Manager shows the device
but cannot rename it.

## What to build

- Spec section first, then a device name card in System › About:
  hostnamed's `SetPrettyHostname` and `SetStaticHostname` with the
  administrator note, the name checked as hostnamed checks it.

## Done when

- [ ] Tests against a fake hostnamed for every polkit outcome and an invalid
      name.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/250/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
