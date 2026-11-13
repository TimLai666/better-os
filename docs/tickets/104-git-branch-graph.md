# Ticket 104 — Better Git shows the branch graph

**Blocked by:** 92 (done) · **Status:** ready

## Why

The owner asked on 2026-11-14 for a third view next to "Changes" and
"History": the graph of every branch, with the coloured lines that show
where branches split off and merge, as `git log --graph`, VS Code's Git
Graph, or Fork draw it. GitHub Desktop has no such view; Better Git's
History shows one branch as a flat list.

## What to build

- **A third tab, "Graph" ("分支圖").** The left pane's `SelectorBar` gets
  Changes, History, Graph. Graph uses the whole width of the window: the
  graph and commit list on the left, the selected commit's details and diff
  on the right, as History does.
- **Lanes computed without I/O.** `git-core` gains a lane layout: from
  commits in topological order with their parents (read by `git-platform`
  with `git log --all --topo-order -z` in an explicit format: hash, parents,
  author, date, subject, refs), it assigns each commit a column and each
  edge a path, keeping a branch in the same column while it lives, reusing
  freed columns, and giving each lane a stable colour from the Fluent chart
  palette. Merge and fork edges are drawn as curves. Unit tests cover a
  straight line, a fork, a merge, an octopus merge, a branch that ends,
  criss-cross merges, and a root commit in the middle (unrelated
  histories).
- **Labels.** Each commit row shows its branch, remote branch, tag, and HEAD
  labels as chips; the current branch is marked. A filter chooses all
  branches, local only, or the current branch only, and whether remote
  branches and tags are shown.
- **The same commit actions as History.** Selecting a commit shows its
  files and diff; the right-click menu offers what History offers (checkout,
  create branch or tag here, cherry-pick, revert, reset, copy hash), and
  dragging works the same way.
- **Large repositories.** The list is virtualized and loads in pages of a few
  thousand commits as the person scrolls, with the lane state carried from
  one page to the next, so a repository with 100,000 commits opens without
  stalling the window; the work runs off the UI thread. Refreshes after a
  fetch or commit keep the scroll position and the selection.
- **Search.** Ctrl+F in the graph finds commits by subject, author, or hash
  prefix and scrolls to them.

## Acceptance

- [ ] Screenshots of the Graph tab on a repository with several branches,
      merges, tags, and a remote, in light and dark, English and zh-TW.
- [ ] Lane layout unit tests for every case listed above.
- [ ] Opening the graph of a generated 100,000-commit repository: the first
      rows show in under one second and scrolling does not stall; numbers
      stated.
- [ ] Tests use temporary repositories only.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
