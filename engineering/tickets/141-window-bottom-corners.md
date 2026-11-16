# Ticket 141 — Every Better OS window has all four corners rounded like GTK's

**Blocked by:** none

## Why

The owner asked on 2026-11-16 that every Better OS window have the same
rounded corners, with the same radius, as GTK windows on the desktop.

## What is there today

- `better_ui::system_frame` (ticket 108) draws the frame through GTK 4 and
  measures the top corner radius from the theme as Chromium does
  (`crates/better-ui/src/system_frame/gtk.rs`, `corner_radius`).
- The bottom corners are drawn square on purpose
  (`gtk.rs` `frame`: "Bottom corners are square, as in Chromium: the content
  is painted over the bottom edge"). GNOME 46's libadwaita and GTK 4 windows
  on Zorin round all four corners, so Better OS windows look different at the
  bottom.

## What to build

- Round the bottom corners with the same radius the theme gives the top
  (measured, not hard-coded), and clip every view's content to that shape so
  nothing paints over the corner, including scroll views, sidebars and
  toasts reaching the bottom edge.
- Maximized, tiled and fullscreen windows keep square corners, as GTK does.
- The same in every window (Manager, Files, Git, Monitor, Touchpad, Awake,
  app chooser, the gallery); Better Launcher's panel follows its own design.
- Compare pixel by pixel with a GTK 4 / libadwaita window (GNOME Text
  Editor) on the owner's theme (ZorinBlue-Dark) and Adwaita, light and dark,
  at scale 1 and 2.

## Done when

- A test that the four corners' radius equals the measured one, and that
  maximized/tiled windows report square corners.
- Side-by-side screenshots against GNOME Text Editor.
