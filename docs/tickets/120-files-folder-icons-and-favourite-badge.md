# Ticket 120 — Better Files: a Developer folder icon, and favourites keep their own icon

**Blocked by:** none · **Status:** open

## Why

The owner asked on 2026-11-14:

- The `Developer` folder should have its own icon wherever it appears. (The
  owner recalls asking for this in the specification; it is not in issue #6
  or the tickets in this repository as of this date, so this ticket is where
  it is recorded.)
- A folder pinned to Favourites must not turn into a star. Today every
  Favourites row uses the star (`crates/files-gui/src/shell.rs`
  `icon_for_row`, `SidebarSection::Favorites => IconName::Star`), so pinned
  Documents, Downloads and an ordinary folder all look the same. A pinned
  folder keeps the icon it has everywhere else, with a small star beside it.

Today only the XDG user directories have their own glyphs, and only as
sidebar places (`crates/files-gui/src/icons.rs` `place_icon`).

## What to build

- One function decides a folder's icon from what the folder is, used by the
  sidebar (places, favourites), the content view (list and icon views), the
  path bar, tabs and the preview: home, Desktop, Documents, Downloads, Music,
  Pictures, Videos, Templates, Public, the trash, a mounted device, a network
  location, and the developer folder, else the plain folder.
- The developer folder is `~/Developer` and the common spellings a person
  already has (`~/Developer`, `~/developer`, `~/Development`, `~/dev`,
  `~/Projects`, `~/Code`, `~/src`; decide the list, case-insensitively, and
  record it), directly under the home folder. Its glyph follows the existing
  Better Files glyph style (see the SVGs `icons.rs` serves) and reads as
  code, for example angle brackets on a folder. It is an original drawing.
- Optional but cheap: a folder that is a git repository's top level gets a
  small mark in the content view only if it can be known without touching
  the file system per row while drawing; otherwise leave it out and say so.
- A Favourites row shows the folder's own icon plus a small star badge at
  its corner (or a small trailing star, whichever DESIGN.md's density allows);
  a pinned file shows its file-type icon the same way. In the content view, a
  pinned folder may show the same small star.
- The icon decision never reads the disk while drawing; it uses the path
  and what the listing already knows.
- English and Traditional Chinese are unaffected except tooltips, if any.

## Acceptance

- [ ] Unit tests: each special folder maps to its icon in every place that
      asks; the developer spellings; a pinned Documents keeps the Documents
      icon with the badge; an ordinary pinned folder keeps the folder icon
      with the badge.
- [ ] Every new glyph is served and is an SVG (extend the existing test).
- [ ] Screenshots: sidebar with pinned special and ordinary folders, the home
      folder in icon and list views, light and dark.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
