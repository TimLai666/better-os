# Ticket 94 — Super+Space opens Better Launcher, and it finds any file

**Blocked by:** 82 · **Status:** ready

## Why

The owner wants Better Launcher to open like Spotlight on macOS and to find
any file or folder, not only applications. Two facts on the owner's machine,
read on 2026-11-12:

- Better Launcher has no shortcut at all: `launcher-platform/src/shortcut.rs`
  leaves the binding undecided. `<Super>space` is GNOME's
  `switch-input-source` (with `<Shift><Super>space` backwards). The owner
  types Chinese with ibus and switches Chinese and English inside the input
  method with Shift, so moving the input-source switch costs nothing.
- GNOME's indexer, Tracker 3 (`tracker-miner-fs-3`), is running. It indexes
  Desktop, Documents, Music, Pictures, and Videos recursively, the home folder
  and Downloads only at their top level, and skips every folder that holds a
  `.git`, so nothing under `~/Developer` can be found.

## Decisions (owner, 2026-11-12)

- `<Super>space` opens Better Launcher. Input-source switching moves to the
  Windows key, `<Alt>Shift_L` (and backwards with `<Shift>Alt_L`), as on
  Windows.
- File search covers the whole home folder, project folders included.

## What to build

- **The shortcut is set by installing.** Installing or enabling Better
  Launcher through Better Manager writes the GNOME custom keybinding
  (`<Super>space`, command `better-launcher`) and moves
  `org.gnome.desktop.wm.keybindings switch-input-source` and its backwards
  key, through Better Defaults' preview, capture, apply, verify, and restore
  engine, so disabling or removing the launcher puts both back exactly as
  they were. A value the person changed by hand is not overwritten without
  asking (Better Defaults' changed-externally rule).
- **Search finds files.** As the person types, Better Launcher asks Tracker
  over D-Bus (`org.freedesktop.Tracker3.Miner.Files`, SPARQL on
  `nfo:fileName`) for files and folders whose names match, off the UI thread
  with stale results dropped, and shows them in Folders and Files sections
  under Applications. Enter opens with the default application; Ctrl+Enter
  shows it in Better Files. No index of Better OS's own.
- **The index covers home.** The same install step sets Tracker's
  `index-recursive-directories` to the home folder, removes `.git` from
  `ignored-directories-with-content`, and adds build and dependency folders to
  `ignored-directories` (`node_modules`, `target`, `.cargo`, `.rustup`,
  `.cache`, `__pycache__`, `.venv`, `venv`, `dist`, `build`, `.gradle`,
  `.m2`), again through the restore engine. The control center's search page
  (ticket 89 or its own) lists these folders and lets the person change them.
- Tests use a fake Tracker and an isolated dconf; nothing touches the
  person's real settings or index.

## Acceptance

- [ ] After installing the built package in a disposable VM or container with
      a GNOME session, Super+Space opens the launcher, Alt+Shift switches
      input sources, and removing the launcher restores both.
- [ ] A file in a project folder is found by name within the time Tracker
      takes to index it; `node_modules` and `target` contents are not.
- [ ] Unit tests for the query, ranking with files, stale-result dropping,
      and the restore of every setting written.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## Spotlight, not an application (owner, 2026-11-13)

The owner finds it wrong that Better Launcher behaves like an application
that is opened and closed, and wants it horizontal, like macOS Spotlight,
since it sits in the middle of the screen. So this ticket also turns the
launcher into a summoned panel:

- **Always ready.** A session service keeps the launcher process resident
  with its catalog and icons loaded; Super+Space only shows it, so it appears
  at once. It starts with the session and is not something a person opens or
  quits: no entry in the application menu or the taskbar, and losing focus,
  Escape, or launching something hides it.
- **Spotlight's shape.** A wide, short search bar centred a little above the
  middle of the screen (about 680px wide); results appear below it in one
  list grouped by kind (Top hit, Applications, Calculator, Folders, Files),
  with a preview of the selected result on the right; nothing below the bar
  until something is typed, or the pinned apps in a single row when the
  person prefers. The current Start-menu grid becomes "All apps" behind a
  key, not the first thing shown.
- **Proving how it is drawn.** A GPUI window is an `xdg_toplevel`, so Zorin's
  taskbar lists it and Mutter may place and animate it like an application.
  Before building, prove one of: hiding it from the taskbar and placing it
  through Better OS's existing GNOME Shell extension (within ADR 0012 and
  0015's bounds), or drawing the panel in the extension itself. Record the
  choice and its evidence in an ADR.
