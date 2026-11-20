# Ticket 244 — Accessibility in Better Manager

**Spec:** `design/system-settings-spec.md` section 13 (Accessibility); ADR 0042 (proposed)
**Blocked by:** 231, 206, and the owner's decision M1

## Why

Accessibility is COSMIC Settings' in the session and GNOME Settings' under
GNOME; the screen reader switch is shared (`org.a11y.Status`).

## What to build

- Spec section first, then Accessibility: screen reader, text size, high
  contrast, magnifier, colour filters, reduce motion, mono audio, keyboard aids
  where the desktop has them; the session's `CosmicComp` keys and the a11y
  manager protocol, GNOME's `org.gnome.desktop.a11y.*`.

## Done when

- [ ] Tests for both stores; Orca reads the page in the VM.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/244/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
