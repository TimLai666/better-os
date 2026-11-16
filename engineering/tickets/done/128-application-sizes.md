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

## Notes

- Delivered by `files_platform::app_size` (the records), `files_gui::app_sizes`
  (cache, reading thread, wording) and the Size column, grid caption,
  Properties panel and application details panel. The policy is the
  "Application sizes" section of `engineering/specs/files-gui-policy.md`.
- Exclusive dependencies are not added: dpkg cannot tell them reliably, so a
  Debian application is its package alone and says so.
- Better OS's own packages had no `Installed-Size` (dpkg-deb does not compute
  it), so their applications showed "—" with "records no size".
  `packaging/build-deb.sh` now writes the field and `verify-deb.sh` requires
  it; packages already installed keep "—" until they are updated.
- The catalog marks an entry with `X-Flatpak-RenamedFrom` as a Flatpak (GNOME
  Text Editor, Evolution). The sizer only follows a Flatpak by its deployment
  or a `flatpak` Exec, so those read as their dpkg package; the catalog's own
  classification is left for its own change.
- Screens were taken in a headless GNOME Shell 46 on this machine's real
  catalog, dpkg and Flatpak records, with a scratch home.
