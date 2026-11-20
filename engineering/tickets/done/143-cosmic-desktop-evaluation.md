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

## Notes

Delivered on 2026-11-21 as
[ADR 0038](/engineering/decisions/0038-cosmic-as-the-desktop.md) (proposed):
the research with its sources, every GNOME-specific part with what COSMIC
offers and the size of the work, installing on Zorin OS 18 and Ubuntu, the
three options with their work and their effect on the tickets, the
desktop-icons recommendation ticket 144 waits on, and the owner's decisions
C1 to C7 at the top.

The VM part ran in a disposable QEMU VM booted from Pop!_OS 24.04 build 29
(COSMIC Epoch 1.11.0): Better Files, Git, Monitor, Manager, and Launcher from
the published 0.2.16 packages at 100 %, 150 %, and 200 %, the trays, the
portal file chooser, Zhuyin through Fcitx 5, COSMIC Files' desktop mode, and
`wayland-info`. Screenshots are in `/home/tim/Developer/better-os-evidence/143/`.
The windows were not built from this branch: the host's disk could not hold
the build and the ISO together, and 0.2.16 already has the system frame.

What the evaluation could not run, and the ADR leaves to option B's own
check: COSMIC installed onto Zorin OS 18 or Ubuntu 24.04 with the pin, GDM
starting the COSMIC session, switching back, removal, real hardware, and a
touchpad. The owner's choice among the options is not part of this ticket.
