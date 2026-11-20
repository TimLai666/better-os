# Ticket 95 — A macOS-style menu bar at the top: design

**Blocked by:** 80 (done)

## Why

On 2026-11-12 the owner asked for a menu bar across the top of the screen like
macOS, and confirmed it after hearing its limit: on Linux today most
applications do not hand their menus to the desktop. GTK 4 and libadwaita
applications have no menu bar to export, Electron applications and the
browsers do not export one, and GPUI (Better OS's own toolkit) has no Linux
global-menu support. The GNOME extension that once did this (Fildem) is
unmaintained and served GTK 3 only.

## What the bar can be

- Left: a Better OS menu (about this computer, control center, sleep,
  restart, shut down, lock, log out) and the focused application's name.
- The application name opens the actions every window has: new window,
  full screen, minimize, close, quit.
- Better OS's own applications hand their full menus (File, Edit, View,
  Window, Help) to the bar through `com.canonical.dbusmenu` and the
  AppMenu registrar, which needs Linux global-menu support added in GPUI or in
  `better-ui`. Other applications show only the shared actions above.
- Right: the system tray (StatusNotifierItem), input method, network,
  sound, battery, the clock, and notifications.
- Tray icons move to the bar when it is on (owner, 2026-11-12): while the top
  bar is enabled, every StatusNotifierItem (Better OS's own and other
  applications') shows in it and not in Zorin's bottom taskbar, so the taskbar
  keeps its space for applications; when the top bar is turned off, they go
  back to the taskbar. The switch is one setting, applied and restored through
  Better Defaults, and nothing about it is per application.

## What to deliver

A design in the same form as ticket 83: a spec with every state and the
zh-TW and English copy; Fluent mockups in light and dark; how it is built on
GNOME 46 (a GNOME Shell extension within the bounds ADR 0012 sets for GJS, or
a layer-shell surface if Mutter allows it, decided with evidence); how it sits
with Zorin's bottom taskbar (kept as is, or turned into a dock), with a
recommendation for the owner; what GPUI needs for exporting menus; and the
implementation tickets that follow.

## What was built

- **The spec**, `design/top-bar-spec.md`, in ticket 83's form: the owner's
  seven decisions first (D1 shape, D2 the ADR, D3 the Zorin taskbar, D4 ticket
  142's left end, D5 a Better OS window's own menu row, D6 XEmbed icons, D7
  notifications), each with a recommendation and its costs; the sources read
  on the machine and upstream with URLs; the bar's layout and every part
  (Better OS menu, application name and the actions every window has,
  application menus by source, tray, input method, network, sound, battery,
  clock, notifications and calendar), every state, keyboard access, screen
  reader names, widths, scale, monitors, right to left, maximized windows,
  fullscreen, GNOME's own panel, and the zh-TW and English copy.
- **How it is built on GNOME 46, with evidence.** The compositor advertises no
  layer-shell (the whole registry list, read with a client that only bound the
  registry); Mutter's maintainers do not intend third-party panels. Mutter 46
  does let the shell make a window a dock (`meta_wayland_client_make_dock`,
  callable from GJS), which in `window.c` is not focused on click, skips the
  taskbar, pager, and Alt+Tab, is sticky, and goes below a fullscreen window;
  only a client the shell spawned can be one, because `new_indirect` and
  `setup_fd` are not in the typelib. So the recommended shape is a GPUI view
  the shell starts and makes a dock, with every decision in a Rust user
  service.
- **ADR 0036** (proposed), `engineering/decisions/0036-top-bar-dock-surface.md`:
  quotes each sentence of ADR 0018's and AGENTS.md's bounds the panel relay
  changes and gives the replacement text, with a closed table of programs the
  extension may start (one row now; ticket 142's taskbar would add the
  second). Numbers 0034 and 0035 were already used on other branches.
- **Global menus.** GPUI at the locked revision stores `set_menus` and exports
  nothing on Linux; the work is a menu model and dbusmenu export in
  `better-ui`, not a GPUI change. GTK 4 and GTK 3 `GtkApplication` windows do
  link their menubar to Mutter through `gtk_shell1` (the ticket's premise that
  GTK 4 has nothing to export is partly wrong); Qt and Chromium/Electron reach
  only the X11 registrar or KDE's `org_kde_kwin_appmenu`, which Mutter lacks;
  Firefox's export is off by default since 125.
- **The taskbar** (spec section 10): Zorin's taskbar kept without its clock
  and system menu now, ticket 142's taskbar later on the same relay, with the
  geometry for each shape. Zorin's `stockgs-keep-top-panel` is what shape B
  would use. **COSMIC** in one paragraph (section 11).
- **Mockups**, `design/top-bar/`: seven views in light and dark, plain HTML
  and CSS with no external resource, in ticket 83's technique and tokens.
- **Tickets 150–160** (148 and 149 were taken): one `dbusmenu` crate; the
  panel relay (blocked on the owner); the component with the Better OS menu
  and window actions; application menus; `better-ui`'s export with Better Git;
  the other Better OS windows' menus; the tray and its Better Defaults switch;
  input method, network, sound, battery; clock and notifications; the taskbar
  and the owner's desktop check; COSMIC.
- Ticket 142 gained a pointer to the shared design.

Not done or not verified:

- Nothing ran in a GNOME Shell: the four facts in spec section 14 (the view
  seeing `WAYLAND_DISPLAY`, the dock's placement and animation, a popup grab
  from an unfocused dock, `panelBox`'s strut when hidden) are ticket 151's
  first step, and its build stops if one fails.
- XWayland as a dock was rejected on reasoning, not tried.
- Whether every StatusNotifierItem re-registers when the watcher changes owner
  is checked by ticket 156, and on the owner's machine by ticket 159.
- The mockups are zh-TW only, as ticket 83's were; the English copy is in the
  spec. Tray items and notifications in them are illustrative.

### Screenshots

In `/home/tim/Developer/better-os-evidence/95/`: every mockup rendered by
headless Chrome at 1490 × 1000 (1010 × 1000 for the 960 view), one PNG per
HTML file.
