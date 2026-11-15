# Ticket 123 — Better Git: wrapped diffs, resizable panes, renames shown as renames, no text running out

**Blocked by:** none · **Status:** open

## Why

The owner reported on 2026-11-15:

- Code in the diff view does not wrap; long lines run off to the right.
- The sync button (fetch, pull, push, publish) has no menu: when there is
  something to push, the only choice is Push. GitHub Desktop's button has a
  drop-down arrow offering the other actions.
- The left and right columns (changes or history list on the left, diff on
  the right) cannot be resized.
- Asked whether `git mv` is recognised. Status parsing knows renames
  (`crates/git-core/src/status.rs` reads porcelain v2 rename entries;
  `log.rs` maps `R` to `ChangeKind::Renamed`), but how a rename is shown in
  the changes list, the diff and history has not been checked.
- In the Graph tab, a long commit subject runs over the author, date and sha
  columns instead of being cut with an ellipsis (screenshot: a row whose
  branch label "engineering-layout" pushes "docs: carry the d…" and the
  columns to the right out of line, and the sha cut at the window edge).

## What to build

- Diff view: a "Wrap lines" option (View menu and a toggle in the diff
  header), on by default, remembered; wrapped lines keep their gutter
  (line numbers, +/- marks) on the first visual line and an indent marker on
  continuations; selection, line staging, copy and search keep working with
  wrapping on; very long lines (minified files) stay fast (wrap lazily, cap
  layout per line).
- Resizable split: the edge between the left list and the diff can be
  dragged, has a minimum for each side, is remembered per window kind,
  resets on double click, and is reachable by keyboard, following the same
  pattern as Better Files' pane edges (ticket 118).
- Renames: a staged `git mv` (and a rename git detects in a commit) shows as
  one row "old → new" with a rename icon, its diff shows only the content
  change (or "renamed, no content change"), staging and discarding treat both
  paths together, and history and the graph's commit details show it the
  same way. An unstaged move (deleted + untracked) is shown as git shows it,
  with no guessing, but staging both sides turns it into a rename row, as
  GitHub Desktop does. Real-git fixture tests for each.
- Graph and history rows: the subject takes the remaining width and ends in
  an ellipsis; ref labels shrink before the subject disappears entirely; the
  author, date and sha columns keep fixed widths and never move; the full
  subject is in a tooltip. Check every list in Better Git for the same
  overflow.

- Sync button: a split button as in GitHub Desktop. The main part does the
  action that fits the state (Publish, Push, Pull, Fetch). The arrow opens a
  menu with every action that applies now: Fetch always; Pull when behind;
  Push when ahead; Force push only when the branch diverged and only with
  its own confirmation; Publish when the branch has no upstream. Each menu
  item names what it does and how many commits it moves; unavailable items
  say why. Keyboard reachable; the whole row is clickable.

## Acceptance

- [x] Tests: the sync menu's items for each branch state (up to date, ahead,
      behind, diverged, no upstream, detached, no remote).
- [x] Tests: wrapping layout math, rename rows from real git (staged mv,
      mv with edits, committed rename, unstaged move), row column widths.
- [x] Screenshots in a headless sandbox: a wrapped long line, the split
      dragged both ways, a rename in changes and in history, a long subject in
      the graph with a branch label; both languages.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
