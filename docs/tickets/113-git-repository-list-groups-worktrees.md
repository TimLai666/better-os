# Ticket 113 — Better Git's repository list keeps worktrees under their repository

**Blocked by:** none · **Status:** open

## Why

On 2026-11-14 the owner's repository dropdown in Better Git showed the same
repository twice and mixed worktrees with repositories:

- "ticket-101" (`/home/tim/Developer/worktrees/better-os/ticket-101`, a linked
  worktree of `~/Developer/better-os`) was listed as a repository of its own,
  with every other worktree of better-os listed under it, including itself.
- "better-os" (`~/Developer/better-os`, the main worktree) was listed again
  below with the same worktrees.
- A detached worktree showed only "分離的 HEAD" (detached HEAD) as its name;
  its folder name (`release-0.2.13`) was only in the truncated path.

A linked worktree that was added (or opened) as a repository is the same
repository as its main worktree: they share one git common directory
(`git rev-parse --git-common-dir`).

## What to build

- The list groups by repository, keyed by the git common directory, not by
  the folder that was added. Each repository appears once, named after its
  main worktree's folder, with its worktrees under it; the main worktree is
  one of the entries, marked as the main one.
- Adding a linked worktree adds (or selects) its repository and selects that
  worktree; it does not create a second repository entry. Saved lists that
  already hold a linked worktree as a repository are merged on load, keeping
  which worktree was selected.
- Each worktree row is named by its folder; the branch, or "detached at
  <short sha>", is the secondary text. The current one is marked; the whole
  row is clickable (ticket 103 rule).
- Removing a repository removes all its worktree entries from the list (not
  from disk).
- The filter matches repository names, worktree folder names and branches.
- A bare repository or a worktree whose main worktree is gone still shows,
  under its common directory, without errors.

## Acceptance

- [ ] Unit tests with fixture repositories (made in a temp dir with real git):
      main + 3 linked worktrees added in every order give one group; a saved
      list with a linked worktree as a repository is merged; detached naming;
      removal; filter.
- [ ] Screenshot of the dropdown in a headless sandbox with a fixture
      repository and worktrees, in both languages.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
