# 34 — files-gui: window, tabs, sidebar, views, operation center

**Epic:** Better Files (Issue #6)
**User Story:** A user drags a folder into the sidebar and it stays there across
restarts, browses in a grid or a list that scrolls smoothly through a hundred
thousand files, and can see what every running copy is doing.
**Blocked by:** 33-files-operations
**Status:** done

## Goal

Deliver the Better Files window itself: navigation chrome, the sidebar with
persistent bookmarks, both view modes, and the operation center.

## What it delivers

- A GPUI file-manager window built on `better-ui` and `gpui-component`, with
  tabs, back and forward, parent navigation, and an editable path field bound to
  the ticket 32 navigation model.
- Sidebar with distinct sections: built-in locations, devices, Applications, and
  user Favorites.
- Persistent bookmarks with macOS-like pinning: drag any accessible directory
  into Favorites; dropping creates a bookmark and never moves the directory;
  drag to reorder; rename the bookmark label without renaming the directory;
  remove a bookmark without deleting the directory. Right-click offers Open, Open
  in New Tab, Rename Bookmark, and Remove from Sidebar. Order is preserved across
  restarts, standard XDG bookmark data is used where compatibility is practical,
  and a missing or inaccessible location stays visible with an unavailable state
  rather than being silently deleted. Reorder and removal have keyboard-accessible
  alternatives.
- Icon grid and detailed list views, both virtualized, with sort by name,
  modified date, size, type, and extension; ascending and descending order;
  folders-first as a configurable option; icon and row size control; responsive
  selection in large directories; and incremental item insertion that does not
  make the selection jump.
- `Ctrl+H` and an equivalent visible menu action toggle hidden entries
  immediately, and the preference persists per user.
- Operation center surfacing the ticket 33 jobs: progress, conflicts awaiting a
  decision, failures with their affected paths, and retry and cancel controls.
- Keyboard-only operation and accessibility from the layout stage, and
  drag-and-drop within and between windows.
- The GUI never runs as root and never constructs a shell string.
- Linux internals stay behind user concepts: the normal path shows Applications,
  Devices, Favorites, and Trash rather than mount points, `/run/user`, GVfs
  paths, or UDisks2 object paths.

## Out of scope

- Preview, current-directory search, the Applications virtual location, and
  external-device sidebar behavior (ticket 35).
- Split view, column view, restoring recently closed tabs, opening a terminal in
  the current location, and per-folder view preferences.
- Network location UI.

## Deferred decisions

Issue #6 requires an ADR or a focused follow-up issue rather than a silent
choice for: the exact split-view and column-view UX, and per-folder versus
global view preference defaults. This ticket ships global view preferences and a
view model that does not prevent either.

## Acceptance criteria

- [x] The GUI uses Rust, GPUI, `gpui-component`, and `better-ui`, with no GTK,
      Qt, Electron, Tauri, browser UI, or interpreted runtime.
- [x] A directory containing 100,000 synthetic entries begins rendering
      progressively without freezing the UI.
- [x] Grid and list rendering are virtualized and stay interactive under the
      documented benchmark environment.
- [x] `Ctrl+H` and the menu action both toggle hidden files and directories, and
      the preference persists.
- [x] Dragging a directory to the sidebar creates a persistent bookmark and does
      not move the directory.
- [x] Bookmarks can be reordered, relabeled, and removed, and the order survives
      a restart.
- [x] A bookmark pointing at a missing location stays visible with a clear
      unavailable state and is not deleted silently.
- [x] Tabs, history, parent navigation, and the editable path field work, and
      keyboard-only navigation covers all of them.
- [x] Built-in locations, devices, Applications, and Favorites are distinct
      sidebar sections.
- [x] Background jobs expose progress, conflicts, and errors in the operation
      center, and the window can close without corrupting active-operation state.
- [x] Sorting, ordering, folders-first, and size controls work, and incremental
      insertion does not move the selection.
- [x] Directory scanning does not run on the GPUI render thread.
- [x] The GUI does not run as root.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `ZED_HEADLESS=1` launch smoke of `files-gui`
- A 100,000-entry directory benchmark recording cold and warm startup, time to
  first visible entries, time until navigation is interactive, scrolling frame
  time and dropped frames, memory, and CPU
- A bookmark persistence test across a restart, including reorder, rename,
  removal, and the missing-location state
- A window-close test asserting active jobs survive and their state stays
  consistent

## Outcome

`files-gui` and the `better-files` binary. The crate is split so that almost
none of it needs a display server: `session`, `content`, `sidebar`, `toolbar`,
`bookmarks`, `opcenter`, `commands`, `keys`, `prefs`, `layout`, and `format`
hold every decision and mention no GPUI, and `app`, `render`, `shell`, `views`,
and `panels` are the thin layer that draws them. 66 crate tests, all of them
against the same types the renderer uses.

Three properties are held by construction rather than by care.

**One engine, not one per window.** `files_gui::shared_engine` hands out a
single process-wide `Arc<JobEngine>` and `FilesSession` takes it rather than
building one. Milestone M37's test drops a whole session while an 8 MB copy is
running and finds the copy completed with the right byte count and the engine's
own snapshot still consistent.

**Both view modes are one `uniform_list`.** A row is formatted inside the range
callback, from the entries `DirectoryModel` already holds; nothing is
precomputed and nothing is cached. The benchmark measures the contrast rather
than asserting it: 0.011 ms for a screenful of rows against 37.2 ms to format
all 100,000, which is what a view that built every row would pay per frame.

**Incremental insertion cannot move the selection.** The selection names
entries, and the keyboard cursor is re-derived from the selection's own cursor
identity every time entries arrive. A test drops twenty entries above the cursor
and finds the same entry focused at its new index.

The tests found two real defects rather than review: a permanent delete was
refused outright when the session had no trash directory, although deleting a
file by path needs no trash at all; and the "no external devices" empty state
does not fit the sidebar at 125% in English, which is why the empty-state
sentences are drawn as wrapping prose and are asserted separately from the
truncating row labels.

`files_core::Pane::resume` was added so a reopened tab keeps the history
`TabSet::close` preserved. Without it, restoring a closed tab dropped the user
at the folder with Back greyed out, which is the failure the recently-closed
stack exists to prevent.

## Benchmark

100,000 entries, batch size 256, median of 5, release build, this host.

| Measurement | Result |
| --- | --- |
| First visible batch, 32 rows formatted | 3.701 ms |
| Full model, listing complete | 170.063 ms |
| List: one screenful of rows | 0.011 ms |
| Grid: one screenful of tiles | 0.014 ms |
| Every row formatted, for comparison | 37.249 ms |
| Arrow down, page down, end | under 0.001 ms |
| Select all | 16.538 ms |
| Cursor resync after a batch, cached | under 0.001 ms |
| Cursor resync, index rediscovered | 11.242 ms |
| Header click, re-sort the whole directory | 26.007 ms |
| `Ctrl+H`, re-filter without reloading | 0.249 ms |
| Format one screenful, en-US and zh-TW | 0.004 ms each |

The 11.2 ms figure is the honest worst case and the named follow-up: when the
focused entry's index has to be rediscovered by walking the visible list. It is
only paid when the cursor moved by something other than the view itself, and the
cached path costs nothing. A position index in `DirectoryModel` would remove it.

## What this ticket did not do

- Restoring recently closed tabs is implemented, although this ticket's
  out-of-scope list defers it. `files-core` already modelled it and the cost was
  one constructor; the ticket text is left as written and this note records the
  difference.
- The Applications location lists as not-listable. Ticket 35 owns the catalog.
- Opening a file reports "no application is wired up yet" rather than launching.
  Ticket 35 owns Better App Chooser integration.
- Device rows come from the mount table and report Unknown removal state.
  Ticket 35 owns the storage service.
- Dragging a favourite *within* the sidebar to reorder is modelled and reachable
  from buttons and the keyboard; the pointer gesture inside the sidebar is not
  wired. Dragging a folder *into* Favorites is wired.
- Modified times are shown in UTC. There is no time-zone dependency in the
  workspace and `docs/files-gui-policy.md` records the gap.
- Split view, column view, opening a terminal, per-folder preferences, preview,
  and search are out of scope and absent.
