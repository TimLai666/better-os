# ADR 0030: Touchpad scroll speed through an evdev/uinput pass-through

## Status

Accepted on 2026-11-15 by the project owner's decision ("everything gets
done"; ticket 125). Supersedes the recommendation of ticket 88's proposed
ADR 0029, which found the route and advised not to build it before a security
review existed. That review is
[touchpad-input-passthrough-security.md](/engineering/specs/touchpad-input-passthrough-security.md).

## Context

GNOME 46 has no scroll-speed key, libinput leaves scroll scaling to the
compositor, and ADR 0029's experiment showed a GNOME Shell extension never
sees a scroll event aimed at a client. The only place left to change scroll
distance on Zorin 18 is below libinput: the events a touchpad produces.

ADR 0008 and ADR 0012 made a security review a precondition for any process
that reads a touchpad, because the permission to read one input node is the
permission to read a keyboard.

## Decision

- **One system service per touchpad**, `better-touchpad-scroll@eventN.service`
  running `better-touchpad-scrolld`, started by a udev rule for every touchpad
  and by the package's postinst for touchpads already present. Installing the
  package turns it on; removing it stops every instance and deletes its
  account and state. It is shipped in `better-touchpad`.
- **It holds the touchpad only while a factor other than 1 is set**: an
  exclusive `EVIOCGRAB`, and every event forwarded to a uinput clone with the
  same capabilities, two-finger finger motion (and any wheel) scaled. At 1 it
  holds nothing. The clone is created once, at start, and stays idle between
  holds, so the service can forbid itself every later uinput setup call.
- **Privilege:** root only to open the two nodes, inside a unit whose device
  cgroup allows exactly `/dev/input/eventN` and `/dev/uinput`; then
  `_better-touchpad-scroll` with no groups, no capabilities, and
  `no_new_privs`, and a seccomp filter of its own that lets the uinput
  descriptor declare no key outside the button block. No polkit action: the
  service checks the calling process's own logind session (active and local),
  because an unprivileged mechanism cannot ask polkit about other processes on
  Ubuntu 22.04.
- **The window talks to it over the system bus**,
  `org.betteros.TouchpadScroll1.eventN`, two methods. Better Touchpad's GNOME
  backend carries the vertical and horizontal scroll factors through it; they
  are available exactly when a pass-through answers.
- **Which device** is decided by capability bits, never names, and anything
  that can report a key is refused (`touchpad_scroll::caps`).
- **Smooth scrolling stays unavailable.** It is an application's drawing
  decision, not an input property.

## Consequences

- Better OS now sits in an input path. The added cost per frame is measured
  (`engineering/performance/touchpad-scroll-performance.md`); the kernel hop
  is owed to a run on real hardware.
- The residual risk: a compromised pass-through holds `/dev/uinput` and could
  drive its one touchpad clone, but cannot create another device, a keyboard,
  or an absolute pointer (the seccomp filter and lock). The
  review bounds it and the owner accepted it.
- libinput sees a clone rather than the original. Capabilities, ids, name,
  integration, and the libinput device group (trackpoint pairing) carry over.
- The factors are per machine, not per person or per pad: the window sets the
  same pair on every touchpad and says so, because Better Touchpad's scrolling
  settings are session-global (ADR 0010) and a per-pad factor would be the
  only per-pad scrolling setting. Each instance keeps them in
  `/var/lib/better-touchpad-scroll/factors`. The person's own choice still
  lives in their Better Touchpad configuration.
- A future GNOME with a scroll-factor key makes this replaceable by one row in
  the GNOME backend's key table, and the pass-through can then be removed.
