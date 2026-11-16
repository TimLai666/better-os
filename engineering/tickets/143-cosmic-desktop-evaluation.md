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

## Owner's rule this ticket must meet

Installing and switching is one action in Better Manager: no terminal, no
extra settings, and removal puts the previous desktop back.
