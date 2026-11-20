# Ticket 249 — Every default app, and apps that start at sign-in

**Spec:** `design/system-settings-spec.md` section 13 (Default and startup apps); ADR 0042 (proposed)
**Blocked by:** 231, and the owner's decision M1

## Why

Better Manager's Default apps page makes Better OS components defaults; the
browser, mail, and the rest are COSMIC Settings' or GNOME Settings'.

## What to build

- Spec section first, then Apps › Default apps gains every default
  (`mimeapps.list` through `app-chooser-core`) and Apps › Startup apps
  (`~/.config/autostart/`), in both desktops; record whether any COSMIC X11
  compatibility setting is needed in the session.

## Done when

- [ ] Tests for `mimeapps.list` edits (one place, `app-chooser-core`) and
      autostart entries.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/249/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
