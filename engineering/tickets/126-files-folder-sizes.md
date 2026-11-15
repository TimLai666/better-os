# Ticket 126 — Better Files shows how big a folder is

**Blocked by:** none

## Why

The owner asked on 2026-11-15 for folders to show their size in Better Files,
as they do for files. Today a folder's Size column is empty (Windows 11 File
Explorer does the same; macOS Finder offers "Calculate all sizes").

## What to build

- Every view that shows a file's size (details/list Size column, the
  Properties panel, the status bar for a selection) shows a folder's total
  size too: the sum of the files beneath it, hard links counted once, without
  following symbolic links or crossing into another filesystem.
- Sizes are worked out off the UI thread, folder by folder, after the listing
  is drawn; a folder whose size is still being worked out says so ("計算中…" /
  "Calculating…") rather than showing nothing or a guess. Sorting by size puts
  such folders last and re-sorts as sizes arrive without moving the selection.
- The work is cancelled when the person leaves the folder, never slows the
  listing itself, and is bounded (a per-folder deadline and a total budget; a
  folder that hits it shows "超過 N 個項目" / "More than N items" rather than a
  wrong number). Results are cached and invalidated by the folder watcher.
- Network shares, FUSE mounts, and the Trash are not walked by default; they
  show "—" with the reason in a tooltip. A setting in Better Files' Settings
  turns folder sizes off, and one turns them on for network locations.
- A folder the person cannot read shows "無法讀取" / "Can't read", not 0.

## Done when

- Tests for the sum (hard links, symlinks, other filesystems, unreadable
  entries), cancellation, the budget, cache invalidation, and sorting.
- A benchmark entry in `engineering/performance/` for a 100 000-file tree
  (time to all sizes, CPU, and that the listing's first frame is unchanged).
- Screens in zh-TW and English, light and dark, at 1600 and 960.
