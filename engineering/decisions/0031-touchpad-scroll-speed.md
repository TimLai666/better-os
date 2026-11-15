# ADR 0031: Touchpad scroll speed needs a privileged input pass-through

## Status

Superseded on 2026-11-15 by ADR 0030: the owner decided scroll speed is to
be built, and ticket 125 builds it. Proposed for ticket 88 until then; kept for
the experiment's evidence. Numbered 0031 because 0029 and 0030 were taken
on main while it was proposed.

## Context

The owner asked to set the touchpad's scroll speed (ticket 88, 2026-11-13).
GNOME 46 has no key for it in `org.gnome.desktop.peripherals.touchpad`, and
libinput leaves scroll scaling to the compositor. The approved order was to
first prove whether the existing GNOME Shell extension could scale scroll
deltas before they reach clients, inside ADR 0012's and ADR 0015's bounds.

### What was tried

A disposable GNOME Shell 46.0 (`gnome-shell --headless --wayland
--virtual-monitor 1600x1000`) with its own session bus, HOME, and runtime
directory, and Better Manager's own window as the client. Scroll input came
from a Clutter virtual pointer (`notify_scroll_continuous`, finger source), so
it entered Mutter the same way a touchpad's events do.

1. Baseline: five scroll steps moved the window's content 150 px.
2. `global.stage` `captured-event` handler that stops every smooth scroll and
   re-emits it doubled from a second virtual device: it saw **no** scroll
   event, and the content still moved exactly 150 px.
3. `Clutter.Event.add_filter(global.stage, …)` with the same rule: it saw no
   event of any type (motion, button, scroll), and the content moved 150 px.

What was observed is that neither hook an extension can reach saw a scroll
aimed at a client window. That fits Mutter forwarding such events to the
Wayland client in its own event handling before Clutter emits them (read from
Mutter's design, not traced in its source here), so an extension cannot
change a client's scroll. Holding a factor in GJS (which ADR 0012 forbids) would not even
help: there is nothing to apply it to.

## Options

1. **Do nothing.** Scroll distance keeps following GNOME's pointer speed, as
   the Scrolling page already says ("GNOME has no setting for this").
2. **An evdev/uinput pass-through.** A privileged helper grabs the touchpad's
   event node (`EVIOCGRAB`), re-emits every event on a uinput clone with the
   scroll-relevant motion scaled, and GNOME uses the clone. Costs: a system
   service with access to `/dev/input` and `/dev/uinput` (reads the keyboard's
   neighbour; ADR 0008's security review is owed first), a polkit action, a
   udev rule to hide the original from libinput, hot-plug and suspend handling,
   and a recovery path when it crashes with the device grabbed (the touchpad
   would stop). Two-finger scrolling is recognised by libinput from finger
   motion, so the helper scales contact motion only while two fingers are down,
   which changes pointer acceleration feel at the start of each scroll.
   Estimate: 3 to 4 weeks with tests and a VM proof, plus the security review.
3. **A Mutter patch or a newer GNOME.** Out of Better OS's reach on Zorin 18.

## Recommendation

Option 1 now, and keep the request open as a decision. Option 2 is feasible
but puts a root process between the person and their touchpad; it should not
start before ADR 0008's security review exists and the owner accepts that risk.
