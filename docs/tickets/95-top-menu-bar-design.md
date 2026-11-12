# Ticket 95 — A macOS-style menu bar at the top: design

**Blocked by:** 80 (done) · **Status:** ready

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

## What to deliver

A design in the same form as ticket 83: a spec with every state and the
zh-TW and English copy; Fluent mockups in light and dark; how it is built on
GNOME 46 (a GNOME Shell extension within the bounds ADR 0012 sets for GJS, or
a layer-shell surface if Mutter allows it, decided with evidence); how it sits
with Zorin's bottom taskbar (kept as is, or turned into a dock), with a
recommendation for the owner; what GPUI needs for exporting menus; and the
implementation tickets that follow.
