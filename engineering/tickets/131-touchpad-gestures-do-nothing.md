# Ticket 131 — Touchpad gestures do nothing on the owner's machine

**Blocked by:** none

## Why

The owner reported on 2026-11-16 that none of the touchpad gestures work,
after installing 0.2.16 (better-touchpad, better-shell-adapter and
better-manager-daemon 0.2.16).

## What was seen (read only, 2026-11-16)

- `touchpad-adapter@betteros.org` is in `org.gnome.shell enabled-extensions`;
  `disable-user-extensions` is false.
- `better-touchpad-gestures.service` is enabled and active.
- Its log before 0.2.16, every start: "safe mode is on and the adapter could
  not be reached: the adapter extension is not on the session bus".
- Its log on 2026-11-16 20:11: "gestures verified: PartiallySupported" and
  "built-in gestures: Suppressed" — GNOME's own swipe gestures are switched
  off, and no gesture of ours has been logged as recognized or performed.
- The person had not logged out since installing, so the shell may still run
  the previous extension code.

## What to build

- Find the real cause on the owner's machine (read-only probes, with the
  owner's consent for anything that changes state): whether the shell loaded
  the new extension, whether events reach the adapter and the service,
  whether safe mode is still on, which profile is in force, and whether the
  recognizer receives frames.
- Fix it at its source. Whatever the cause, GNOME's built-in gestures must
  never be suppressed while ours are not working: if the pipeline cannot
  perform gestures, it gives GNOME's back and the Touchpad page says why.
- Add the missing logging so the next failure is diagnosable from the journal.

## Done when

- The owner's four-finger and three-finger gestures work, or GNOME's own
  work and the page explains what is missing.
- Tests for the "never suppress while not working" rule.
