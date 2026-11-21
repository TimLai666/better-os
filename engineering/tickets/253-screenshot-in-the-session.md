# Ticket 253 — Print Screen and the Screenshot portal's dialog work in the Better OS session

**Spec:** better-os-session.md S4 (the "Screenshot" and "Media keys" rows of the process table)
**Blocked by:** 203

## Why

Ticket 201 found that COSMIC's default `system_actions` binds the screenshot
key to `cosmic-screenshot`, which is not one of the 14 packages of the pinned
closure, so Print Screen does nothing in the session. Under GNOME the shell
takes screenshots. `xdg-desktop-portal-cosmic` serves the Screenshot portal
(its non-interactive path), but a person pressing Print Screen gets nothing.

## What to build

- **Decide the provider with the owner first** (a choice C2 bounds): (a) add
  `cosmic-screenshot` to the closure, which changes the compiled-in pin list
  (ticket 204) and needs the owner's answer because C2 names 14 packages;
  (b) a Better OS screenshot program in Rust that asks the Screenshot portal
  (interactive) and saves to `~/Pictures/Screenshots` as GNOME does, with a
  notification through ticket 215's service; (c) bind the key to
  `xdg-desktop-portal`'s Screenshot call through a one-line helper with no
  dialog. Recommendation: (b), it keeps the closure as decided and gives the
  owner's familiar save place.
- The session's `system_actions` default (ticket 203's package data) names
  the chosen program; ticket 203's test that every action names a shipped
  program covers it.

## Done when

- [ ] The owner's answer is recorded in this ticket.
- [ ] In ticket 201's VM (`packaging/tests/session-vm/`), Print Screen in the
      session saves an image of the screen where the person expects it, and an
      application asking the Screenshot portal interactively gets a dialog
      and an image; screenshots in `/home/tim/Developer/better-os-evidence/253/`.
- [ ] Tests for the program's rules (save path, file name, refusal when the
      portal says no).
