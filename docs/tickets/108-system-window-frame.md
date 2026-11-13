# Ticket 108 — Windows have the system's frame and window buttons

**Blocked by:** 80 (done) · **Status:** ready

## Why

The owner, 2026-11-14: Better OS windows should have the same window
corners, edges, and window buttons as the rest of the desktop, the way
Claude Desktop's do, while still putting the app's own controls in the
title bar.

GNOME's compositor offers no server-side decorations to Wayland clients
(the owner's session lists 27 Wayland globals and none of them is a
decoration manager, read 2026-11-14), so every Wayland app draws its own
frame. GTK apps and Chromium/Electron apps draw it from the GTK theme, which
is why they match the desktop; Better OS draws a Windows 11 frame and
caption buttons (`better_ui::fluent::chrome::TitleBar`).

The owner's settings, read 2026-11-14: GTK theme and icon theme
`ZorinBlue-Dark`, `color-scheme` `prefer-dark`, `button-layout`
`appmenu:minimize,maximize,close`, double-click on the title bar
`toggle-maximize`.

This changes the earlier "everything Fluent" direction for the window frame
only: the content and the controls inside the title bar stay Fluent.

## How it is drawn (owner, 2026-11-14)

Chromium, and so Claude Desktop, draws its Linux window frame and caption
buttons by asking GTK to render them with the current theme, offscreen, and
painting the result (`ui/gtk/window_frame_provider_gtk.cc` and
`ui/gtk/nav_button_provider_gtk.cc` in Chromium, BSD-licensed, read for the
method only). Claude Desktop on the owner's machine is a native Wayland
window (absent from the XWayland window tree), so its frame is not drawn by
the compositor; it looks like the system because GTK draws it.

The owner chose the same: Better OS asks GTK to draw the frame, the shadow,
and the caption buttons, rather than imitating the theme.

- GTK is loaded at run time (`dlopen` of libgtk-3 or libgtk-4, as Chromium
  does), so a machine without it falls back to Adwaita-like drawing and the
  binary does not link GTK. Initialise GTK without opening a display
  connection of its own where possible, and never let it create windows.
- For each needed part (frame edges and corners with their shadow, focused
  and unfocused; each caption button in normal, hover, pressed, and backdrop
  states, light and dark) render it through GTK's style machinery into a
  Cairo image surface at the window's scale, cache the images per theme,
  scale, and state, and draw them in GPUI. Re-render when the theme, the
  colour scheme, the scale, or `button-layout` changes.
- GTK and Cairo are LGPL system libraries, loaded dynamically: add them to
  the "dynamically linked system libraries" section of the license inventory
  and to the packages' Depends as recommended libraries.

## What to build

- **Caption buttons like the system's.** `TitleBar` draws minimize,
  maximize/restore, and close the way the GTK theme does on GNOME and Zorin
  (round flat buttons with the symbolic icons `window-minimize-symbolic`,
  `window-maximize-symbolic`, `window-restore-symbolic`,
  `window-close-symbolic` from the current icon theme, the theme's hover and
  pressed fills), in the order and on the side
  `org.gnome.desktop.wm.preferences button-layout` says (left, right, or
  both; buttons left out are not drawn), and follows changes to it live.
- **Corners, edges, shadow like the system's.** The window's corner radius,
  border, and shadow (focused and unfocused) match a GTK 3/4 window under
  the current theme, with no frame when maximized or tiled; the invisible
  resize area around the window is as wide as GTK's, and every edge and
  corner resizes.
- **Title bar behaviour like the system's.** Double, middle, and right click
  on empty title bar space follow `action-double-click-titlebar`,
  `action-middle-click-titlebar`, and `action-right-click-titlebar` (the
  right click shows the compositor's window menu), dragging moves the
  window, and dragging a maximized window restores it.
- **Light and dark follow the system** (`color-scheme`), and the accent
  follows GNOME's accent where the theme has one.
- **Where reading the theme is not possible** (another desktop, a theme the
  kit does not know), fall back to Adwaita's look, never to the old Windows
  look.
- The app's own controls in the title bar (Better Git's menus and repository
  picker, Better Manager's back button and search) stay, laid out between
  the button groups.
- Every app gets this through the kit; no app draws its own frame. The
  Fluent gallery shows the frame in light and dark, with buttons on the left
  and on the right.

## Acceptance

- [ ] Side-by-side screenshots in a headless GNOME Shell 46 session (ticket
      102's method) of a Better OS window next to a GTK window (for example
      GNOME Text Editor) under Adwaita and under a Zorin theme, light and
      dark, focused and unfocused, and maximized: frame, corners, shadow,
      and buttons match.
- [ ] `button-layout` with buttons on the left, on the right, and only close
      each draws correctly, and a live change redraws.
- [ ] Every edge and corner resizes; double click maximizes; right click
      opens the window menu.
- [ ] Unit tests for reading `button-layout` and the title bar actions, and
      for the fallback.
- [ ] Tests never change the person's real GNOME settings.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
