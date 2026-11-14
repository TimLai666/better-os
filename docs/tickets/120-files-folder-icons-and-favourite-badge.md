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

- [x] Unit tests: each special folder maps to its icon in every place that
      asks; the developer spellings; a pinned Documents keeps the Documents
      icon with the badge; an ordinary pinned folder keeps the folder icon
      with the badge.
- [x] Every new glyph is served and is an SVG (extend the existing test).
- [x] Screenshots: sidebar with pinned special and ordinary folders, the home
      folder in icon and list views, light and dark.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **One decision.** `icons::folder_glyph` says what a folder is from its
  path, the session's user directories and its mount table, never the disk:
  home, Desktop, Documents, Downloads, Music, Pictures, Videos, Templates,
  Public, the trash, a mounted drive's top folder, a server's folder (a
  network location, or a GVfs mount), the developer folder, else a folder.
  It is used by the sidebar's places and favourites, the path bar, tabs,
  the columns view, and the list and icon views; the preview pane draws no
  folder icon, so it has nothing to change.
- **The developer folder** is a folder directly under home named, in any
  case, `Developer`, `Development`, `dev`, `Projects`, `Code`, `src`,
  `source` or `repos` (`icons::DEVELOPER_NAMES`). Its glyph is a folder with
  angle brackets, drawn for this in the 24 px, 2 px stroke grid of Better
  Files' other glyphs. In the full-colour views, where icon themes have no
  developer folder, it is the theme's folder with a small `</>` mark on a
  disc at its corner.
- **Favourites keep their icon.** A pinned folder is drawn as itself (a
  pinned Documents keeps the Documents icon) with a small accent star on
  its icon's corner (the kit's `NavItem::emblem`). Better Files' sidebar
  only offers folders as favourites (a pinned file shows as unavailable, as
  before), so there is no file-type case to draw.
- **New glyphs**, each served and tested as SVG: home, Templates, Public,
  developer, the `</>` mark; drives and servers use the kit's hard drive and
  globe.
- A git repository mark in the content view was left out: knowing it needs
  a look inside each folder for `.git`, which the drawing must not do.

Tests (`tests.rs`): every special folder, the developer spellings and what
is not one, a drive and a GVfs server folder, a pinned Documents and an
ordinary pinned folder with the star (and a place without it), and every
glyph served as SVG (`icons.rs`). Screenshots in `.shots/`:
`t120-home-grid-light.png`, `t120-home-list-light.png`,
`t120-home-grid-dark.png`, `t120-home-list-dark.png` (the sidebar with a
pinned Documents, an ordinary folder and the developer folder).
