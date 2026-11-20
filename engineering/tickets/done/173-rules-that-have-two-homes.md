# Ticket 173 — Rules found with two homes on 2026-11-21

**Blocked by:** none

## Why

AGENTS.md: a rule two places need lives in one place both call. Workers on
tickets 134 and 146 found these and left them as out of scope:

- `git-platform` writes its own `.trashinfo` file (`crates/git-platform/src/
  patch.rs`), a second trash writer beside `files-platform`'s
  (`crates/files-platform/src/trash.rs`): the name escaping, the collision
  rule for an existing name, and the per-volume trash choice can differ.
- `monitor-gui`'s `startup.rs::for_desktops` matches a desktop entry's
  `OnlyShowIn`/`NotShowIn` itself, while `app-catalog-core` has
  `DesktopEnvironments` for the same question.
- `packaging/test-daemon-e2e.sh` reads `/etc/os-release` with `sed` inside the
  end-to-end container; it is a test script, so decide whether it should call
  the binary's own reading instead and record the answer.

## What to build

- One trash writer used by both; one desktop-entry visibility rule used by
  both; a grep test for each so a copy cannot come back, as tickets 138 and
  146 added for their rules.

## Done when

- The copies are gone, the tests exist, and behaviour is unchanged where both
  copies agreed; where they differed, the ticket's notes say which behaviour
  was kept and why.

## Notes

### One trash writer

- `files_platform::trash` writes every trash record. It gained `Put::{Move,
  Copy}` and `put_in_trash`/`copy_to_trash` (the claim-the-name-then-place
  order is unchanged), `choose_trash`/`own_device_trash` (which trash an item
  goes to, as a `TrashTarget`), and `trash_path` for a caller with no job.
  `files-operations`' trash job now takes its choice from `choose_trash`
  instead of deciding it inline. `git-platform::patch::trash` is gone with
  its `trash_dir`, `percent_encode`, `copy_entry`, and `deletion_date`; a
  discard calls `files_platform::trash_path`.
- Where the two writers agreed, nothing changed: the `.trashinfo` written
  with `O_EXCL` before the item is placed and removed again when placing
  fails, `Path` percent-encoded from the bytes with `A-Za-z0-9-_.~/` left
  alone, and `DeletionDate` on the local wall clock.
- Where they differed, Better Files' behaviour is kept, each time because it
  is the FreeDesktop Trash specification's (version 1.0, "Trash
  directories" and "Contents of a trash directory") or the one the
  specification leaves to the implementation and Better Files already shows:
  - Per-volume trash: Better Git always used the home trash and fell back to
    copy-and-delete across devices. The specification gives an item on
    another mount its device's `$topdir/.Trash/$uid` or `$topdir/.Trash-$uid`,
    so a discard in a repository on a USB disk now goes to that disk's trash,
    and only a device with no usable trash gets the home copy.
  - Collision name: Better Git named the second copy `f.txt.2`; Better Files
    names it `f.1.txt`. The specification only asks for a unique name; the
    number before the extension keeps the restored file's type, and both
    trashes now look the same. `git-platform/tests/patch.rs`'s
    `discard_twice_keeps_both_copies` says `f.1.txt` now.
  - `$XDG_DATA_HOME`: Better Git took any non-empty value, Better Files only
    an absolute one, as the XDG Base Directory specification requires
    (a relative value is ignored).
  - A record in a volume trash holds the path relative to the top directory,
    which Better Git never wrote because it never used one.
  - A copy of a directory is now possible (`copy_tree`); Better Git's copy
    handled files and symbolic links only.
- `git-platform/tests/trash_local_time.rs` now discards a real change in a
  real repository rather than calling the deleted function.
- Grep test: `crates/files-platform/tests/one_trash_writer.rs` fails when
  any other crate's non-test source has a string with `[Trash Info]`,
  `.trashinfo`, or `DeletionDate=`.

### One desktop-entry visibility rule

- `app_catalog_core::DesktopEnvironments::shows(only_show_in, not_show_in)`
  is the rule; `ApplicationRecord::visibility_in` and `shows_entry` (for a
  raw entry, given a key's value) both call it. `monitor-gui`'s
  `startup::DesktopEntry::for_desktops` calls `shows_entry`.
- Where they agreed (case-insensitive names, an `OnlyShowIn` list needing one
  current desktop, `NotShowIn` excluding), nothing changed. Where they
  differed, the catalog's is kept: names are trimmed, and a list is split as
  the Desktop Entry specification splits one ("Possible value types": `\;` is
  a literal semicolon), where Better Monitor split on every `;`.
- Grep test: `crates/app-catalog-core/tests/one_visibility_rule.rs` fails when
  any other crate's non-test source names `OnlyShowIn` or `NotShowIn` in a
  string.

### `packaging/test-daemon-e2e.sh`

- Decision: the script reads no os-release. Calling our own reading is
  possible there (the e2e client is in the image before the script runs, and
  could be given a subcommand), but it is the wrong oracle: the daemon under
  test resolves the host with `better_core::host` and refuses a plan for
  another release, so a test taking its release from the same rule would
  agree with the daemon by construction. The release is the one the image was
  built for, the Dockerfile's `RELEASE_TARGET`, which CI always passes; the
  `sed` of `VERSION_ID` (wrong on a derivative such as Zorin, which sets its
  own number) was only a fallback for running the script by hand without that
  argument, and the script already built package names from
  `RELEASE_TARGET` unconditionally. It now refuses to run without it.
- Grep test: `crates/better-core/tests/os_release_readers.rs`'s
  `no_packaging_script_reads_os_release` fails when a script under
  `packaging/` names os-release outside a comment. `install.sh` keeps its
  copy by design, as before.

### After review (2026-11-21)

- No home trash and no free name are failures of their own now
  (`TrashError::NoHomeTrash`, `TrashError::NoFreeName`), and a discard's
  trash failure reaches Better Git as `GitError::Trash { reason, detail }`:
  the window says a sentence per reason in English and zh-TW ("Nothing was
  discarded: …") with the system's words as the detail
  (`git-gui` `every_git_failure_is_said_in_the_windows_language_…`).
- A move from another drive falls back to the home copy only when the
  drive's trash itself cannot be used (its folders, its record, a free name,
  or a full or read-only trash). An item that cannot be read or moved fails
  as itself. When the original cannot be deleted after the home copy, the
  copy and its record are removed again, so nothing says it was trashed
  (`a_move_whose_original_cannot_be_deleted_leaves_no_copy_or_record_behind`).
- A copy refuses anything that is not a regular file, a folder, or a link,
  before anything is written (`TrashError::NotCopyable`; a named pipe would
  block `fs::copy` for ever), and the cleanup of a failed copy makes each
  folder it wrote writable again first, so a 0555 folder no longer leaves an
  orphan under `files/` with no record.
- Two copy-into-trash functions remain, on purpose: `copy_to_trash`
  (`copy_tree`) for a caller with no job, and `files-operations`'
  `copy_into_trash`, which copies through the job engine (progress,
  cancellation, hard links kept, verification before the original goes) and
  then trashes the staged copy through `move_to_trash`. The job engine
  depends on files-platform, so files-platform cannot call it. Either way
  the record is written by `put_in_trash`.
  `one_trash_writer.rs`' `only_the_two_known_functions_copy_into_a_trash`
  fails on a third.
- Submodules: a scratch repository with a submodule (built inside the test,
  never one of the owner's) shows the submodule as a change row of the
  parent, so Better Git offered Discard on it, and that copied the whole
  submodule worktree into the trash and reset its commit. Discard (whole
  files and lines) now refuses a submodule before anything is changed, with
  a sentence that says to open it as its own repository
  (`git-platform/tests/patch.rs`'
  `discard_refuses_a_submodule_and_changes_nothing`). GitHub Desktop's own
  behaviour was not checked; this is the refusal the review allowed.
- vfat/exFAT: when `mkdir .Trash-$uid` works and `chmod` says EPERM or
  EOPNOTSUPP (a filesystem that keeps no mode), the directory is used when
  it is owned by this user, which is what every later trashing does with the
  same directory, so the first item no longer goes home while the rest stay
  on the drive. Any other `chmod` failure removes the directory it just made
  before falling back. Tested with the device-number seam and an injected
  `chmod` error (`a_drive_that_holds_no_mode_keeps_every_item_in_its_own_trash`,
  `a_drive_trash_that_cannot_be_made_private_is_not_left_behind`); no real
  FAT drive was used.
- `TryExec`: Better Monitor's autostart list resolved it with its own PATH
  search; it now asks `app_catalog_core::try_exec_found` with the catalog's
  `HostProbe`. The rules agreed for absolute paths, names on PATH, and
  relative paths with a slash (refused); `HostProbe` also ignores relative
  PATH entries, which Better Monitor searched. `one_visibility_rule.rs`'
  `no_crate_but_the_catalog_reads_try_exec` fails on another reader, with
  `files-preview/src/thumbnails.rs` the one recorded exception: it parses a
  thumbnailer file's `TryExec` and resolves nothing (so a thumbnailer whose
  program is missing is not filtered out, which is a separate defect).
