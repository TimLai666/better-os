# Ticket 238 — Notifications in Better Manager

**Spec:** `design/system-settings-spec.md` section 12; ADR 0042 (proposed)
**Blocked by:** 231, 215, 216, the owner's decision M4, and the owner's decision M1

## Why

COSMIC Settings 1.11 has no notifications page (its desktop entry names a
subcommand the program does not define), and in the session the notifications
are Better OS's own service (tickets 215, 216), so only Better Manager can
offer their settings there.

## What to build

- System › Notifications of section 12: Do not disturb, the lock screen, the
  per-application list with banners, sound, and lock-screen content; GNOME's
  `org.gnome.desktop.notifications` keys through the dconf writer under GNOME,
  the notification service's own bus in the session (whatever names ticket 215
  fixed), the service as the only owner of its settings.
- Window attention's cards (ticket 106) moved in as the last group;
  `--page window-attention` opens it.

## Done when

- [ ] Tests for both stores, an application that has sent nothing yet, the
      service absent in the session, GNOME's per-application schema created
      for a new application.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/238/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
