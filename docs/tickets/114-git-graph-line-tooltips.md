# Ticket 114 — The branch graph says what a line is when the pointer rests on it

**Blocked by:** 104 (merged) · **Status:** open

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

- [ ] Unit tests on fixture graphs: linear, fork, merge, octopus merge, a lane
      with no ref, two refs on one lane, a lane that changes column.
- [ ] Screenshots in a headless sandbox of a line tooltip, a merge curve
      tooltip and a dot tooltip, with the lane highlighted, in both languages.
- [ ] Scrolling the 60k-commit fixture stays as smooth as before (same
      measurement as ticket 104).
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
