# Ticket 246 — Printers in Better Manager

**Spec:** `design/system-settings-spec.md` section 13 (Printers); ADR 0042 (proposed)
**Blocked by:** 231, the owner's decision M6 (and M2, which may move it to wave 1), and the owner's decision M1

## Why

In the session no program can add or manage a printer: COSMIC Settings has
no printers page and GNOME Settings does not run there (F1, F2).

## What to build

- Spec section first, then Bluetooth and devices › Printers: printers found
  and added, the default, the queue, adding a driverless IPP Everywhere
  printer first, removing, a test page; CUPS over IPP for reading,
  cups-pk-helper for changes (ADR 0042).

## Done when

- [ ] Tests against a fake CUPS and cups-pk-helper.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/246/`. The VM adds an IPP Everywhere printer an `ippeveprinter` serves.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
