# Ticket 276 — A Better OS window reopens the way it was left

**Blocked by:** none

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks.

A window keeps its page, sort order and size across an update restart
(`better_ui::restart`), but an ordinary close and reopen starts from the
defaults. The hand-off already knows what a window's view is.

## What to build

- One rule in `better-ui`, for every window that has a restart view: on a
  normal close the same view is kept as the person's setting and read at the
  next start (window size and whether it was maximized, the page, the sort
  order and what else the restart view holds that still makes sense later:
  not a half-typed field, not a selection that names a process). Decide per
  window what "still makes sense" is and write it in each window's `restart`
  module.
- Named in each manifest's `paths`, removed with the settings box.

## Done when

- Tests per window in its GPUI-free `restart` module; a stored view from an
  older version; a screen that is now smaller than the stored size.
