# ADR 0025: Better Launcher is one instance on org.betteros.Launcher1

## Status

Accepted on 2026-08-30 (tickets 21 and 27). Written as a record on 2026-11-15
from the delivery status decision log.

## Decision

- Better Launcher is a single-instance transient application that owns
  `org.betteros.Launcher1` and serves `org.freedesktop.Application`.
  `Activate` opens the overlay and `ActivateAction("toggle")` toggles it.
- The global keyboard shortcut is expressed as GNOME settings the launcher
  names and never writes; applying them belongs to Better Defaults.

## Why

Every activation path (a dock, a panel, `gio launch`, a second
`better-launcher`) has to reach one overlay, and the interface a
`DBusActivatable` desktop entry is activated through gives them one route.
Clicking a launcher icon then never closes the launcher. An unprivileged
application cannot register a system-wide shortcut on GNOME, and Better OS does
not grab the keyboard to fake one.

## Options rejected

- A new process per activation: two overlays and no toggle.
- Grabbing the keyboard for the shortcut: needs privileges and fights the shell.

## Consequences

- The activation paths are described in `/engineering/specs/launcher-activation.md`.
- The default key combination stays undecided.
