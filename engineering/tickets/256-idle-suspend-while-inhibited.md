# Ticket 256 — Idle suspend in the session respects a sleep inhibitor instead of asking for a password, and logging out never takes logind down

**Spec:** better-os-session.md S4 (the "Blanking, idle, suspend on idle" row); flow 2 ("let it idle to blank; suspend and resume")
**Blocked by:** 203

## Why

Ticket 201's hour of use found two faults around idle suspend in the
session, on Ubuntu 24.04 (systemd 255.4-1ubuntu8.19) and Zorin OS 18.1:

- **With a sleep inhibitor held** (`systemd-inhibit --what=sleep`, as Better
  Awake and any media player hold one), 30 minutes after the last input a
  process of the person's session asked logind to suspend while ignoring
  inhibitors: polkit logged `FAILED to authenticate … for action
  org.freedesktop.login1.suspend-ignore-inhibit` and `…systemd1.manage-units`
  for a session bus client of the person's (Zorin, 05:05:49; the session had
  locked itself first, screenshot `zorin/92-idle-suspend-asks-polkit.png`).
  Under GNOME an inhibitor simply keeps the machine awake; here the session
  asks for an administrator's password behind the lock screen. Which process
  asked is not identified (`cosmic-idle` running the `Suspend` action's
  `systemctl suspend` is the likely one).
- **Logging out while such a request was pending crashed logind** (Ubuntu,
  04:06:40): `systemd-logind: Assertion '!q->denied_action' failed at
  src/shared/bus-polkit.c:254, function async_polkit_read_reply(). Aborting.`
  logind restarted 90 s later, GDM's greeter did not come back, and only a
  reset brought the login screen back. That run had also been through a VM
  S3 suspend whose resume left a kernel worker hung
  (`ubuntu/serial-after-vm-suspend-hung-task.log`), so the crash is not yet
  shown to need only the session; the core is
  `ubuntu/out-hour/_usr_lib_systemd_systemd-logind.0.crash`.

## What to build

- **Reproduce** in the VM (`packaging/tests/session-vm/`) without any VM
  suspend: hold a sleep inhibitor, let the session idle past the suspend
  time, log out while the polkit request is pending; then without the
  inhibitor. Name the process that calls (watch the system bus with
  `busctl monitor org.freedesktop.login1`).
- **The fix in the session's configuration**, not in COSMIC's code: the
  session's `cosmic-idle` defaults (ticket 203's package data, through
  `XDG_DATA_DIRS`; a directory there replaces COSMIC's
  `com.system76.CosmicIdle/v1/` defaults whole, so it holds every key of it,
  as the session's shortcut directory does) so that idle suspend honours inhibitors as GNOME does, or
  idle suspend handed to logind's own `IdleAction` path; and the idle times
  carried from GNOME's power settings (`org.gnome.settings-daemon.plugins.power`
  `sleep-inactive-ac-timeout` and `-battery-`), which ticket 208's table does
  not yet list: add the row there.
- If the logind crash reproduces from the session alone, report it to Ubuntu
  with the core and say in the spec what the session does to avoid the
  trigger.

## Done when

- [ ] In the VM, with an inhibitor held, the session never asks for a
      password to suspend and stays awake; without one, it suspends at the
      time GNOME's setting says.
- [ ] Logging out while idle and inhibited leaves logind running and GDM's
      greeter on screen, ten times in a row; the journal shows no assertion.
- [ ] Evidence in `/home/tim/Developer/better-os-evidence/256/`.
