# Ticket 152 — The top bar component: service, view, Better OS menu, and the window actions

**Blocked by:** 151

## Why

`design/top-bar-spec.md` describes a bar drawn by a GPUI view the shell makes
a dock, with every decision in a Rust service. This ticket builds that
skeleton and everything on the bar that needs no other ticket: the Better OS
menu, the application name and the actions every window has, the clock's
place, the shared states, keyboard access, monitors, sizes, and the component
itself, so tickets 153 and 156–158 each add one part.

## What to build

- **Crates.** `top-bar-core` (no GPUI): the bar's model and every rule in spec
  sections 1–4, 7, and 8 (item order, what shrinks first at each width, which
  monitor gets a bar, what the application name shows for each focus state, the
  shared states table, the copy in both languages). `top-bar-service`
  (`/usr/libexec/better-top-bar`): owns `org.betteros.TopBar1`, asks the relay
  for the view and asks again after 1, 2, 4, then 30 seconds when it ends,
  stops after three failures a minute and reports it to Better Manager's Home,
  reserves the strip, hides GNOME's panel, and serves the view.
  `top-bar-gui` (`/usr/libexec/better-os/top-bar-view`): draws what the service
  sends, menus and flyouts as `AnchoredPopup`, talks only to the service, reads
  no environment variable beyond what GPUI and the session bus need (a test
  asserts the list), mounts `UpdateRestart` and exits rather than re-executing
  when its program is replaced (ADR 0036).
- **Better OS menu** (spec section 3): every item, its condition, its D-Bus
  call (logind, GNOME's session manager, `org.gnome.ScreenSaver`,
  `org.freedesktop.Accounts`), the lockdown keys, the failure toast.
- **Application name and the window actions** (spec section 4), through the
  relay's `FocusChanged` and `WindowAction`; Better OS windows get About,
  Settings…, Quit instead.
- **States** (spec section 2), including the adapter missing, off, or too old,
  GNOME other than 46, fullscreen, overview, and lock screen.
- **Keyboard and screen readers** (spec section 7): Ctrl+F2 focus, arrow
  navigation across items and menus, type-to-select, Escape returning focus;
  accessible names and roles from the spec's tables through GPUI's AccessKit;
  the focus visual.
- **Key bindings through Better Defaults**: GNOME's `toggle-message-tray` moved
  to the bar's `--notifications` (Super+V, Super+M) and Ctrl+F2 to `--focus`,
  captured, applied, verified, and restored when the bar is turned off, with
  the changed-elsewhere rule ticket 94 uses.
- **Monitors and scale** (spec section 8): primary only by default, "Show the
  top bar on every display" setting, monitors added or removed, per-monitor
  scale, fractional scale.
- **The component**: package `better-top-bar` built by
  `packaging/build-deb.sh` (user unit with the maintainer scripts from
  `packaging/maintainer/user-units.sh`, `verify-deb.sh` checks), manifest
  `components/manifests/better-top-bar.yaml` with its `paths` for
  `$XDG_STATE_HOME/better-os/top-bar/`, a dependency on `better-shell-adapter`
  at the version that carries the panel relay, Better Manager's Settings ›
  Top bar card (spec section 12), turning off and removing restoring every
  entry it applied.
- The clock item drawn with ticket 146's local-time function (its flyout is
  ticket 158).

## Done when

- [ ] Unit tests in `top-bar-core` for every rule named above, in both
      languages, including each width step of spec section 8 and each focus
      state of section 4.
- [ ] In a headless GNOME Shell 46 (sandbox rules), installed from the built
      package into the sandbox's own root: the bar appears at the top at
      scale 1 and 2, the Better OS menu opens and each item calls the right
      method on a fake service, a GTK test window's name and actions work,
      fullscreen hides it, the view killed comes back, and turning the
      component off restores GNOME's panel and the key bindings. Screenshots
      light and dark, zh-TW and English, in
      `/home/tim/Developer/better-os-evidence/152/`, compared with
      `design/top-bar/rest-other-*.html` and `betteros-menu-*.html`.
- [ ] Orca (or `accerciser` reading AT-SPI) in the sandbox names the bar's
      items as the spec's tables say.
- [ ] fmt, clippy on stable, the crates' tests, `verify-deb.sh`, and the
      license inventory check pass.
