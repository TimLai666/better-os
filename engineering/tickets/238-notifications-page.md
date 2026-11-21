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
- The lock screen's two settings (`design/login-screen-spec.md` section 4.2,
  L4 (a), given exact copy by ticket 260): 「在鎖定畫面上顯示通知」 / "Show
  notifications on the lock screen", on by default, and under it
  「顯示內容」 / "Show their content", on by default and off while the first
  is off (the Better OS session only; under GNOME the per-application "Show
  content on the lock screen" rows are the control). Take the words from
  `login_screen_core::copy` (`show_notifications_on_lock_screen`,
  `show_their_content`) rather than a second copy; the lock screen reads the
  same two values as `login_screen_core::lock_content::LockScreenNotifications`.
- Window attention's cards (ticket 106) moved in as the last group;
  `--page window-attention` opens it.

## Done when

- [ ] Tests for both stores, an application that has sent nothing yet, the
      service absent in the session, GNOME's per-application schema created
      for a new application.
- [ ] In a disposable Zorin OS 18 VM, in both GNOME and the Better OS session (ticket 201's image), the page matches the spec and its mockups in both appearances and languages; screenshots in `/home/tim/Developer/better-os-evidence/238/`.
- [ ] fmt, clippy on stable, and the changed crates' tests pass; the license inventory is regenerated if `Cargo.lock` moved.
