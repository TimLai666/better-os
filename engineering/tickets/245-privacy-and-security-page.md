# Ticket 245 — Privacy and security in Better Manager

**Spec:** `design/system-settings-spec.md` section 13 (Privacy and security); ADR 0042 (proposed)
**Blocked by:** 231, 206, and the owner's decision M1

## Why

In the session no program offers screen lock, location, camera, or
microphone settings (F1, F2).

## What to build

- First read, in the VM, what locks the session's screen and after what
  delay, and whether geoclue has an agent there; record both in Notes.
- Spec section first, then Privacy and security: screen lock, location,
  camera and microphone per application (the portal's permission store, shared),
  recent files; GNOME's keys under GNOME.

## Done when

- [ ] Tests against a fake permission store; each card's session state
      matches what the VM showed.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/245/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
