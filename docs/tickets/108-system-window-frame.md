# Ticket 108 — Windows have the system's frame and window buttons

**Blocked by:** 80 (done) · **Status:** implemented, not merged

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

## What was built

### GTK draws the frame

`crates/better-ui/src/system_frame/gtk.rs` opens `libgtk-4.so.1` with
`dlopen` (and `libadwaita-1.so.0` when it is installed, since GNOME's own
applications are libadwaita applications and libadwaita, Zorin's build
included, decides how their frame looks). Nothing links GTK: every symbol,
GTK's, GLib's, GSK's, and Cairo's, is looked up at run time. GTK is
initialised once, on its own thread, which also runs GLib's default main
context so GTK's display connection is read and libadwaita sees theme
changes. GTK never creates a window. Each CSS node is an unrealized `GtkBox`
with the node's name, classes, and state, as Chromium builds them
(`AppendCssNodeToStyleContextImpl`), and every part is drawn with
`gtk_render_background` and `gtk_render_frame` into a Cairo image surface at
the window's whole scale (a fractional scale is drawn at the next one up):

- the frame around a `2 × 64` px window, focused and in the background, with
  square bottom corners as in Chromium (`window.background.csd`, plus
  `.maximized` or `.tiled`);
- the header bar background, cut by the window's top corner radius, focused
  and in the background (`headerbar`, as libadwaita applications show it in a
  toolbar view);
- minimize, maximize, restore, and close (`windowcontrols > button.titlebutton.*
  > image`), normal, hover, and pressed, focused and in the background, with the
  symbolic icon from the current icon theme in the image's colour.

The shadow's reach on each side and the top corner radius are measured from
the drawn pixels, as Chromium does, because GTK 4 cannot report CSS values.
`system_frame/service.rs` keeps one drawing per theme, icon theme, light or
dark, scale, and maximized or tiled state, and follows
`gsettings monitor org.gnome.desktop.interface` and `… wm.preferences`: a
change to the theme, the icon theme, the colour scheme, or `button-layout`
redraws every window within a fifth of a second. Nothing writes a setting.

### The kit paints it

`better_ui::window_chrome::root` replaces `gpui_component::Root::new` in every
framed window (Better Git, Files and its preview, Manager, Monitor, Awake,
Touchpad, the app chooser, and the Fluent gallery); Launcher and Monitor's
pop-up are overlays and keep no frame. `fluent::window_frame::FramedView`
paints the four corners and four stretched edges around the content, sets the
client inset to the widest side of the shadow (kept while maximized so
restoring does not resize), takes input only on the window and GTK 4's 12px
resize handle around it, and starts a resize from every edge and from 24px
corners, as `gtk/gtkwindow.c` does. The window background is painted there
with the desktop's top corners, so app roots no longer paint one.

`fluent::chrome::TitleBar` is as tall as GTK's header bar, paints GTK's header
background and buttons, puts them where `button-layout` says (start, end,
both, or none), and keeps the app's own controls (back button, icon, title,
menus, search) between the two groups. Double, middle, and right clicks on its
empty space do what `action-*-click-titlebar` say (a dialog never maximizes);
dragging moves the window and the compositor restores a maximized one. The
frame is drawn in the window's own appearance (the owner's decision,
2026-11-14), which is the desktop's
`color-scheme` when the person chose "system". When the GTK theme names an
accent (`accent_bg_color`, `accent_fg_color`) the Fluent accent takes it.
`window_chrome::title_bar` now returns this bar, so the older windows that used
gpui-component's bar have the system's too.

Without GTK 4, or with `BETTER_OS_FRAME=adwaita`, the kit draws libadwaita
1.5's frame itself, from values read out of libadwaita's own stylesheet:
12px corners, a 47px header with its shade, and round title buttons.

### Evidence

Headless GNOME Shell 46 (`--headless --wayland --virtual-monitor 1600x1000`,
private D-Bus session, HOME/XDG/dconf in a temporary directory, pointer and
keyboard through `org.gnome.Mutter.RemoteDesktop`), GNOME Text Editor beside
the Fluent gallery: Adwaita and ZorinBlue, light and dark, focused, in the
background, and maximized. Corners, shadow, border, header colour and height,
and button size, position, colours, and icons match at the pixel; the
difference is the bottom corners, which are square, as in Chromium and Claude
Desktop, because GPUI cannot clip content to a rounded shape. Each of the four
button layouts drew correctly and changed live; all eight edges and corners
resized; double click maximized (and minimized when set to `minimize`), middle
click followed `toggle-maximize` when set, right click opened Mutter's window
menu, dragging moved the window and restored a maximized one, and the three
buttons minimized, maximized, restored, and closed.

### After review

- A drawing request never blocks a window: the render thread sleeps in GLib's
  own poll (GTK's descriptors plus a wake pipe), so an idle process has no
  frame-related wakeups; it gives up for good, and windows stop waiting, if it
  cannot start, cannot open GTK, or ends for any reason.
- After a window's first drawing, its maximized, tiled, and other-appearance
  drawings are made at once, and a window keeps its last drawing until a new
  one is ready, so maximizing or switching light and dark never shows the
  fallback. The first frame waits at most one second, for the settings and the
  drawing together; the settings are read off the UI thread with one
  `gsettings list-recursively` per schema.
- A drawing made for a theme that has just been replaced is dropped; a new
  theme or icon theme drops every drawing and takes its images out of the GPU
  atlas, while a new colour scheme keeps them.
- The accent reaches gpui-component's controls at once (focus rings,
  selection, switches, sliders, links), goes back to Windows' blue when the
  theme has none or GTK is unavailable, and Better Launcher follows it too.
- Title bar presses become moves only past GTK's 8px drag threshold. A
  fullscreen window has no frame. The inset never shrinks, and the fallback
  reserves the most GTK may need, so GTK arriving late does not resize the
  window; on X11, where the input region cannot be narrowed, it stays within
  20px. A window the compositor decorates still gets its background.
- `gsettings monitor` is started again, up to five times with a growing pause,
  if it exits.
- Every package with a window recommends `libgtk-4-1` and `libadwaita-1-0`, and
  `verify-deb.sh` checks it; the license inventory lists GTK 4, libadwaita, and
  Cairo as libraries loaded at run time.

### Not done here

- GTK 3 is not a second drawing path: GTK 4 is on every GNOME this targets
  (42 and later), and without it the built-in Adwaita drawing is used.
- The bottom corners are square: GPUI clips content only to rectangles.
