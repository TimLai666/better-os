# Ticket 121 — Better Files: reorder favourites by dragging, and applications show their icons

**Blocked by:** none · **Status:** open

## Why

The owner reported on 2026-11-14:

- Pinned items in Favourites cannot be reordered. The code can move a
  favourite (`crates/files-gui/src/bookmarks.rs` `move_up`/`move_down`/
  `move_to`), but the only way to reach it is two keyboard commands that act
  on the sidebar cursor (`session.rs` `Command::MoveBookmarkUp/Down`, ~2113);
  nothing in the sidebar can be dragged and the context menu offers no move.
- In the Applications location every application shows the default icon
  instead of its own. The view falls back to a generic glyph when an entry
  has no icon path (`crates/files-gui/src/views.rs` `entry_icon`,
  `FallbackIcon::Application`); find why no application gets one (the
  desktop entry's `Icon=` name not resolved through the icon theme, the
  resolved file being SVG or a size the view will not load, paths dropped
  with `EntryWarning::DroppedIconPath` in `apps.rs`, or the lookup never
  being called) and fix the cause. Better Launcher already shows application
  icons (`crates/launcher-gui/src/overlay.rs`); share its lookup instead of
  writing a second one.

## What to build

- Favourites reorder by dragging a row within the section: a line shows
  where it will land, dropping writes the new order to the shared bookmarks
  file (foreign lines keep their places, as `bookmarks.rs` already
  guarantees), Escape cancels. Dragging a folder from the content view into
  the section pins it at the position it is dropped, not always at the end.
  The context menu gains Move up, Move down, Move to top and Move to bottom;
  the existing keyboard commands stay and are listed in the shortcuts page.
- Applications show their real icons in every view mode and size, in the
  sidebar's Applications section, in "Open with" lists and anywhere else an
  application is named: resolved through the icon theme with the same
  fallbacks the launcher uses (theme, inherited themes, hicolor, pixmaps,
  absolute paths, SVG and PNG), off the UI thread, cached, and refreshed when
  the icon theme changes. An application whose icon truly cannot be found
  keeps the fallback glyph.
- Check the app chooser and Better Manager's Default apps page for the same
  missing icons and fix them through the shared lookup.

## Acceptance

- [x] Unit tests: drag from index a to b for every pair on a list with
      foreign lines between; drop-to-pin at a position; the context menu
      moves; icon resolution against a fixture icon theme (themed name,
      inherited theme, hicolor, pixmaps, absolute path, SVG, missing).
- [x] Sandbox evidence: before and after screenshots of the Applications
      location with fixture desktop entries and a fixture icon theme; a
      favourite dragged to a new place and still there after a restart.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **Why every application had the same icon.** The content view asked the
  icon theme for the generic `application-x-executable` for every
  Applications row and never looked at the row's own `Icon=`, which the
  catalog had carried all along (`ApplicationFacts::icon`). Nothing was
  dropped and no lookup failed: the lookup was never called.
- **Shared lookup, nothing moved.** Better Launcher's lookup already lived
  in `better_ui::icons::IconSearch::resolve` (the desktop's theme and what
  it inherits, Adwaita, `hicolor`, pixmaps, absolute paths, SVG and PNG),
  and the app chooser already used it. Better Files now resolves each
  row's `Icon=` with it on a thread of its own, once per value, and draws
  the theme's generic application icon until it is found or when it
  cannot be. When the desktop's icon theme changes, every icon is looked
  up again. Better Manager's Default apps page draws no application icons
  at all (its choices are text buttons), so it has no missing icon to fix.
- **Favourites move by dragging.** A favourite row drags within its
  section; an accent line shows where it will land; the drop writes the new
  order to the shared bookmarks file (other programs' lines stay, in their
  order); Escape cancels any drag. A folder dragged from the content view
  onto a favourite is pinned at that place (or moved there if already
  pinned) instead of at the end. The menu gains Move to top and Move to
  bottom beside Move up and Move down; Alt+Up and Alt+Down stay and are on
  the shortcuts sheet. The kit's `NavigationView` gained `decorate_rows`
  for this.

Tests (`tests.rs`): every from/to pair on a list with foreign lines between,
a drop pinned at a position (and an already pinned one moved), Move to top
and to bottom through the menu, the shared lookup against a fixture theme
(themed, inherited, `hicolor`, pixmaps, absolute path, SVG, missing), and a
row's icon arriving from the resolving thread. Screenshots in `.shots/`,
with fixture desktop entries and a fixture icon theme:
`t121-applications-before.png` (every row the generic icon),
`t121-applications-after.png` (each its own; the one whose icon does not
exist keeps the fallback), `t121-favourite-dragging.png` (the line above
the target), `t121-favourite-after-restart.png` (the new order kept).

## After review

- **Nothing read while drawing.** Every icon lookup (themed names and
  applications' own) goes to one thread and is cached, misses too; the row
  shows a fallback until the answer arrives. That thread asks for the
  desktop's icon theme every ten seconds, with or without the frame
  service, and starts over in a new theme.
- **Absolute `Icon=` paths** (shared with Better Launcher and the app
  chooser): only a PNG, SVG or XPM file, at most 4 MB, a PNG at most
  1024 px each way by its header, never under `/proc`, `/sys` or `/dev`;
  a path that is not UTF-8 is kept as it is.
- **The bookmarks file is shared.** Every edit reads the file again and
  applies the change by location, so a favourite added in Nautilus or
  another window survives; the sidebar follows outside edits; a
  symbolically linked bookmarks file stays a link; each write has its own
  temporary name. Moving a favourite moves only bookmarks among the lines
  bookmarks hold: every other program's line keeps its exact line.
- **Only folders are pinned** by a drop; a file dragged over Favourites
  shows no line and is not taken. A folder already pinned and dragged in
  from the content view shows the same line as a favourite moved by hand.
