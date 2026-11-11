# Ticket 76 — Backlog: three directions the owner set on 2026-10-05

**Status:** backlog, not planned · **Blocked by:** a plan for each item

These are directions, not work items yet. Each needs its own plan and tickets
before anyone builds anything, and the third needs an ADR because it changes
what `AGENTS.md` says the project is.

## 1. A Git client to replace GitHub Desktop

On the owner's Zorin 18 machine GitHub Desktop opens with every label blank.
The owner wants a first-party Git desktop client built with GPUI like the rest
of the suite. Open before planning: which GitHub Desktop workflows it covers
first (clone, commit, branch, push and pull, pull requests), how it signs in to
GitHub, and whether it shells out to `git` or links a library.

## 2. WinUI style across every Better OS window

Every first-party window moves to the look of WinUI (Fluent): its type scale,
spacing, control shapes, and surfaces. This replaces the direction in
`DESIGN.md` and ADR 0004, so it starts with a new design baseline in
`better-ui` and a new ADR, then a pass over each window. Open before planning:
which WinUI version is the reference, and how far the titlebar and window
chrome follow it on GNOME.

## 3. A Better OS distribution built on Debian

The long-term goal is a Linux distribution of its own, based on Debian and no
longer dependent on Zorin or Ubuntu. `AGENTS.md` currently says Better OS is
not a distribution fork, so this needs an ADR that replaces that statement.
What it touches: the packaging matrix (Debian releases instead of Ubuntu
22.04 and 24.04), the host probe and `install.sh`, the manifests' distribution
IDs, a signed APT repository (signing is still an undecided follow-up, and a
distribution cannot ship without it), an installer image, and the choice of
desktop shell, since Better Touchpad and Better Defaults are written against
GNOME 46. Open before planning: which Debian release, which desktop, and
whether Zorin and Ubuntu stay supported while the distribution is built.
