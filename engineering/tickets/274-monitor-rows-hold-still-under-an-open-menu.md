# Ticket 274 — A Better Monitor row does not move while its menu is open

**Blocked by:** none

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks.

The old branch held the table still for two seconds after a selection or a
menu opened. On `main` a selection follows its row by key, so the wrong
process is never acted on, but the rows still reorder under the pointer
while a context menu is open or the pointer is on a row's button; only the
manual Pause stops it.

## What to build

- While a row's menu or a confirmation for a row is open, the order of the
  visible rows is held (values may still update in place); it resumes when
  the menu closes. No timer-based guess.

## Done when

- Tests in `monitor-views` for the hold and its release; seen in a sandbox
  with live rounds.
