# Ticket 151 — The shell adapter's panel relay

**Blocked by:** the owner accepting ADR 0036 and choosing shape A (decisions
D1 and D2 in `design/top-bar-spec.md` section 0)

## Why

On GNOME 46 a Better OS window can be a panel only if GNOME Shell starts it and
makes it a dock (`design/top-bar-spec.md` section 9). ADR 0036 sets what the
shared extension may do for that; this ticket builds exactly that and the
tests that hold it there.

## What to build

- **First, the four facts the spec leaves unverified (section 14), in a
  headless GNOME Shell 46** under the sandbox rules (`systemd-run --user
  --scope -p MemoryMax=4G -p MemorySwapMax=0`, a no-op `ibus-daemon` first on
  `PATH`, private `dbus-run-session`, every `XDG_*` directory temporary, ended
  by PID): a GPUI test window started through `Meta.WaylandClient.spawnv`
  connects on `WAYLAND_SOCKET` and sees `WAYLAND_DISPLAY`; `make_dock`,
  `move_frame`, and a 32px-tall `xdg_toplevel` give a strip at y = 0 that
  Mutter neither moves nor animates; an `xdg_popup` grabbed from a click on the
  dock receives keys; hiding `panelBox` drops its strut. Record each result in
  this ticket's Notes. If any fails, stop and bring it to the owner before
  building on it.
- **The relay** in `adapters/gnome-shell-adapter/`: interface
  `org.betteros.ShellAdapter1.Panels` exactly as `design/top-bar-spec.md`
  section 9.4 lists it, in `org.betteros.ShellAdapter1.Panels.xml` beside the
  extension; counterpart `org.betteros.TopBar1` accepted only when its owner
  runs `/usr/libexec/better-top-bar`. `StartView` spawns the one program in
  ADR 0036's table with its fixed argument list; `ReserveStrip` adds one
  invisible chrome actor per monitor and edge; `SuppressBuiltInPanel` and
  `SuppressKeyboardIndicator` hide and restore GNOME's actors; `FocusChanged`,
  `Monitors`, `FullscreenChanged`, `InputSources`, and the notification
  members report what the shell has, to the counterpart only; `WindowAction`,
  `ActivateWindow`, `FocusView`, `ShowOverview`, `ActivateInputSource`,
  `ActivateInputProperty`, and the notification actions perform the shell's
  own actions, resolving each index against the shell's current list at call
  time. Everything GNOME's is put back when the counterpart's name goes and on
  `disable()`.
- **A Rust contract module** (`top-bar-core::shell`, GPUI-free) naming every
  member, its signature, and the wire values (window actions 0–5, edges 0–1,
  flag bits), as `window_attention_core::bus` does for the attention relay.
- **`crates/top-bar-service/tests/extension.rs`**: the allow-list assertions
  ADR 0036 lists (one spawn, constant argv equal to the table; only the named
  fields assigned; every method checks the caller and the executable; no input
  argument of type `s`; every hidden GNOME actor restored on name loss and on
  `disable()`; signals sent to the counterpart only), a second test that
  applies forbidden variants (a remembered window list, a string argument, a
  second program) and requires each to fail, and `gjs` parsing the module.
- AGENTS.md's GJS rule and the extension's header comment gain ADR 0036's
  exception in its words, and name the closed program table.

## Done when

- [ ] The four facts are recorded with how they were seen.
- [ ] In the headless shell, a fake counterpart (a test binary at the accepted
      path inside the sandbox's own root, or the check made injectable for the
      test only) starts a test view, gets `ViewStarted`, places it, reserves a
      strip that a maximized test window respects, hides and restores GNOME's
      panel, and gets `FocusChanged` for a GTK test window with its GTK menubar
      path. Screenshots in `/home/tim/Developer/better-os-evidence/151/`.
- [ ] Removing the counterpart's name restores GNOME's panel, the keyboard
      indicator, and the work area, and the view exits.
- [ ] The extension tests above pass; the window attention relay's tests pass
      unchanged.
- [ ] fmt, clippy on stable, and the crates' tests pass.
