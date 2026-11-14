# Ticket 116 — Better Git's toolbar names the repository, and the worktree separately

**Blocked by:** 113 (merged) · **Status:** open

## Why

After ticket 113 the repository dropdown groups worktrees under their
repository, but the toolbar's "Current repository" cell still shows the open
worktree's folder name (for example `ticket-101`) where the repository's name
(`better-os`) belongs. The owner decided on 2026-11-14: the cell shows the
repository's name, and the worktree's name is shown separately.

## What to build

- "Current repository" shows the repository's name: its alias if it has one,
  otherwise the main worktree's folder name (the same name the dropdown's
  group uses).
- When the open folder is a linked worktree, its folder name is shown
  separately and clearly as the worktree, in the same cell as secondary text
  or as its own small element beside it, following DESIGN.md; the main
  worktree shows nothing extra. It must fit the toolbar at the minimum window
  width without pushing other cells out, with a tooltip carrying the full
  path.
- The window title and any other place that names the open repository
  (recent list, dialogs that say "in <repository>") use the same rule
  consistently.
- English and Traditional Chinese (Taiwan usage).

## Acceptance

- [ ] Unit tests: main worktree, linked worktree, alias, bare or missing main
      worktree.
- [ ] Screenshots in a headless sandbox with a fixture repository: main
      worktree and linked worktree, both languages, normal and minimum width.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
