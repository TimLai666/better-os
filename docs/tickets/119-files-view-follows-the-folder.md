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

Added by the owner on 2026-11-14:

- The Reload button in the navigation row draws Redo's single curved arrow.
  It gets a real refresh glyph (two arrows round a circle, as Windows 11 File
  Explorer's Refresh); every other app's plain reload that reads as Redo gets
  the same.
- The File operations panel's finished list cannot be cleared and grows
  without limit: `SessionHistory` only pushes, the engine never forgets a
  finished job (`jobs()` returns every job ever run, and `refresh_jobs`
  re-records all of them every frame), and a finished job was drawn twice
  (as a card with a progress bar and again under "Finished this session").
  A finished job appears once, in the finished list; each row can be
  dismissed and "Clear" removes them all; the list keeps a bounded number;
  the engine lets go of finished jobs; a failed job stays until dismissed
  and says what failed; nothing new is kept across restarts.
- The File operations panel is too wide (nearly half of a narrow window). Its
  default width is modest, never more than about a third of the window, and
  its edge can be dragged, remembered and reset by a double-click like ticket
  118's sidebar divider, with the code shared.

## Acceptance

- [x] Integration tests in a temp dir: a file created, removed, renamed and
      modified by another process shows up in an open pane without a reload;
      trash, delete, move, copy, rename and undo by Better Files update every
      pane showing the folder; selection and scroll survive; a burst of 10,000
      files settles without re-listing per file; an unmount is not blocked.
- [x] Sandbox evidence: the owner's case (select two files, move to trash,
      they leave the view at once), with screenshots before and after and of
      the jobs button idle and busy.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **Every pane watches its folder.** The session keeps one watch per pane
  (`FolderWatch` in `session.rs`), made on the first frame a pane shows a
  folder and replaced when it moves; a closed tab's goes with it. Row events
  are applied as rows: the last event per path wins, removals leave in one
  pass (`DirectoryModel::remove_many`), and changed or new rows are built by
  the listing's own code (`files_platform::read_entry`, used by
  `watch::refresh_with`), so a file that arrives by an event has the same
  type and icon as a listed one. Lost events, or more than 256 in one frame
  (a burst), re-read the folder once instead.
- **Re-reading keeps the view.** `Pane::refresh` reads the folder beside the
  list on screen and `DirectoryModel::reconcile` applies only what differs:
  selection, cursor and scroll stay, an entry that is gone leaves the
  selection, and requests made while a reading runs become one more reading.
  The Refresh button still starts over.
- **Folders without events.** A network or FUSE-daemon folder
  (`MountTable::changes_may_go_unannounced`: not `fuseblk`), the Trash and
  Recents are re-read every five seconds while visible, besides the watch.
- **A finished job updates every pane at once.** The first frame a job is
  seen finished, every pane (all tabs, both sides of a split) is refreshed
  in place, without waiting for a watch; undo and redo do the same. Which
  folders a job touched is not worked out: reading an unchanged folder
  changes nothing on screen.
- **Unmounting.** inotify does not hold a filesystem busy (the kernel drops
  the watch on unmount), and Eject now lets go of every watch under the
  device before it asks the storage layer anyway.
- **The jobs button.** A new glyph (two arrows passing) replaces the
  spinner-shaped `LoaderCircle`; while jobs run, their number is on its
  corner (a badge the kit's command bar now offers, `Command::badge`, with
  `Command::icon_path` for a window's own glyph). No other app used
  `LoaderCircle`; Better Monitor's `Loader` icons are for fans and Better
  Git's for a pending check, where a spinner shape fits.
- **Refresh glyph, shared.** `better_ui::fluent::glyphs::REFRESH` (Lucide's
  refresh-cw) is served by each window's assets. It replaces Redo on Better
  Files' Reload, Better Monitor's two Refresh commands, Better Awake's
  Refresh button and Better Git's Fetch origin. Better Manager has no
  reload drawn with Redo.
- **Finished jobs.** A finished job is shown once, under "Finished this
  session", with a Dismiss button; "Clear" removes them all. The list keeps
  the newest 50 that did not fail (more than a screen of rows at any window
  height), and every failed one until it is dismissed, with what failed and
  Retry. The engine keeps at most 64 finished jobs in memory
  (`FINISHED_KEPT`, completed ones let go of before failed ones, failed ones
  last because Retry needs them) and forgets one when it is dismissed;
  `refresh_jobs` files each job once and its bookkeeping is pruned to what
  the engine lists, so its work no longer grows with the session's past.
  Nothing on disk changed: interrupted-job recovery reads the store as before.
- **Panel width.** 320 px by default, never under 240 or over a third of the
  window as the person sees it (the frame's shadow excluded), so 293 px at
  the 880 px minimum. Its left edge is an 8 px grab strip with a resize
  pointer and an accent divider while hovered or dragged; double-click goes
  back to the default; the width is stored in Better Files' preferences
  (`operations_width`) when the drag ends. The policy (`layout::PaneEdge`)
  and the strip (`pane_edge`, `follow_edge` in `render.rs`) are written for
  ticket 118's sidebar to use as well.

Tests: `files-gui` `tests.rs` (a change by another program; selection; a job
updates both tabs; copy, rename, move and delete; undo; a 10,000-file burst
read at most four times; dismiss and clear; the bounds), `layout.rs`
(clamping, stored and unreadable widths, reset), `files-core` `model.rs`
(reconcile, `remove_many`), `files-operations` `engine.rs` (1,000 finished
jobs leave 64; a failed job outlasts completed ones; forget),
`files-platform` `mounts.rs`, `better-ui` `glyphs.rs`.

Sandbox (headless GNOME Shell 46, private bus, fake `tailscale`), in
`.shots/`: `t119-trash-before.png` and `t119-trash-after.png` (two files
selected, Delete, gone 0.8 s later with the job filed once),
`t119-watch-appeared.png` (a file written by another process),
`t119-jobs-busy-zh.png` and `t119-jobs-done-zh.png` (the badge while a copy
waits on a decision, then gone), `t119-panel-min-width.png` (293 px at the
880 px window), `t119-panel-dragging.png`, `t119-panel-restart-keeps-width.png`,
`t119-panel-reset.png`.

Found and not changed here: at about 1,190 px wide the command bar's last
buttons (the jobs button and "…") run past the window's right edge instead
of moving into the overflow menu.

## After review

An independent review of the first version found these, all fixed:

- **One watch for the process.** `files_platform::WatchHub` holds the only
  inotify instance (a tab each would exhaust the kernel's 128 per user) and
  one thread that makes and drops watches, reads the mount table (fresh for
  each folder, so a share mounted later is polled), reads `.hidden` (again
  when it changes), and `lstat`s changed rows. Nothing that can stall on a
  network filesystem runs on the window's thread. A network or FUSE-daemon
  folder gets no inotify watch and is read again every 5 s while visible,
  30 s in the background, and at once when its tab is switched to.
- **No stale reading wins.** Rows are applied only to a settled list; a
  change that arrives while the folder is being listed or read again asks
  for one more reading instead, so a reading taken before the change cannot
  undo it, and a removal cannot come before the batch carrying the file.
- **No gap after a navigation.** The watch is asked for in the same frame
  as the listing, and its arrival reads the folder once more, so a change
  between the listing and the watch is not lost (one extra reading per
  folder opened, on the reader's thread).
- **Lost events.** notify 7 reports a kernel queue overflow as an event
  flagged `Rescan`, not an error; it now reads the folder again.
- **The folder itself deleted or moved.** The view goes to the nearest
  folder above it that still exists and says “"X" was moved or deleted, so
  this is the folder it was in.”
- **Bursts.** The hub gathers 30 ms of events and keeps the last word per
  path, so a file being written is one row per batch; more than 4,096
  distinct paths read the folder again.
- **Reconcile is linear.** Changed rows leave the order in one pass; 20,000
  changed rows reconcile in well under a second in a debug build.
- **Eject.** Every window's watches under the device are let go of before
  the eject is asked for, and asked for again when it fails.
- **Finished jobs and windows.** The engine counts the windows that read it;
  a finished job is let go of only when every window has filed it, so one
  window's Clear cannot cost another its undo entry. Retry on a job the
  engine no longer keeps says why.
- **Smaller things.** A double-click reset stores no width (so the default
  returns once the window is wide enough); the divider stops being lit when
  its drag ends; a window that is reading a folder again polls at the busy
  rate; a command bar command that moved behind "…" keeps its own glyph
  (Better Monitor's Refresh); the badge wrapper is drawn only for a command
  with a badge, so every other command bar icon is laid out as before.

Tests added for each: the hub (`watch.rs`: rows, `.hidden`, suspend and
resume, the folder gone, a `Rescan` event, one row for many writes), a held
reading finished after a change (fails without the fix), changes in the gap
after a navigation (fails without the fix), rows kept while a folder is read
again, a folder deleted and one renamed, eject through `eject_device` with a
failed eject, reset at 880 px, two windows and Clear, Retry on a forgotten
job, and 20,000 changed rows. A real kernel queue overflow was not produced;
the test feeds the event notify makes of one.

Screenshots: `t119-review-folder-gone.png`, `t119-review-monitor-bar.png`
(Better Monitor's command bar with the refresh glyph, icons aligned), and
`t119-review-git-bar.png` (Better Git's toolbar unchanged).
