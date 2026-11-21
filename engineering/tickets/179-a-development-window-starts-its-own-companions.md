# Ticket 179 — A window started from a build directory never starts an installed program

**Blocked by:** none

## Why

Found on 2026-11-21 while photographing ticket 170 in a sandbox. Better
Monitor's window starts its tray by bare name from `PATH`, so a debug window
in a sandbox started the installed `/usr/bin/better-monitor-tray` against the
sandbox's bus and home; two were left running. The same day a sandbox's
private bus activated the installed Better Monitor and Better Awake services
from the system's D-Bus service directories. A window under test then talks
to programs of another version, and screenshots and tests describe a mix
nobody ships.

## What to build

- One rule, in one place, for how a Better OS program finds a companion
  program (a tray, a service, a helper): beside its own executable first, the
  packaged location otherwise, never a bare name resolved through `PATH`.
  Every place that starts a companion uses it (find them all: trays, services
  started on demand, the before-removal and on-disable programs, helpers).
- Under the test guard (ticket 178) nothing installed is started at all.
- The sandbox recipe in the project's notes for a private session bus lists
  only the service directories a test needs, so the system's Better OS
  services cannot be activated there.

## Done when

- Tests: a program run from a build directory starts its sibling from that
  directory and refuses when there is none; with the guard set it starts
  nothing.
