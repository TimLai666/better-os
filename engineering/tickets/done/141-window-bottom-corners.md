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

## What was built

- `better_ui::system_frame::corners` holds the geometry, tested without
  GPUI: the corner radii a window has (`window_corner_radii`: the theme's
  measured top and bottom radius while floating, square when maximized,
  fullscreen, server-decorated, or tiled on any edge, as libadwaita 1.5's and
  Zorin's `window.csd.tiled*` rules make a GTK window's), the window's shape,
  `fill_radii` (the radius a fill's corner needs to stay inside the curve,
  exact for an element flush with or near a corner), `clip` (the largest
  rectangle of an element clear of the curve, with a `Cut` naming which
  strip is given up), and the input region as whole-pixel spans.
- GTK draws the frame with every corner rounded (the CSS that squared the
  bottom is gone) and the bottom radius is measured from its own sample
  (`gtk.rs` `bottom_corner_radius`). The fallback frame rounds all four.
- `FramedView` rounds the window background on all four corners, publishes
  the window's shape for its content (`window_frame::window_shape`), gives
  the compositor an input region that follows the curve grown by GTK's 12px
  handle, and resizes from a press in the window's own corner pixels outside
  the curve.
- GPUI clips only to rectangles, so content is kept inside the curve by the
  containers that reach a corner: `window_chrome::corner_fill` draws a fill,
  border, and shadow with the corner rounded, `corner_clip` paints contents
  inside the rectangle `clip` gives, and `corner_panel` pairs them. The kit
  uses them in `NavigationView` (window fill, pane list, content layer, the
  overlay pane and its shadow, and the page, which gives up the 45° strip by
  default, `content_clip`), `ContentDialog` (smoke and the inline dialog the
  app chooser is), toasts, and the staged-changes bar.
- Per window: Files (`content_clip(None)`; the operations panel, the side
  slot, and its two overlays), Git (the list pane and the detail pane),
  Touchpad and Awake (the body fill, the sidebar, the page), and the app
  chooser (its fill). Manager, Monitor, and the gallery needed nothing beyond
  `NavigationView`; Files' preview window has no fill at its corners. Better
  Launcher's overlay has no frame of the kit's, so `window_shape` is `None`
  there and nothing changes: its panel keeps its own design.

## Notes

- Tests: `system_frame::corners` (radii per state and tiled edge, curve
  depth, containment, fill radii sampled along the arc, clip at scale 1, 1.25
  and 2 for every cut, outside rows, input spans pixel for pixel),
  `system_frame::tests` (a press outside the curve resizes from its corner,
  the outside rows are resize zones, the bottom radius measurement), and
  `fluent::window_frame::tests` (four corners take the measured radius, square
  maximized, fullscreen, and tiled on each edge). `measured_on_this_desktop`
  is an ignored test that draws with the session's GTK and asserts the same;
  it was run inside the sandbox.
- Measured by GTK in the sandbox (`measured_on_this_desktop`), top and
  bottom: ZorinBlue-Dark 15/15, ZorinBlue-Light 15/15, Adwaita 12/12, light
  and dark, scale 1 and 2.
- Compared in a headless GNOME Shell 46.0 (private bus, HOME, dconf, and
  XDG runtime directory, 4G scope, no-op ibus) beside GNOME Text Editor 46 on
  a flat grey desktop, ZorinBlue-Dark, ZorinBlue-Light, Adwaita light and
  dark, at scale 1 (1600x1000) and scale 2 (3200x2000), focused and in the
  background, plus the built-in Adwaita frame (`BETTER_OS_FRAME=adwaita`) at
  scale 2. Per-row corner profiles (pixels on each row that differ from the
  window's own colour) agree within one device pixel on every corner, for
  example Adwaita light scale 2 bottom left: GNOME Text Editor
  `[20, 16, 14, 12, 10, 9, 8, 7, 6, 5, 4, 4, 3, 3, 2, 2, 1, 1, 1, 1]`, Better
  OS `[19, 16, 13, 12, 10, 9, 8, 7, 6, 5, 4, 4, 3, 2, 2, 2, 1, 1, 1]`, the
  fallback frame identical to GNOME Text Editor's. The one-pixel difference is
  the same at the top corners and is libadwaita's 1px `outline` that GTK
  draws over its own content (in Adwaita dark it shows as a faint line along
  every edge of GNOME Text Editor); the kit has never drawn it. Maximized and
  tiled left, the gallery's corner pixels are the window's own colour.
- Input, at scale 2: a press in the handle's square corner beyond the grown
  curve focused GNOME Text Editor underneath; a press on the left handle near
  the corner stayed with the window; dragging the bottom right handle and the
  window's own corner pixel outside the curve both resized it.
- Every window was looked at (ZorinBlue-Dark, scale 1): Manager with its
  page scrolled to the bottom, Files with the operations panel open, Git with
  the history list and a toast at the bottom right, Monitor, Touchpad, Awake,
  the app chooser, Files' preview, and the gallery with its bottom pane row
  selected and its page scrolled (also Adwaita dark, scale 2), and Files with
  its pane scrolled so a row is cut at the bottom. No pixel outside the curve
  is square and the curve is filled.
- What the clip costs: where a page's own fill reaches the window's bottom
  trailing corner inside `NavigationView`, it stops the curve's 45° depth
  short of the right and bottom edges (4px at 12px, 5px at 15px), where the
  layer shows instead. In the gallery's window chrome page, whose samples are
  wider than the page, that strip is visible; a window whose panels reach the
  corner draws them with `corner_fill` and passes `content_clip(None)`, as
  Files does. Touchpad, Awake, and Git's diff give up the full radius along
  the bottom of the page instead, because their rows run the page's width.
- Evidence: `/home/tim/Developer/better-os-evidence/141/` — for each theme
  and scale `*-focus-gte.png` and `*-focus-better.png`, side-by-side zooms of
  the bottom and top corners, maximized and tiled shots, and
  `*-measurements.txt` with the profiles; `window-*` for every window.
