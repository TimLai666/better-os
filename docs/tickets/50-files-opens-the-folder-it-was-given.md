# Ticket 50 — A folder opened from the desktop opens in Better Files

**Epic:** Better Files (Issue #6) · **User Story:** a person who opens a folder
from another application lands in that folder · **Branch:** `ticket-50-52` ·
**Blocked by:** none · **Status:** ready

## What it delivers

`io.betteros.Files.desktop` declares `Exec=better-files %U` and
`MimeType=inode/directory;`, so the desktop hands Better Files a folder whenever
a person opens one from a browser's download bar, a terminal, or another file
manager. The window ignores that argument and always opens its default location.
After this ticket, the window opens the location it was given.

## Acceptance criteria

- [ ] A `file://` URI or a plain path to an existing directory opens that
      directory.
- [ ] A path to a file opens its parent directory with the file selected, or,
      if selection is not supported by the view, its parent directory.
- [ ] A URI Better Files cannot open (non-`file` scheme, missing path,
      unreadable directory) opens the default location and says why, rather
      than failing silently or exiting.
- [ ] More than one argument opens the first and reports the rest were ignored.
- [ ] An argument that starts with `-` and is not `--help` or `--version` is
      refused with exit 2, matching the rule `manager-gui` follows.
- [ ] Percent-encoded URIs (spaces, non-ASCII names) decode correctly.
- [ ] The argument parsing is a pure function with unit tests; no GPUI in them.
