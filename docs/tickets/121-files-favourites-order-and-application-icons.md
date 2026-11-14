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

- [ ] Unit tests: drag from index a to b for every pair on a list with
      foreign lines between; drop-to-pin at a position; the context menu
      moves; icon resolution against a fixture icon theme (themed name,
      inherited theme, hicolor, pixmaps, absolute path, SVG, missing).
- [ ] Sandbox evidence: before and after screenshots of the Applications
      location with fixture desktop entries and a fixture icon theme; a
      favourite dragged to a new place and still there after a restart.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
