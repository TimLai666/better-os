# Ticket 143 — Evaluate COSMIC as Better OS's desktop, installed and switched to in one click

**Blocked by:** none

## Why

On 2026-11-16 the owner asked whether Better OS should move to another
desktop, named COSMIC (System76's Rust desktop, Epoch 1 since late 2025),
and set the bar for any such move: once Better OS is installed, one click
installs COSMIC and switches the person to it, with nothing else to set up.

## What to find out (no product code until the owner decides)

- Run Better OS's windows (GPUI) on COSMIC in a VM: Files, Git, Monitor,
  Manager, Launcher — frames, corners, scaling, portals, tray icons.
- For every GNOME-specific part (the shell adapter extensions and their
  gestures, window attention, the defaults engine's dconf/gsettings writes,
  Launcher's Super key, the Touchpad page's GNOME backend, the scroll
  pass-through's settings, GDM), say what COSMIC offers instead and the cost.
- What COSMIC makes simpler: a Better OS taskbar and top bar as layer-shell
  Rust processes (no GJS exception), one theme with one corner radius for
  every window (ticket 142).
- How COSMIC is installed on Zorin OS 18 and Ubuntu 22.04/24.04 (System76's
  packages, Pop!_OS repository, versions, signing), what a one-click install
  must do (packages, display manager choice, default session, person's
  settings carried over), and how to switch back and remove it cleanly.
- Write an ADR with the options — stay on GNOME, support both, or make COSMIC
  the primary desktop — with the work each needs, for the owner to decide.

## Desktop icons (owner, 2026-11-16)

The desktop must keep holding files and folders as it does today (Zorin's
`zorin-desktop-icons` extension on GNOME). Confirm whether COSMIC's own
desktop mode in COSMIC Files is in the stable release and how it behaves; and
weigh Better Files drawing the desktop itself as a layer-shell surface, which
would give the same desktop on GNOME-free installs and match Better Files'
own views. Either way the switch keeps what is on `~/Desktop` in place.

## Owner's rule this ticket must meet

Installing and switching is one action in Better Manager: no terminal, no
extra settings, and removal puts the previous desktop back.

## Our own desktop session (owner, 2026-11-21)

Told that on GNOME nothing but the shell can draw a panel, the owner asked
again whether Better OS should make its own desktop, and added a requirement:
every gesture has the transition macOS has (the result follows the fingers,
can be turned back halfway, and settles when they lift).

What has piled up since this ticket was written, each one reached on GNOME
only through shell code and its own widening of the GJS bounds:

- a taskbar and a top bar (ticket 142 item 3, tickets 151 to 159, ADR 0036);
- a notification island and a right-edge notification center (ticket 161);
- one corner radius for every window, which only a compositor can clip
  (ticket 142 item 2);
- the desktop surface and stacks (ticket 144);
- gestures that animate. AGENTS.md records that on GNOME 46 Better OS's
  gestures fire at the end and nothing animates. Only code inside the shell
  can drive its transitions, so finger-following gestures on GNOME mean
  JavaScript that takes progress values and drives the overview and the
  workspace animation through GNOME Shell's private methods, which change
  between releases;
- the edge swipe for the notification center, which no desktop reports and
  which needs the pad's raw contacts (ticket 161, N3).

So the evaluation gains a fourth option beside "stay on GNOME", "support
both", and "make COSMIC the primary desktop":

**A Better OS session on COSMIC's compositor.** `cosmic-comp` is Rust and
GPL-3.0, as this project is. Better OS would ship a session of its own: that
compositor (as released, or with patches kept as a small fork), and Better
OS's own panels, launcher, notifications, and gesture transitions as Rust
processes on layer-shell, with no GJS. The GPUI this workspace builds against
already has a layer-shell surface (`crates/gpui/src/platform/layer_shell.rs`
at the pinned revision); nobody has run a Better OS window as one.

Writing a compositor from nothing is not an option to evaluate: input,
several monitors, fractional scaling, input methods, screen sharing,
XWayland, accessibility, and graphics-driver faults are years of work before
the first Better OS feature.

For this option, find out and write down:

- which of the list above `cosmic-comp` already allows from outside (layer
  shell, session lock, foreign toplevel management, workspace protocols), and
  which need a change inside it: finger-following workspace and overview
  transitions for gestures Better OS defines, the edge swipe, clipping every
  window to one radius;
- what a fork costs to keep: how often upstream moves, how large each patch
  would be, and whether upstream would take them;
- what the session still needs that GNOME gives today for nothing: settings
  daemons, portals, polkit agent, keyring, screen lock and greeter, input
  methods (the owner types Chinese), Bluetooth and network prompts, XWayland;
- what of today's work carries over unchanged (every window, the services,
  the top bar's service and view) and what is GNOME-only and would be left
  behind (the shell adapter relays, the dconf adapters);
- how it meets the owner's rule above: one action in Better Manager installs
  the session and switches to it, and removal puts the previous desktop back.

Until this is decided, each new GNOME-only relay is work that the fourth
option would not use. Say in the ADR which of tickets 151 to 161 should wait.
