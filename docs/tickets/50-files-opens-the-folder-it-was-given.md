# Ticket 50 — A folder opened from the desktop opens in Better Files

**Epic:** Better Files (Issue #6) · **User Story:** a person who opens a folder
from another application lands in that folder · **Branch:** `ticket-50-52` ·
**Blocked by:** none · **Status:** released in v0.2.8

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

## What was built

`better-files` now reads its arguments. `crates/files-gui/src/launch.rs` holds
all of it and has no GPUI in it: `invocation` sorts the argument list into
open, print, or refuse; `plan_start` turns the first location into a folder,
a file in a folder, or a reason it could not be opened; `main.rs` prints or
exits, and `app.rs` opens the first tab where the plan says.

- A `file:` URI is decoded the way GLib's `g_filename_from_uri` decodes one:
  the host must be empty or `localhost`, a fragment is refused, an escaped `/`
  or NUL is refused, and the decoded bytes become the path unchanged, so a name
  that is not UTF-8 still opens. Arguments are read with `args_os` for the same
  reason; `args` would panic on such a name.
- A plain path is taken as it is, and a relative one is resolved against the
  working directory, because a person at a terminal types `better-files .`.
  Anything else that starts with a URI scheme is refused as not local.
- A file opens its folder with the file selected. The selection is set by name
  before the listing has delivered anything, and the cursor lands on the file
  when it arrives (`FilesSession::select_by_name`).
- A location that cannot be opened — another scheme or host, a URI that does
  not decode, a missing path, a folder this user cannot list — opens the usual
  starting folder and says why in the window's notice bar, and on standard
  error for someone at a terminal. Further locations are counted and the notice
  says how many were ignored.
- The option rule is `manager-gui`'s: `--help`, `--version`, and their short
  forms `-h` and `-V` are answered; any other argument that starts with `-`,
  wherever it appears, is refused with exit 2. The short forms go beyond the
  ticket's wording on purpose, to match `manager-gui`.

Not done, deliberately:

- Better Files' own schemes (`trash:///`, `recent:///`) are refused like any
  other non-`file` scheme, as the ticket asks. The desktop entry declares only
  `inode/directory`, so the desktop never sends them.
- `..` in a plain path is kept as typed, as `files-core` keeps it everywhere,
  because resolving it lexically is wrong across a symbolic link.
- A selected file that the view hides (a dotfile with hidden files off) stays
  selected but unseen; nothing acts on it, because the selection commands only
  act on visible entries.

Verified: 17 unit tests in `launch.rs` and two session tests in
`integration_tests.rs`; `better-files --help`, `--version`, `-x`, and
`/tmp --select` were run and exited 0, 0, 2, 2. The window was opened in a
headless nested sway session with a percent-encoded URI to a file in a folder
named `My Files 文件` plus a second argument, and showed that folder with the
file selected and the ignored-argument notice; a second run with an `https://`
URI opened home with the not-local notice. Not verified: opening a folder from
another application on a real desktop, which goes through GIO's `%U` expansion
rather than an argument typed by hand.
