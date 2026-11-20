# Ticket 248 — What happens when a drive or phone is plugged in

**Spec:** `design/system-settings-spec.md` section 13 (Removable media); ADR 0042 (proposed)
**Blocked by:** 231, and the owner's decision M1

## Why

GNOME's autorun choices are GNOME Settings'; in the session nothing runs an
autorun handler at all.

## What to build

- Decide in the spec whether Better Files offers autorun in the session; then
  System › Drives gains the choices (`org.gnome.desktop.media-handling` under
  GNOME, Better Files' setting in the session), with today's mount choices.

## Done when

- [ ] Tests for both stores.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/248/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
