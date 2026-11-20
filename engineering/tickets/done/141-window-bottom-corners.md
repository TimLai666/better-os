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

- **Rounded clipping is not available in GPUI.** `ContentMask` is a rectangle
  (`bounds` only) at the locked revision `ae394f3`
  (`crates/gpui/src/window.rs`), `overflow_mask` builds a rectangle from the
  bounds and borders (`crates/gpui/src/style.rs`), and the wgpu shaders
  discard by the distance to that rectangle (`clip_to_mask`,
  `crates/gpui_wgpu/src/shaders.wgsl`); Zed's `main` at `0a514d7`
  (2026-11-20) is the same. Rounded clipping would need a fork of `gpui` and
  `gpui_wgpu` (a corner-radius field on `ContentMask`, an SDF test in every
  fragment shader); nothing was bumped.
- **The geometry** is `better_ui::system_frame::corners`, tested without
  GPUI: the radii a window has (the theme's measured top and bottom radius
  while floating; square maximized, fullscreen, server-decorated, or tiled on
  any edge, as libadwaita 1.5's and Zorin's `window.csd.tiled*` rules make
  a GTK window's), `fill_radii`, `curve_depth`, `corner_depth`, `clip`,
  `overlay_area` and `shift_inside`, and the input region as whole-pixel
  spans.
- **The frame.** GTK draws it with every corner rounded and the bottom radius
  is measured from its own sample. The theme's CSS `outline` (libadwaita's
  `1px solid rgba(255, 255, 255, 0.07)` at `outline-offset: -1px`) is drawn
  by GTK with `gtk_snapshot_render_focus` (`gtk_render_focus` draws the border
  in 4.14) and painted over the content, as GTK paints it over a window's
  children; Zorin's `outline: none` draws nothing; the fallback paints
  Adwaita's value. The input region follows the curve grown by GTK's 12px
  handle, and a press in the window's own corner pixels outside the curve
  resizes from that corner.
- **One rule keeps content inside the curve**, and gives up no width:
  - every surface that reaches a corner paints its own fill into it with
    `window_chrome::corner_fill` (radius from `fill_radii`), so its colour runs
    to the curve;
  - what scrolls past a bottom corner is painted with `corner_clip(inset)`,
    which raises only its bottom by the curve's depth at the rows' own side
    inset (`15 - sqrt(15² - (15 - 4)²) = 4.8` → 5px for rows 4px in at
    ZorinBlue's 15px, 4px at Adwaita's 12px, nothing for content 15px in or
    more, the whole radius for rows that run the full width), and the scroll
    extent ends with the same depth (`corner_depth`), so the last row scrolls
    fully above the curve and can be clicked;
  - a scrollbar there ends its track above the curve (`corner_scrollbar`);
  - tooltips, menus, submenus, suggestion lists, and anchored panels are
    moved inside the window's rounded shape (`inside_window`): GPUI places
    them anywhere in the surface, the shadow included.
- **Where it is applied.** In the kit: `NavigationView` (window fill, pane
  list and its scrollbar, content layer, overlay pane and its shadow),
  `ContentDialog` (smoke, inline card), toasts, menus, the staged-changes
  bar, tooltips. Per window, naming the surface that reaches a corner: Files'
  operations panel, side slot and two overlays; Git's list pane and its diff
  (full-width lines: the radius, with as much padding at the end of the
  list); Touchpad's and Awake's body fill, sidebar fill and page scrollbar;
  the app chooser's fill; the gallery's page, whose samples wider than a
  narrow window are now cut at the page's padding. Manager, Monitor, and
  Files' preview needed nothing of their own. Better Launcher has no frame of
  the kit's, so `window_shape` is `None` there and its panel keeps its own
  design.
- The inline `ContentDialog` (the app chooser, the gallery's sample) has no
  shadow any more: drawn in place it sits on its surface, as DESIGN.md says
  of the page ("Don't put a shadow, a gradient, or a radius larger than 4px
  on anything that sits in the page"); the gallery's sample still reads as a
  dialog (its card, stroke, and button row), see
  `window-gallery-adwaita-dark-s1-inline-dialog-sample.png`.

## Notes

- Tests: `system_frame::corners` (radii per state and per tiled edge, curve
  depth, containment, fill radii sampled along the arc, content clipped at
  its inset never crossing the curve at scale 1, 1.25 and 2 with its sides
  unmoved, the band's arithmetic, floating boxes moved inside, outside rows,
  input spans pixel for pixel), `system_frame::tests` (a press outside the
  curve resizes from its corner, the outside rows are resize zones, the
  bottom radius measurement), `fluent::window_frame::tests` (four corners take
  the measured radius, square maximized, fullscreen, tiled on each edge, and
  server-decorated). `measured_on_this_desktop` (ignored; needs a session)
  draws with the session's GTK and asserts round floating corners, square
  maximized, fullscreen and tiled corners, and no outline then; run in the
  sandbox, its output is `measured-by-gtk.txt`: ZorinBlue-Dark top 15 bottom
  15 with no outline, Adwaita top 12 bottom 12 with an outline, scale 1 and 2.
- Evidence, all from the build of 2026-11-21 02:36 CST (`target/debug`,
  every window binary within 02:36:24–02:36:30), in
  `/home/tim/Developer/better-os-evidence/141/`, in a headless GNOME Shell
  46.0 (private bus, HOME, dconf and runtime directory, 4G scope, no-op
  ibus) on a flat grey desktop:
  - every window at scale 1 in ZorinBlue-Dark, Adwaita light and Adwaita
    dark (`window-*-s1-*.png`, each with a `-bottom-corners-zoom`): Manager
    scrolled to the bottom; Files at its smallest height with its pane and
    its file list scrolled to the end; Git with a 200-line diff scrolled to
    its last line and that line's tick cleared by a click, and History with a
    row selected (its accent bar whole at the pane's leading edge); Monitor,
    Touchpad and Awake scrolled to the bottom (the scrollbar thumb whole
    above the curve); the app chooser; the gallery with its bottom pane row
    selected and its page scrolled, and narrowed so a page sample is wider
    than the window;
  - Manager, Files and Git at scale 2 in ZorinBlue-Dark;
  - a context menu opened at Files' bottom right corner and a tooltip opened
    at the bottom edge (Git's commit button): both inside the window, clear
    of the curve; before, the tooltip painted past the window's edge;
  - GNOME Text Editor beside the gallery (`*-focus-gte.png`,
    `*-focus-better.png`, `*-bottom-corners-gte-vs-better.png`,
    `*-top-corners-gte-vs-better.png`, maximized and tiled-left shots) in the
    three themes at scale 1 and 2, and the fallback frame at scale 2.
    `*-measurements.txt` gives, per corner, the profile depth (how many rows
    from the edge before the window's edge runs straight; not the radius) and
    each row's width outside the window's own colour. They agree within one
    device pixel on every corner, the inner outline included: Adwaita dark
    scale 1 bottom left, GNOME Text Editor `[9, 8, 6, 5, 4, 3, 2, 2, 1, 1, 1, …]`,
    Better OS `[8, 8, 6, 5, 4, 3, 2, 2, 1, 1, 1, …]` (the trailing 1s are the
    outline, +15 levels over the window colour in both); Adwaita light scale
    2, both `[20, 16, 14, 12, …]`/`[19, 16, 13, 12, …]`; the fallback frame
    identical to GNOME Text Editor's. A profile starting at 24 or 32 is the
    page's own content at that row, not the edge.
- Every image above was looked at at pixel level: no square pixel outside a
  curve, no band of another colour in a corner, nothing hidden at the end of
  a list.
- What remains of the trade-off, as the rule allows: rows that run the full
  width (Git's diff) stop the corner's radius above the window's edge while
  they scroll past it; at the end of the list they are whole. Rows 4px in
  stop 4–5px above it; pages padded 15px or more give up nothing.
