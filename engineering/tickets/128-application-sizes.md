# Ticket 128 — Applications show how much space they take

**Blocked by:** none

## Why

The owner asked on 2026-11-15 why no application in Better Files'
Applications view shows its size. Windows' Installed apps and macOS's
Applications folder both do.

## What to build

- The Applications view (`Location::Applications` in Better Files) shows each
  application's size in the Size column and the Properties panel, from what
  the system already records, read unprivileged:
  - a Debian package: dpkg's `Installed-Size` of the package that owns the
    desktop entry, plus its exclusive dependencies only if that can be told
    reliably (otherwise the package alone, labelled as such);
  - a Flatpak: the installed size `flatpak` reports for the app (runtimes
    shared with others are not added, and the panel says so);
  - a Snap: the size of its current revision's squashfs;
  - an AppImage or a program in the person's home: its file size.
- An application whose size cannot be found shows "—" with the reason, never
  a guess. Sizes are read off the UI thread and cached; sorting by size works.
- The person's own data for the application (its `~/.config`, `~/.local/share`
  and cache folders) is not counted silently; if shown, it is a separate
  "Data" figure.

## Done when

- Tests for each source against fixtures (dpkg status, flatpak and snap
  output), the missing case, and sorting.
- Screens in zh-TW and English, light and dark, at 1600 and 960.
