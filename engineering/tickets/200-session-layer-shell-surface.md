# Ticket 200 — A Better OS window can be a layer-shell surface on `cosmic-comp`

**Spec:** better-os-session.md S4 (foundation for every surface)
**Blocked by:** none

## Why

Every surface of the session (top bar, taskbar, launcher, switcher, overview,
notification island and center, desktop) is a GPUI window opened as
`WindowKind::LayerShell`. GPUI at the pinned revision (Zed `ae394f3`) has it,
and nobody has run a Better OS window as one (ADR 0038, "GPUI's layer-shell
surface at the pinned revision"). If it does not work on `cosmic-comp`, no
surface ticket can be built, so this is checked first and cheaply.

## What to build

- **A nested `cosmic-comp` for tests.** System76's released `cosmic-comp`
  for `noble` (the version ADR 0038 names, or the newest), extracted with
  `dpkg-deb -x` into a scratch directory (never installed on the host), run
  nested in a headless sway under AGENTS.md's sandbox rules
  (`systemd-run --user --scope -p MemoryMax=4G -p MemorySwapMax=0`, a no-op
  `ibus-daemon` first on `PATH`, private `dbus-run-session`, every `XDG_*`
  directory temporary, ended by PID), as `packaging/tests/cosmic-nested.sh`.
  If its libraries do not resolve against the host's, or it does not run
  nested, record why and use the VM from ticket 201 instead, saying so here.
- **First, the facts**, with a small GPUI test program in that compositor:
  a layer surface on each of the four layers; anchors on every edge
  combination; an exclusive zone that a maximized `xdg_toplevel` respects, and
  changed after creation; keyboard interactivity none, on-demand, and
  exclusive, and what focus does on a click; an `xdg_popup` parented to the
  layer surface, with keyboard input; `set_input_region` letting clicks through
  outside it; fractional scale at 100, 150, and 200 %; a second output added
  and removed (sway's headless outputs). Record each in Notes. If a surface
  cannot be shown, anchored, or given an exclusive zone, stop and bring it to
  the owner.
- **`better_ui::layer`**, the shared helper every surface uses: one window per
  output following outputs as they come and go (GPUI opens one surface per
  call), recreating a surface to change anchors, margins, or layer (GPUI does
  not expose those after creation), the input region kept to what is drawn,
  and an animation clock for slides and finger-following inside a fixed
  surface. GPUI-free rules (which outputs get a surface, what to recreate on
  which change) in a module with tests.
- **The session check in `better_core::desktop`**: `is_better_os_session()`
  reading `XDG_CURRENT_DESKTOP`'s `BetterOS` entry, beside `is_gnome()`, the one
  place every component asks.

## Done when

- [ ] The facts are in Notes with how each was seen, and screenshots in
      `/home/tim/Developer/better-os-evidence/200/`.
- [ ] `better_ui::layer`'s rules are unit-tested (outputs added, removed,
      rescaled; a change that needs recreation; input region).
- [ ] The test program, built from this branch, shows a top strip with an
      exclusive zone and a popup on two nested outputs at scale 2.
- [ ] `better_core::desktop` tests for `BetterOS:COSMIC`, `COSMIC` alone, and
      GNOME's values.
- [ ] fmt, clippy on stable, and the crates' tests pass.
