# Ticket 119 — Better Files' view follows the folder, and the jobs button stops looking like a stuck spinner

**Blocked by:** none · **Status:** open

## Why

On 2026-11-14 the owner moved two files to the trash in Better Files. The
File operations panel said "Moved to trash, 2 / 2, done", but both files
stayed in the folder view. The owner also read the toolbar button that opens
that panel as a progress spinner that was not spinning.

Causes found in the code:

- The view is never told the folder changed. `crates/files-platform/src/watch.rs`
  has a directory watcher that turns kernel events into single-row refreshes
  (issue #6 requires it), with tests, but nothing in `crates/files-gui`
  creates one: the only watcher the window starts is the application
  catalog's (`app.rs` ~1059). Finishing a job does not reload the pane either
  (`session.rs` `refresh_jobs`, ~1929, only files the job into history). So a
  folder is listed once and stays as it was until the person reloads, for
  Better Files' own operations and for changes made by any other program.
- The jobs panel button uses the `LoaderCircle` icon (`shell.rs` ~467), the
  shape of a spinner, as a static icon.

## What to build

- Every open pane (each tab, each window, the second pane if any) watches
  its folder with the existing watcher and applies its row events: files
  appearing, disappearing, renamed, changed size or time; a dropped-events
  notice re-reads the folder. Selection, cursor and scroll position survive;
  an entry that disappeared leaves the selection. Watching stops when the
  pane leaves the folder or closes.
- Folders where events do not arrive (GVfs network mounts, some FUSE
  mounts, the trash, search results) are re-read when a job that touched them
  finishes and on a modest interval while visible, and the Refresh button
  always works.
- A finished job updates every pane showing a folder it touched at once,
  without waiting for the watcher, so the result of the person's own action
  is never late. Undo does the same.
- Watching costs nothing while idle and does not hold a removable drive busy
  (unmounting still works: the watch is dropped first).
- The jobs button gets an icon that reads as "file operations" and does not
  look like a spinner; while jobs run it shows progress (an animated ring or
  a count) and stops when they finish. Check the other toolbar and panel
  icons in Better Files, and the other apps, for a static spinner icon.
- English and Traditional Chinese where text changes.

## Acceptance

- [ ] Integration tests in a temp dir: a file created, removed, renamed and
      modified by another process shows up in an open pane without a reload;
      trash, delete, move, copy, rename and undo by Better Files update every
      pane showing the folder; selection and scroll survive; a burst of 10,000
      files settles without re-listing per file; an unmount is not blocked.
- [ ] Sandbox evidence: the owner's case (select two files, move to trash,
      they leave the view at once), with screenshots before and after and of
      the jobs button idle and busy.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
