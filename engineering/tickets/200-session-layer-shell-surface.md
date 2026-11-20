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

- [x] The facts are in Notes with how each was seen, and screenshots in
      `/home/tim/Developer/better-os-evidence/200/`.
- [x] `better_ui::layer`'s rules are unit-tested (outputs added, removed,
      rescaled; a change that needs recreation; input region).
- [ ] The test program, built from this branch, shows a top strip with an
      exclusive zone and a popup on two nested outputs at scale 2.
- [x] `better_core::desktop` tests for `BetterOS:COSMIC`, `COSMIC` alone, and
      GNOME's values.
- [x] fmt, clippy on stable, and the crates' tests pass.

**Blocked:** on the owner. A GPUI layer surface that fills an axis (the top
bar, the taskbar, the desktop) dies of a protocol error on any output above
100 % (GPUI defect 1 below); choosing how GPUI gets the fix is the owner's.
Two outputs on `cosmic-comp` itself also wait for ticket 201's VM.

## Notes

### Where it stands

Built: `packaging/tests/cosmic-nested.sh` (the nested `cosmic-comp`),
`packaging/tests/session-layer-facts.sh` (every fact below, again, in one
command), `crates/layer-check` (`better-layer-check`, the GPUI program, and
`better-wl-probe`, screenshots, clicks, keys, globals), `better_ui::layer`
(`rules`, `clock`, and `LayerSurfaces`), and
`better_core::desktop::CurrentDesktop::is_better_os_session`. The spec's
"Surfaces on layer-shell" section documents the helper, the three GPUI defects,
and the test compositor's commands. Evidence, screenshots and logs:
`/home/tim/Developer/better-os-evidence/200/`.

Left, and why:

1. **A filled surface above 100 %.** Seen on `cosmic-comp` at 150 % and 200 %
   (`09-scale-1.5-…`, `09-scale-2-…`: no strip, no reserved zone) and on sway
   at 200 % for the second of two strips (`logs/outputs-scale2-strip-strip.log`).
   `logs/wayland-debug-cosmic-scale2-filled-strip-viewport-error.log` shows the
   order: `wp_fractional_scale_v1.preferred_scale(240)` arrives before the
   layer surface's first `configure`; GPUI rescales a window whose width is
   still the requested 0 and sends `wp_viewport.set_destination(0, 32)`
   (`gpui_linux` `wayland/window.rs` lines 1286–1290 at `ae394f3`); the
   compositor answers `error 0: negative or zero values` and the connection is
   dead. Zed's `main` (fetched 2026-11-21) has the same code (`window.rs` line
   1560), so a GPUI bump alone does not fix it. What it would take:
   - **(a, recommended) a three-line GPUI patch**: in `set_size_and_scale`,
     skip `set_destination` while either dimension is 0 (the configure that
     follows sets it). Carrying it means a Better OS fork of `zed` at
     `ae394f3` plus the patch, named in a `[patch."https://github.com/zed-industries/zed"]`
     section of the workspace manifest: every Zed crate's source line in
     `Cargo.lock` changes, the third-party notices are regenerated, and the fix
     is offered upstream so the fork can go at the next bump. Defect 3 below is
     a one-line condition in the same file and could ride along.
   - (b) wait for upstream, then bump GPUI: Zed's `main` has not fixed it, and
     a bump drags in everything since `ae394f3` against the
     `gpui-component` revision the workspace pins.
   - (c) no GPUI change: give a filled surface an explicit width learned from
     the output's logical size. GPUI exposes no logical size it can be trusted
     for at fractional scales (its display bounds divide by the integer
     `wl_output` scale: 960 for a 1920-pixel output at 150 %), so this needs a
     second Wayland connection of Better OS's own reading `xdg-output`, and
     still breaks when an output's mode changes. Not recommended.
   Fixed-size surfaces are not affected: a 400×120 surface with a popup
   showed at 150 % and 200 % on `cosmic-comp`
   (`13-cosmic-scale-*-fixed-size-surface-with-popup.png`), and per-output
   600×40 surfaces followed outputs at 200 % on sway (`10/11/12-outputs-scale2-fixed-*`).
2. **"A top strip with an exclusive zone and a popup on two nested outputs at
   scale 2"** cannot pass until 1 is decided, and "nested" can only mean sway:
   `cosmic-comp`'s nested backends have exactly one output (`src/backend/winit.rs`
   `all_outputs` returns one; `src/backend/x11.rs` calls `add_window` once, line
   367, at commit `6968c5d`). On sway it passed at scale 1 with two outputs,
   one added and one removed (`10/11/12-outputs-scale1-strip-*`). Two outputs on
   `cosmic-comp` itself wait for ticket 201's VM (a virtio-gpu with two heads).

### The facts, and how each was seen

All on `cosmic-comp` `0.1~1795167359~24.04~6968c5d` (Pop `noble`, `.deb`
SHA-256 `2cf26f10…7606f`, checked against the signed `InRelease`), nested in a
headless sway 1.9, unless marked sway. GPUI at `ae394f3`. Every line comes from
`better-layer-check`'s own event log and `better-wl-probe find`'s pixel
measurement on a screenshot; the logs are in the evidence directory.

- **Libraries.** Every library `cosmic-comp` links resolves against Zorin 18's
  (`ldd`, nothing missing). It runs nested with its winit backend.
- **Layers** (`02-layers-around-a-window.png`): background and bottom are
  under an ordinary window, top and overlay over it.
- **Anchors** (`anchors/`): all sixteen combinations, measured: none centred
  (540,340); one edge on that edge, centred along it; two adjacent edges in
  that corner; opposite edges with a 0 size stretched (1280×32 across, 32×800
  down); three edges a strip along the middle one; all four the whole output.
- **Exclusive zone** (`01`, `03`): a maximized window starts below the 32-pixel
  strip (y=68: 32 plus `cosmic-comp`'s 36-pixel title bar). Changed in place
  to 96 with `Window::set_exclusive_zone` (`04b`): the window is reconfigured
  to start at 132. Changed with the strip's height to 64 (`04`): the strip is
  64 high and the window starts at 100. That second change first went through
  `Window::resize`, which turned the 0 width into 1 (`window.rs` line 1501) and
  the strip vanished; `layer::rules` now recreates a filled surface for a size
  change (`Recreate::FilledSize`), which is what `04` shows.
- **Changing anchors after creation** (`05`): a new surface at the bottom, the
  maximized window moves up (y=36). The first attempt closed the old surface
  first and the program quit: GPUI's default quit mode on Linux ends the
  application when its last window closes, and a new GPUI window took about a
  second to appear. `LayerSurfaces` now opens the new one first and sets the
  quit mode to explicit.
- **Keyboard** (`06-*`, `logs/kb-*`), a window focused, then keys, a click on
  the strip, keys, a click on the window, keys: `none` never received a key
  and the click did not move focus; `on-demand` took focus when it mapped,
  kept it when clicked, and gave it to the window when that was clicked;
  `exclusive` took every key, and a click on the window activated it only for
  a moment before focus came back.
- **Popup** (`07-*`): a click on the strip opens an `xdg_popup` parented to the
  layer surface (GPUI's `WindowKind::AnchoredPopup`, `get_popup` on the layer
  surface) with a grab; it took the keys even from a `none` strip, Escape
  closed it, and the next key went to the window that had focus before.
- **Input region** (`08`): an overlay with input only in its left 300 pixels;
  a click at x=100 reached the overlay, a click at x=800 the window under it.
- **Scale** (`09-*`, `13-*`): at 100 % everything above. At 150 % and 200 % a
  fixed-size surface and its popup show at the right size; a filled one dies
  (Left 1). At 150 % `cosmic-comp` sends `preferred_scale(180)`, and GPUI
  then sets buffer scale 2 on entering the output, because `cosmic-comp`
  offers `wl_compositor` version 5 and GPUI's integer path runs below 6
  (`window.rs` lines 1191–1195): every GPUI window draws at 2 and is scaled
  down, the right size and slightly soft (`logs/wayland-debug-cosmic-scale1.5-fractional-overridden.log`).
- **Outputs** (sway, `10/11/12-outputs-*`): with `--every-output` a surface
  opened on each of two outputs; an output added was given one within the
  500 ms poll (GPUI has no event for it); an output removed had its surface
  closed by the compositor (`closed output=5`), which is how `LayerSurfaces`
  learns it, since GPUI keeps listing a removed output (`client.rs` line 1286,
  a TODO); a new output after that got a surface again.
- **A GPUI window's start** takes about a second in the nested setup (the
  trace shows wgpu enumerating adapters over a dozen registry round trips per
  window), which is why recreation opens before it closes.
