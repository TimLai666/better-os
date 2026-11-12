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

## 4. Omarchy (withdrawn)

On 2026-11-11 the owner asked for Better OS to run on Omarchy (Arch Linux with
Hyprland) and considered moving from Zorin to it. On 2026-11-12 the owner
withdrew that: Better OS stays on Zorin and Ubuntu with GNOME, and Omarchy is
not a goal. Nothing was built for it. `omacom/folio` remains the reference for
Better Files' features, which does not depend on running on Omarchy.

## 5. Better Patch: known Linux desktop faults, found and fixed

Proposed by the owner on 2026-11-11 after two faults on one machine in one
evening, both reproduced and both fixed there.

- Every label in an Electron application was blank. The user's fontconfig
  cache was corrupt: every font query answered with one file and an unknown
  style. `fc-cache -f` rebuilt it and the text came back.
- The cursor was half size over an Electron 32 window on a display scaled to
  200%. On native Wayland that Electron does not scale its cursor, and
  `XCURSOR_SIZE` changes nothing. Running the application through XWayland
  fixed it with the picture still sharp. For a Flatpak that is a per-user
  override: `--nosocket=fallback-x11 --socket=x11` and
  `ELECTRON_OZONE_PLATFORM_HINT=x11`; `--socket=x11` alone leaves `DISPLAY`
  empty while `fallback-x11` is granted. Two limits: it was verified on a
  Flatpak only, and the override should stop applying once the application
  ships an Electron that scales its cursor.

The idea is a component that carries a list of known faults. Each entry can
detect its fault on this machine, say what is wrong in plain words, apply its
fix, check that the fix worked, and undo it. Open before planning:

- Whether this is a new component or a part of Better Defaults, whose engine
  already previews, applies, verifies, and restores.
- The rule for an entry. A fix is typed code in the repository with a test,
  never a script fetched or supplied at run time, and never a privileged
  change outside the daemon.
- What counts as worth an entry: a fault someone reproduced and a fix someone
  verified, starting with the two above.
- How entries stay correct across Zorin and Ubuntu releases.

## 6. One control center, inside Better Manager

On 2026-11-12 the owner decided that Better OS does not grow more small
apps. Better Awake, Better Touchpad, and Better Defaults stop being separate
windows, and Better Manager becomes the one control center, laid out like
Windows 11 Settings: categories on the left, settings cards on the right. It
holds what it holds today (installing, updating, and checking components),
those three, Better Patch, and hardware controls such as the performance
mode, the battery charge limit, the keyboard backlight, and the fans where
the hardware allows it. Writing those needs root, so it goes through
`manager-daemon` and polkit like every other privileged change.

On the owner's ASUS Vivobook S 14 M5406WA the kernel exposes, read on
2026-11-12: `platform_profile` with quiet, balanced, and performance;
`charge_control_end_threshold` on `BAT1`; `asus::kbd_backlight`, single colour
with brightness 0 to 3 (no RGB); and fan speed through hwmon. Whether its fan
curve can be set is not yet checked.

## 7. Every app complete before it is called done

On the same day the owner rejected shipping thin first versions: each app is
built with its whole feature set, in Fluent, and is not released until all of
it works on a real desktop with nothing for the person to type. Better Files
matches every feature of `omacom/folio` (MIT), rewritten in Rust and GPUI;
Better Git matches GitHub Desktop.
