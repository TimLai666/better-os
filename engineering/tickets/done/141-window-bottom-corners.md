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
  (`crates/gpui/src/window.rs`), `overflow_mask` builds a rectangle
  (`crates/gpui/src/style.rs`), and the wgpu shaders discard by the distance
  to that rectangle (`clip_to_mask`, `crates/gpui_wgpu/src/shaders.wgsl`);
  Zed's `main` at `0a514d7` (2026-11-20) is the same. Rounded clipping would
  need a fork of `gpui` and `gpui_wgpu` (a corner-radius field on
  `ContentMask`, a rounded-rectangle test in every fragment shader); nothing
  was bumped.
- **The geometry** is `better_ui::system_frame::corners`, tested without
  GPUI: the radii a window has (the theme's measured top and bottom radius
  while floating; square maximized, fullscreen, server-decorated, or tiled on
  any edge, as libadwaita 1.5's and Zorin's `window.csd.tiled*` rules make a
  GTK window's), `fill_radii`, `curve_depth`, `corner_depth`, `clip`,
  `overlay_area`, `shift_inside`, `submenu_x`, and the input region as
  whole-pixel spans.
- **The frame.** GTK draws it with every corner rounded; the bottom radius is
  measured from its own sample. The theme's CSS `outline` (libadwaita's
  `1px solid rgba(255, 255, 255, 0.07)` at `outline-offset: -1px`) is drawn by
  GTK with `gtk_snapshot_render_focus` (`gtk_render_focus` draws the border in
  4.14) and painted in a deferred layer above every other one, dialogs'
  smoke, menus and toasts included, as GTK paints it over all of a window's
  children; Zorin's `outline: none` draws nothing; the fallback paints
  Adwaita's value. The input region follows the curve grown by GTK's 12px
  handle, and a press in the window's own corner pixels outside the curve
  resizes from that corner.
- **One rule keeps content inside the curve**, and gives up no width:
  - every surface that reaches a corner paints its own fill into it with
    `window_chrome::corner_fill`, so its colour runs to the curve;
  - what scrolls past a bottom corner is painted with `corner_clip(inset)`,
    which raises only its bottom by the curve's depth at the rows' own side
    inset (`15 - sqrt(15² - (15 - 4)²) = 4.8` → 5px for rows 4px in at
    ZorinBlue's 15px, 4px at Adwaita's 12px, nothing for content 15px in or
    more, the whole radius for rows that run the full width), and the scroll
    extent ends with the same depth (`corner_depth`), so the last row scrolls
    fully above the curve and can be clicked;
  - a scrollbar there ends its track above the curve (`corner_scrollbar`);
  - tooltips, menus, submenus, suggestion and combo lists, and flyouts float
    through the kit (`float_at`, `anchored_panel`, the menus), which keeps
    them inside the window's rounded shape (`inside_window`); a submenu that
    does not fit opens on its parent's other side; a menu or list taller than
    the window scrolls within it. A test requires every window to float
    through the kit (`no_window_builds_its_own_floating_placement`).
- **Where it is applied.** In the kit: `NavigationView` (window fill, pane
  list and its scrollbar, content layer, overlay pane and its shadow),
  `ContentDialog` (smoke, inline card), toasts, menus, tooltips, the
  staged-changes bar. Per window, naming the surface that reaches a corner:
  Files' operations panel, side slot and two overlays; Git's list pane, its
  diff (full-width lines: the radius, with as much padding at the end of the
  list), and the drop bar shown while a commit is dragged in History and
  Graph; Graph's tooltip through `float_at`; Touchpad's and Awake's body fill,
  sidebar fill and page scrollbar; the app chooser's fill; the gallery's
  page, whose samples wider than a narrow window are cut at the page's
  padding. Checked and left alone, because each sits away from a bottom
  corner: Files' and Git's pane edges, Git's graph lane divider, Monitor's
  selection bar inside its tiles, Files' status bar (no fill, text 16px in),
  Git's footer hint (no fill), and the kit's in-control pieces (`list.rs`,
  `text_box.rs`, `tabs.rs`). Manager, Monitor, and Files' preview need nothing
  of their own. Better Launcher has no frame of the kit's, so `window_shape`
  is `None` there and its panel keeps its own design.
- `NavigationView`'s keyboard never scrolled the item it moved to into view
  (before this ticket too: the key handler only moved the cursor); it now
  scrolls it fully into view above the curve's depth (`reveal_offset`).
- The inline `ContentDialog` (the app chooser, the gallery's sample) has no
  shadow: drawn in place it sits on its surface, as DESIGN.md says of the page
  ("Don't put a shadow, a gradient, or a radius larger than 4px on anything
  that sits in the page"). The gallery's sample still reads as a dialog, with
  its card, stroke and button row (`window-gallery-adwaita-dark-s1-inline-dialog-sample.png`).

## Recorded limits

Both follow from GPUI clipping only to rectangles, at the pinned revision and
on Zed's `main`:

- A scrolling surface whose rows run its full width (Git's diff) gives up a
  band the height of the corner's radius along the bottom: while its rows
  scroll past, they are not drawn there, and the wheel and presses there do
  nothing. At the end of the list its last row is whole above the band.
- The gallery's samples wider than a narrow window are cut at the page's
  padding.

## Notes

- Tests:
  - `system_frame::corners`: radii per state and per tiled edge, curve depth,
    containment, fill radii sampled along the arc, content clipped at its
    inset never crossing the curve at scale 1, 1.25 and 2 with its sides
    unmoved, the band's arithmetic, floating boxes moved inside, a submenu
    that does not fit opening on the other side, outside rows, input spans
    pixel for pixel;
  - `system_frame::tests`: a press outside the curve resizes from its corner,
    the outside rows are resize zones, the bottom radius measurement;
  - `fluent::window_frame::tests`: four corners take the measured radius,
    square maximized, fullscreen, tiled on each edge, server-decorated;
  - `fluent::corner_tests`, through GPUI's test window with the window's shape
    given as the frame gives it (the test platform has no client
    decorations, so the frame itself is not driven there): a tooltip floated
    at the bottom corner lies in the overlay area and takes a press at its
    centre; a pane list scrolled to its end has its last row's bottom 5px above
    the window's edge and a press on its bottom pixel row selects it; the
    keyboard brings the last row into view the same way (this one fails
    without the fix); a full-width list scrolled to its end has its last row
    above the 15px band and a press there reaches it; the corner scrollbar's
    track ends 5px above the edge; the outline shows floating and not
    maximized, fullscreen, tiled or server-decorated, and not with a theme
    that has none;
  - `fluent::tests::no_window_builds_its_own_floating_placement`, and in
    `git-gui` `the_diff_drop_bar_and_graph_tooltip_keep_inside_the_rounded_corner`;
  - `measured_on_this_desktop` (ignored; needs a session), run in the
    sandbox: `measured-by-gtk.txt`.
- Evidence: `/home/tim/Developer/better-os-evidence/141/`, all from the build
  of 2026-11-21 03:20 CST (`BUILD.txt`), in a headless GNOME Shell 46.0 on a
  flat grey desktop:
  - at scale 1 in ZorinBlue-Dark, Adwaita light and Adwaita dark: Manager's
    System page scrolled to its end; Files at a short height in a folder of
    120 files, its pane and its grid scrolled to the end, and the details view
    scrolled to file-120; Git with a 200-line diff scrolled to its last line
    and that line's tick cleared by a click, and History with a row selected
    (its accent bar whole at the pane's leading edge); Monitor, Touchpad and
    Awake scrolled to the bottom; the app chooser; the gallery with its bottom
    pane row selected and its page scrolled, and narrowed so a page sample is
    wider than the window; each with a `-bottom-corners-zoom`;
  - Manager, Files and Git the same way at scale 2 in ZorinBlue-Dark;
  - at minimum size (ZorinBlue-Dark, scale 1): Git's File and Repository
    menus, a changed file's and a History row's context menus opened at the
    bottom (moved up inside the window), Files' overflow menu taller than the
    window (it scrolls), its View submenu opening on the parent's left side,
    and Files' context menu at the bottom right corner; a tooltip at Git's
    bottom edge; the gallery's live dialog over its smoke in Adwaita dark
    (the outline drawn above the smoke);
  - GNOME Text Editor beside the gallery in the three themes at scale 1 and 2
    and the fallback frame at scale 2: `*-focus-*.png`, side-by-side zooms of
    the bottom and top corners, maximized and tiled-left, and
    `*-measurements.txt`, which gives per corner the profile depth (rows from
    the edge before it runs straight; not the radius) and each row's width
    outside the window's own colour, a `*` marking a row where the window's
    own content (a card's edge) meets the edge rather than the curve. The
    profiles agree within one device pixel on every corner, the inner outline
    included (Adwaita dark: a 1px line 15 levels above the window's colour in
    both).
- The input region and corner resizing were tried by hand on this
  ticket's first build (2026-11-21 02:1x, scale 2) and not repeated on this
  one, which did not change them: a press in the handle's square corner beyond
  the grown curve focused GNOME Text Editor underneath, a press on the left
  handle near the corner stayed with the window, and dragging the bottom
  right handle and the window's own corner pixel outside the curve resized
  it. The tests that hold them (`input_follows_the_window_curve_grown_by_the_handle`,
  `a_press_outside_a_rounded_corner_resizes_from_it`) ran on this build.
- Not looked at: the frame on the owner's own desktop.
