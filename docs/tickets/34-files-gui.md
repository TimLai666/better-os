# 34 — files-gui: window, tabs, sidebar, views, operation center

**Epic:** Better Files (Issue #6)
**User Story:** A user drags a folder into the sidebar and it stays there across
restarts, browses in a grid or a list that scrolls smoothly through a hundred
thousand files, and can see what every running copy is doing.
**Blocked by:** 33-files-operations
**Status:** todo

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

- [ ] The GUI uses Rust, GPUI, `gpui-component`, and `better-ui`, with no GTK,
      Qt, Electron, Tauri, browser UI, or interpreted runtime.
- [ ] A directory containing 100,000 synthetic entries begins rendering
      progressively without freezing the UI.
- [ ] Grid and list rendering are virtualized and stay interactive under the
      documented benchmark environment.
- [ ] `Ctrl+H` and the menu action both toggle hidden files and directories, and
      the preference persists.
- [ ] Dragging a directory to the sidebar creates a persistent bookmark and does
      not move the directory.
- [ ] Bookmarks can be reordered, relabeled, and removed, and the order survives
      a restart.
- [ ] A bookmark pointing at a missing location stays visible with a clear
      unavailable state and is not deleted silently.
- [ ] Tabs, history, parent navigation, and the editable path field work, and
      keyboard-only navigation covers all of them.
- [ ] Built-in locations, devices, Applications, and Favorites are distinct
      sidebar sections.
- [ ] Background jobs expose progress, conflicts, and errors in the operation
      center, and the window can close without corrupting active-operation state.
- [ ] Sorting, ordering, folders-first, and size controls work, and incremental
      insertion does not move the selection.
- [ ] Directory scanning does not run on the GPUI render thread.
- [ ] The GUI does not run as root.

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
