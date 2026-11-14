# Ticket 114 — The branch graph says what a line is when the pointer rests on it

**Blocked by:** 104 (merged) · **Status:** done, not merged

## Why

The owner asked on 2026-11-14: in Better Git's Graph tab, resting the pointer
on a line should say which line it is. Today the lanes are coloured lines
with no way to tell which branch a lane belongs to once its label has
scrolled away.

## What to build

- Resting the pointer on a lane segment, a merge or fork curve, or a commit
  dot shows a tooltip, and highlights that whole lane (other lanes dim) so it
  can be followed by eye.
- The tooltip for a line names the branch or branches whose history the lane
  carries: the nearest ref (local branch, remote branch or tag) that the lane
  leads up to; when no ref points into it, "leads to <short sha> <subject>".
  A merge or fork curve says "<from> merged into <to>" or "<branch> branched
  from <parent>". A dot shows the commit's short sha, subject, author and
  relative date.
- The hit area is wider than the drawn line (at least 8 px) so a 2 px line
  can be hit; hovering never starts a click or blocks clicking the row.
- English and Traditional Chinese (Taiwan usage).
- Lane-to-ref resolution is computed with the graph layout, off the UI
  thread, and stays correct with 60k commits (ticket 104's fixture).

## Acceptance

- [x] Unit tests on fixture graphs: linear, fork, merge, octopus merge, a lane
      with no ref, two refs on one lane, a lane that changes column.
- [x] Screenshots in a headless sandbox of a line tooltip, a merge curve
      tooltip and a dot tooltip, with the lane highlighted, in both languages.
- [x] Scrolling the 60k-commit fixture stays as smooth as before (same
      measurement as ticket 104).
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **What a lane is named after.** `git_core::lanes::LaneRefs` is built beside
  `GraphRows`, from the same rows, in the graph's background job: every row
  is pushed with what its commit's labels can name a lane by under the
  filter (`graph::lane_label`: a local or remote branch, else a tag; a
  detached `HEAD` names nothing). It keeps each lane's commit rows and, for
  every commit, the nearest commit at or above it on its lane with a branch
  and the nearest with any label. A page is a segment that carries on from
  the open lanes of the page before (`LaneRefs::carry`, held in the feed
  like the layout's state) and is appended on the window's thread, so paging
  and a refresh give exactly what one layout of everything gives. A lookup is
  a binary search in one lane's rows.
- **Branches before tags.** A line names the branches on the nearest commit
  at or above it on its lane that has any (`main, origin/main`), and only a
  lane with no branch above is named after its nearest tag. The ticket asked
  for "the nearest ref"; read literally, every line of main below a release
  tag said "Line of v0.2", which names a release, not the branch the line
  carries. A lane with neither "Leads to <short sha> <subject>" of its first
  commit. More than three names end in "and N more".
- **Curves.** A merge curve says "<from> merged into <into>": the merged
  lane's name, or the merged parent itself when nothing names it (a branch
  deleted after its merge), into the merge commit's lane's name or the merge
  commit. A fork curve says "<branch> branched from <from>" the same way. A
  dot gives the short hash and subject, then the author and how long ago in
  words. English and Traditional Chinese (`GraphCopy`).
- **Hit test.** `graph::spot_at` uses the cell's own geometry
  (`better_ui::fluent::lanes::segment` and `clipping`): within 7px of the dot
  is the dot (also in the overflow column for a hidden one), otherwise the
  nearest drawn line within 5px, so the hit area is 10px across a 1.5px
  line; curves are measured along 24 points. Lines clipped past the visible
  lanes are not hit.
- **The window.** Each graph cell has an overlay with a hitbox that blocks
  nothing, so the row is still clicked, dragged and hovered. After the
  pointer rests 500ms (GPUI's tooltip delay) the tooltip shows beside the
  pointer, and the lane is highlighted: the cell fades to 30% and the lane's
  lines and dots are drawn again over it at 2.5px. Moving straight to another
  line switches at once; a press, the wheel, leaving the cell, a drag, a
  replaced list or another repository put it away. The tooltip is the kit's
  tooltip, 480px wide at most.

Tests: `crates/git-core/tests/lanes.rs` (a straight line, a fork, a merge, an
octopus merge, a lane with no label, two labels on one lane, a branch before
a nearer tag and a tag on a lane without branches, a line that moves to
another column through a merge curve, and 2,000 shuffled commits laid out in
pages against one layout); `crates/git-gui/src/graph.rs` (the hit test, the
words for each spot in both languages through the background job, the
filter, a lane nothing names, many names, names across three pages, and how
long ago); `resting_on_a_line_a_curve_or_a_dot_says_what_it_is` in
`crates/git-gui/tests/graph.rs` against a repository made by real `git`.
Screenshots of a named line, a line nothing names, a merge curve, a fork
curve and a dot, in English and Traditional Chinese, and a click on a dot
under its tooltip selecting the row, were taken in a headless GNOME Shell
with a fixture repository made by real `git`.

Figures, from ticket 104's 100,000-commit measurement
(`cargo test -p git-gui --test graph --release -- --ignored --nocapture`),
release build, load 7–9, lane names now built in every page:

| Measured | Ticket 104 (load 8–9) | With lane names (load 7–9) |
| --- | --- | --- |
| First 3,000 rows, off the window's thread | 482 ms | 389 ms |
| All 100,000 commits | 724 ms | 572 ms |
| Slowest page read (background) | 8.4 ms | 7.0 ms |
| Slowest page added on the window's thread | 7.5 ms | 7.2 ms |
| Lanes for 30 rows at row 77,777 (one screen) | 6.6 µs | 6.1 µs |
| A tooltip's words for one line or dot there | — | 350 ns |
| Swap after a commit | 57 µs | 51 µs |

The measurement does not cover drawing: each visible row now also paints an
overlay and registers three pointer listeners, which was not timed.
