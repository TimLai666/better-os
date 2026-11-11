# Ticket 76 — Backlog: directions the owner set on 2026-10-05 and 2026-11-11

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

## 4. Better OS runs on Omarchy

On 2026-11-11 the owner changed the project's positioning: Omarchy, an Arch
Linux distribution with the Hyprland compositor, has to be able to use Better
OS. Three parts of the project assume otherwise today, and each needs its own
decision before any ticket is cut:

- **Installation.** Better Manager, the privileged daemon, the health check,
  the release packages, and `install.sh` are all built on `.deb`, `apt-get`,
  and `dpkg`. Arch uses pacman. Either the daemon gains a second package
  backend with its own packages, or the components reach Omarchy another way
  (an AUR package, a plain archive) and Better Manager does not manage them
  there.
- **Host support.** `better-core::host` refuses any host that is not Ubuntu
  based, and every manifest lists `ubuntu` and `zorin` as its distributions.
- **Desktop integration.** Better Touchpad's gestures run through a GNOME Shell
  extension and its settings through dconf, and Better Defaults writes GNOME
  keys. Hyprland has neither. The windows themselves are GPUI and should run
  on any Wayland compositor, which nobody has tried on Hyprland.

This direction and direction 3 pull apart: a Debian-based distribution keeps
`.deb` and can keep GNOME, while Omarchy needs pacman and Hyprland. The ADR
that replaces "not a distribution fork" has to say which comes first and
whether both are goals.
