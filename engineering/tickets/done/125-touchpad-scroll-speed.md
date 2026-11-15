# Ticket 125 — Touchpad scroll speed that really works on GNOME 46

**Blocked by:** none

## Why

The owner decided on 2026-11-15 that Better Touchpad's vertical and horizontal
scroll speed must work on GNOME 46 ("everything gets done"). GNOME 46 has no
key for either, and ticket 88's experiment (proposed ADR 0031) showed a GNOME
Shell extension never sees a client's scroll events, so the only route is
below libinput: grab the touchpad's evdev node, scale its scroll motion, and
re-emit it through a uinput device. ADR 0008 and ADR 0012 require a security
review before any Better OS process reads a touchpad, because a process that
can read the touchpad can read the keyboard.

## What to build

- The security review, `engineering/specs/touchpad-input-passthrough-security.md`:
  threat model, who holds which privilege, exactly which nodes may be opened,
  how no key event is ever read or forwarded, how a crash or a hang never
  leaves the touchpad grabbed, the bus and udev boundary, what an attacker
  gains, and the test that pins each claim. ADR 0030 records the decision.
- `touchpad-scroll`: the device rule (touchpads only, by capability bits,
  never anything that can report a key), the frame scaling (two-finger motion
  on ABS and MT touchpads, `REL_WHEEL` and `REL_WHEEL_HI_RES` with the
  remainder carried), the grab lifetime (held only while a factor other than 1
  is set, taken and released only with the pad untouched, released on drop and
  panic), and the evdev/uinput layer behind a device seam.
- `better-touchpad-scrolld`, one system service per touchpad, started by udev
  and by the postinst, dropping to `_better-touchpad-scroll` after opening its
  two nodes, watchdog-supervised, owning `org.betteros.TouchpadScroll1.eventN`
  on the system bus with a logind active-session check.
- The GNOME backend carries the vertical and horizontal scroll factors through
  it, live whenever a pass-through answers. Smooth scrolling stays
  unavailable, with the reason.
- Packaging in `better-touchpad`: unit, udev rule, bus policy, sysusers,
  tmpfiles, maintainer scripts that start it on install and remove every trace
  on uninstall; manifest and `verify-deb.sh` checks.
- Added latency per frame measured and recorded in
  `engineering/performance/touchpad-scroll-performance.md`, and the owner's
  one-command real-hardware check.

## Done when

- [x] The security review and ADR 0030 are written, and every claim names its
      test or check.
- [x] Scaling, the keyboard refusal, and grab release on panic and exit are
      unit-tested against fake devices; no test opens `/dev/input` or
      `/dev/uinput`.
- [x] The window's client is tested against a fake pass-through on a private
      bus.
- [x] The package installs, enables, and removes the service, and
      `verify-deb.sh` and the maintainer-script test check it.
- [x] The in-process cost per frame is recorded.
- [ ] The owner ran `packaging/tests/touchpad-scroll-hardware-check.sh` on the
      real touchpad and the kernel-hop cost is recorded.

## Notes

- An independent security and input review of the first commit found no
  keyboard path and ten things to fix before merge, all fixed with tests:
  the race between the untouched check and the grab, carried offsets in the
  clickpad button zone and scaled pinches, a seccomp filter on the uinput
  descriptor, `BindsTo=` the device with a failing exit on `ENODEV`, the
  original's integration and libinput device group carried to the clone, the
  per-session bus authorization, refusal-log rate limiting and unchanged-store
  skipping and a narrower bus policy, a postrm that stops again before
  `userdel` and reports its failure, and the factors stated as one pair for
  every touchpad.
- A second review asked for eight more, all fixed with tests: the 22.04 bus
  policy names each instance's destination, a sessionless caller is refused
  while the user has a remote login (and the same-uid limit is written down),
  the clone is created once and then every uinput setup call is forbidden
  (with wheel-only relative bits and no direct-input property before that),
  install from Better Manager starts the instances through a coldplug unit and
  records "restart or reconnect" for the window when it cannot, a resting
  finger is put back in the frame a finger lifts or a button goes down, the
  device-gone exit is 69 and never restarted, trackpoint pairing was checked
  in libinput 1.20.0 and 1.25.0 (no refusal needed), and the clone's udev
  properties require a matching real touchpad.
- The final review asked for the silent twin and the on-screen-keyboard
  residual path to be stated plainly, and for gestures on a held pad to keep
  that pad's profile; both done.
- Done except the owner's real-hardware run, which is the one open box. No
  test touched a real `/dev/input` node, `/dev/uinput`, or the real system
  bus; the window was looked at in a headless GNOME Shell 46 against a fake
  pass-through on a private bus.
