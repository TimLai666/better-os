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

## Notes

### What was found (read only, owner's machine, 2026-11-21)

- Every package is 0.2.16 and the person has logged in again several times
  since. The installed `touchpad-adapter@betteros.org` is the repository's,
  it is active, and it owns `org.betteros.TouchpadAdapter1`.
- `better-touchpad-gestures.service` was dead on 5 of 7 logins since
  2026-11-16: it starts about 30 s into the session, the adapter is not on the
  bus yet, it logged "the GNOME Shell adapter extension is not on the session
  bus" and exited 0, and `Restart=on-failure` never started it again.
  `systemctl --user show` reads `ActiveState=inactive Result=success
  ExecMainCode=1 ExecMainStatus=0`, which Better Manager's health rule read as
  "finished on purpose", and the Touchpad page said "connected" because it only
  looked at the adapter's name. So nothing listened for gestures.
- On the two logins that won the race the service suppressed GNOME's own
  swipe trackers for nothing. GNOME Shell 46.0's `swipeTracker.js` (read from
  `/usr/lib/gnome-shell/libshell-14.so`'s gresource) has
  `const GESTURE_FINGER_COUNT = 3;` and ignores any other count, while
  `GNOME_46_GESTURES` said three *and* four. `gestures.json` still carries
  `conflicts` on the four four-finger swipes from that model.
- The extension built the pinch signal as `(uuddt)` with six values; GJS
  dropped the device node, so every pinch read as coming from an unknown
  device.
- SIGTERM killed the service without giving GNOME its gestures back, and the
  extension kept the suppression after the service was gone.
- `shell-adapter@betteros.org` (package `better-shell-adapter`) is installed
  and not in `enabled-extensions`. It is a companion of `better-manager`, and
  both the install steps and the startup pass read only the component's own
  dpkg file list, which has no extension. On this machine `better-manager`
  also moved to 0.2.16 by an external upgrade (`host.externally_upgraded`
  in the state record), so only the startup pass could have turned it on.

### What is fixed (branch of this ticket)

- The service waits for the adapter, starts when it appears, and when it
  goes away drops the pipeline and waits again. It never exits on purpose:
  with safe mode on, nothing configured, or the integration turned off by
  itself it stays resident and idle, owning its name. SIGTERM and SIGINT give
  GNOME its gestures back and then end the process by that signal
  (`ExecMainCode=2`, read as stopped). Better Manager no longer counts an
  exited unit as healthy; the startup pass and Check again start it.
- It watches `gestures.json` and the safe mode marker and takes every change
  at once: a gesture turned off in a window stops, a new one works, GNOME's
  gestures are taken or given back for what now collides. It writes back only
  verification results, conflict annotations and the switch the automatic
  disable turns off, onto the file as it is now.
- It owns `org.betteros.TouchpadGestures1`. The extension takes a suppression
  only from that name's owner, accepted only when its process runs
  `/usr/bin/better-touchpad-gestured`, and restores GNOME's trackers the
  moment the name loses its owner (ADR 0035, proposed, which applies ADR
  0018's relay rule). The service accepts gesture signals only from the
  adapter's current unique name.
- On attach the service adopts what the adapter reports, so the suppression
  the 0.2.16 pair left behind is lifted after the upgrade without a logout.
- GNOME's gestures are suppressed only for a gesture that collides under the
  corrected model, is on, and verified, and only after a gesture has arrived
  from the adapter. The window never suppresses. With the Mac-style preset
  nothing collides, so GNOME's three-finger gestures stay. A restore writes
  `enabled = true`, never a captured value.
- The pinch signal is `(uuddts)`; a test checks every emitted signal against
  the XML.
- One function corrects stale conflict annotations, used by the window and
  the service.
- The Touchpad page and the `touchpad.gesture_bridge` check tell apart
  listening, safe mode, nothing switched on, the service not running (with
  "Check again" as the way to start it), and the adapter missing. Every check
  detail in the Diagnostics table is shown in Traditional Chinese when the
  window is.
- The journal has one line per gesture begun and ended, per action and its
  outcome, per suppress and restore with the reason, while waiting for the
  adapter, for each configuration change, and at exit with whether GNOME has
  its gestures.
- A companion package's extension is turned on with its component, by the
  startup pass, Verify, Install, Update, Restore and AddCompanions. GNOME's
  switch that turns off all extensions is a warning, never a failed check,
  for every component (`engineering/specs/security-and-rollback.md`).

### What stays a limit: GNOME does not let go reliably

GNOME Shell 46's `workspaceAnimation.js` turns its three-finger workspace
tracker back on after every animated workspace switch and when the overview
hides, and the overview's workspace strip (`workspacesView.js`) and the
application grid (`appDisplay.js`) have three-finger trackers the adapter
never turns off. A Better OS gesture on three fingers in a direction GNOME
uses can therefore fire beside GNOME's. Holding the trackers off against
GNOME's own writes would be a decision in GJS, which no ADR grants. ADR 0035
records it, the Touchpad page warns when such a gesture is on, and the
Mac-style preset (four fingers) is not affected.

### What is left: one observation with a hand on the touchpad

Nobody has made a gesture against the fixed build, and the thresholds
(`swipe_travel`, `EventScale::swipe_pad_pixels` = 1000, activation 0.6) have
never been tuned against a hand. The owner also has to accept or reject ADR
0035. Once the fixed build is installed:

1. Log out and back in, so GNOME Shell loads the new extension. Open Better
   Manager once (its startup pass turns `shell-adapter@betteros.org` on;
   that extension then loads at the next login).
2. `systemctl --user status better-touchpad-gestures.service` reads
   `active (running)`.
3. Run `journalctl --user -b -f -u better-touchpad-gestures.service`. Expect
   "gestures are configured", "waiting for the GNOME Shell adapter extension
   …", then "the GNOME Shell adapter appeared after N s of waiting" (or "… is
   on the session bus"), then "gestures verified: PartiallySupported".
4. Swipe up with four fingers. Expect "the first gesture arrived from the
   adapter", "gesture began: swipe with 4 fingers on /dev/input/eventN, the
   global profile", "gesture ended: …", and
   "performed overview.show for overview (the global profile): done".
   - No "gesture began" line: no event reaches the service (the extension or
     Clutter side).
   - "began" and "ended" with no "performed" line: the recognizer did not
     accept the swipe, and the thresholds above are what to tune.
5. Four fingers left and right switch workspaces; thumb and three fingers
   pinching in opens Better Launcher; three fingers still do GNOME's own
   overview and workspace switching, and no "built-in gestures suppressed"
   line appears.
6. Turn gestures off in Better Manager's Touchpad page: the journal says "the
   gesture configuration changed" and "no gesture is switched on", and a
   four-finger swipe does nothing. Turn them on again: it works at once.
7. `systemctl --user restart better-touchpad-gestures.service` logs
   "exit: stopped by signal 15; GNOME has its own gestures" and starts again.

The ticket stays in `doing/` until that observation is made.
