# Ticket 116 — Better Git's toolbar has a repository dropdown and a separate worktree dropdown

**Blocked by:** 113 (merged) · **Status:** built, not merged

## Why

After ticket 113 the repository dropdown groups worktrees under their
repository, but the toolbar's "Current repository" cell still shows the open
worktree's folder name (for example `ticket-101`) where the repository's name
(`better-os`) belongs. The owner decided on 2026-11-14: the cell shows the
repository's name, and the worktree's name is shown separately.

## Owner decision, later on 2026-11-14

Worktrees are chosen in their own dropdown, as in GitHub Desktop, and are not
listed inside the repository dropdown. This replaces ticket 113's grouped
rows in the repository dropdown; ticket 113's grouping by git common
directory, its merging of saved entries and its background reads stay.

## What to build

- The repository dropdown lists repositories only, one row each (alias or the
  main worktree's folder name, path underneath). Choosing one opens the
  worktree of it that was open last, or its main worktree.
- "Current repository" shows the repository's name.
- A new toolbar cell "Current worktree" sits between the repository and the
  branch cells. It shows the open worktree's folder name and opens a dropdown
  of that repository's worktrees: folder name, branch or "Detached at <sha>"
  underneath, the main one marked, the current one marked, a filter box, the
  whole row clickable, missing ones shown as missing and not clickable, a
  tooltip with the full path. The existing worktree actions (add, remove,
  open in file manager, whatever the worktrees view already offers) are
  reachable from this dropdown.
- A repository with only its main worktree still shows the cell, so adding a
  worktree is one click away; decide by DESIGN.md whether it is quieter then.
- The toolbar fits the minimum window width: cells shrink with ellipsis and
  nothing is pushed out; the shortcut buttons stay visible (ticket 103).
- The window title and other places that name the open repository use the
  repository's name, with the worktree's name added when it is not the main
  one.
- Keyboard: the shortcut that opened the repository list still does; add one
  for the worktree list if a free, conventional one exists.
- English and Traditional Chinese (Taiwan usage).

## Acceptance

- [x] Unit tests: main worktree, linked worktree, alias, bare or missing main
      worktree; choosing a repository reopens its last worktree; the
      repository dropdown holds no worktree rows.
- [x] Screenshots in a headless sandbox with a fixture repository: both dropdowns open, main
      worktree and linked worktree, both languages, normal and minimum width.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **The repository dropdown lists repositories only.** `RepositoryList::groups`
  still keys by the common directory and merges saved entries (ticket 113),
  but a `Group` no longer carries worktree rows; it keeps `folders` (every
  folder that is the repository) for ranking and removal. Its filter matches
  the repository's name, its folder, or one of its worktrees' folder names.
  The row opens the worktree of it that was open last and is still there,
  else the main worktree, else the first worktree that is there.
- **One name everywhere.** `GitSession::open_names` takes the repository's
  name from its row in the list (alias, else the main worktree's folder; a
  repository without one is named after its common directory, a plain
  `.git` after the folder it was in) and the worktree's from its folder.
  The toolbar, the window title the desktop shows (`better-os · ticket-101 —
  Better Git`, just `better-os — Better Git` on the main worktree), the
  remove-repository dialog, and the folder suggested for a new worktree or a
  published repository (the main worktree's folder, never the alias) use it.
  Nothing here touches the disk; a linked worktree of a `--separate-git-dir`
  repository borrows its main worktree from what the list's read learned.
- **"Current worktree"** sits between the repository and the branch. Its
  dropdown (`repositories::worktree_list`) lists the open repository's
  worktrees: folder name, branch or "Detached at <sha>" beneath, "Main
  worktree" and missing ones marked, the open one selected, the full path in
  the tooltip, a filter on name, branch, and folder, the whole row clickable
  (a missing one is not). "Add worktree…" is in its header; each row's More
  button and its context menu offer Open, Show in Better Files, Copy folder
  path, Move worktree, and Remove worktree. The old Worktrees dialog is gone:
  the dropdown has everything it had. With only the main worktree the cell's
  value is in secondary text (DESIGN.md: hierarchy comes from the ramp and
  secondary color) and the dropdown says how a worktree helps.
- **Keyboard.** Ctrl+T still opens the repository list; Ctrl+Alt+W, which
  opened the Worktrees dialog, now opens the worktree dropdown, as does the
  Repository menu's "Worktrees…".
- **Minimum width.** The three cells start at the width their text asks for
  and shrink to 112px with an ellipsis, the sync button to 152px; once the
  shortcut buttons are icons they keep their width, so nothing leaves the
  window at 960px.

Tests: four new and five changed in `crates/git-gui/src/repositories.rs`
(the dropdown holds no worktree rows, reopening the last worktree, names
with an alias and without a main worktree, the two searches) and two new in
`crates/git-gui/tests/session.rs` (names and title on the main and a linked
worktree, with an alias, choosing the repository again; a bare repository's
worktree). Screenshots in a headless GNOME Shell with fixture repositories,
both dropdowns, main and linked worktree, both languages, 1280px and 960px.


## Review changes

- **The open worktree is not moved or removed from its own window**, nor the
  main one; a locked one shows Move and Remove off ("unlock it first"), and a
  missing one offers only Copy folder path (`WorktreeRow::actions`). The
  remove confirmation names the folder and says how many changed and
  untracked files it holds, read in the background (`Op::WorktreeChanges`).
  A row that is gone gets no menu.
- **Keyboard.** Opening any of the three dropdowns empties and focuses its
  filter (`Flyout::initial_focus`, focused once as it opens); Up and Down
  walk a highlighted row (the focus visual), Enter opens it, Escape closes,
  Tab reaches each row's More button and Add. The More button is a
  `DropDownButton`, so its menu hangs from it whether a click or a key opened
  it. Focus left on a closed dropdown's controls, or on a closed context
  menu, comes back to the window or the dropdown's filter.
- **Locked worktrees** are marked "Locked".
- **Minimum width.** Captions are never cut: an item keeps its caption whole
  and only the value shrinks, and when the captions cannot all fit (a checks
  item at 960px) they are hidden; the checks item clips and its re-run button
  becomes an icon. A full-width button's label ends in an ellipsis, and the
  commit button's tooltip has it whole.
- The worktree filter matches folder names and branches only; a bare
  repository with one linked worktree is not "only the main worktree"; every
  write of the recent list starts from the file, which is also re-read when
  the repository dropdown opens; the names are worked out once per frame.

- **Locked worktrees, and ones that are gone.** Git's lock reason is read
  (`Worktree::lock_reason`) and shown ("Locked: on a USB stick"). A linked
  worktree's menu offers "Lock worktree…" (reason optional) and, once
  locked, "Unlock worktree"; Move and Remove stay off until it is unlocked,
  and Remove never passes `--force` twice. One whose folder is gone offers
  "Clear from the list…", which runs `git worktree prune`: Git cannot narrow
  it to one worktree, so the confirmation names every worktree it would
  clear when there is more than one, and says no files are deleted; a locked
  one is not cleared. The new commands go through `git-platform`'s
  `lock_worktree`, `unlock_worktree`, and `prune_worktrees`.
