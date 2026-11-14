# Ticket 113 — Better Git's repository list keeps worktrees under their repository

**Blocked by:** none · **Status:** done, not merged

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

- [x] Unit tests with fixture repositories (made in a temp dir with real git):
      main + 3 linked worktrees added in every order give one group; a saved
      list with a linked worktree as a repository is merged; detached naming;
      removal; filter.
- [x] Screenshot of the dropdown in a headless sandbox with a fixture
      repository and worktrees, in both languages.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **One entry per repository.** `repositories::Known` holds what Git said
  about a folder: its common directory, its worktrees, and where its main
  worktree is. Git lists the common directory itself as the main entry for
  a submodule and a `--separate-git-dir` repository, so the main worktree is
  the folder itself when its git directory is the common one, else
  `core.worktree` (`Git::core_worktree`, new), else the main entry when it
  is a working tree. The common directory stands for the repository only
  when it is bare or its recorded main worktree is gone; when Git cannot
  say (a linked worktree of a `--separate-git-dir` repository), the entry
  is kept as it is.
- **Only real answers count.** A read is accepted only when the folder is
  the top of a repository (or a bare repository's own folder): Git walks up
  from a plain folder to an enclosing repository, and that answer is about
  another repository. A folder Git cannot read is left in the list exactly
  as it was and drawn as missing.
- **Opening a worktree saves its repository.** The open answer carries the
  common directory, and `RepositoryList::add_opened` stores the repository and
  turns an entry for the worktree itself into the repository's. The recent
  list keeps the worktree, so the one that was open is still the one marked.
- **Old lists are merged on load.** The window reads the list's worktrees,
  in a lane of its own beside the git queue (one read at a time, again
  after it when asked meanwhile), at start (and whenever the dropdown opens, by click or Ctrl+T), and
  `RepositoryList::merge_worktrees` folds every entry that turned out to be a
  linked worktree into its repository's entry, in its place, and writes the
  file once. The repository's own alias wins; a worktree entry's alias carries
  over only when the repository had none.
- **The dropdown draws groups.** `RepositoryList::groups` keys by the common
  directory, so even before the merge is written a repository shows once. Each
  repository row is named after its main worktree's folder and opens it; under
  it, when it has linked worktrees, every worktree is a row named by its
  folder, with the branch, "Detached at <short sha>", "Main worktree", or
  "Can't be found" beneath, the open one marked and the whole row clickable.
- **Drawing touches no disk.** Whether a folder is there is what the last
  read found, and Git's canonical paths are compared as they are.
- **A second window cannot undo a fold or a removal.** The file keeps the
  folders dropped lately (up to 256), and a window merging the file into a
  stale list drops them too; adding a folder again takes it off.
- **Removing** a repository removes every saved entry that is it and the
  recent folders of its worktrees; nothing on disk is touched.
- **The filter** matches a repository's name or folder (all its worktrees
  stay) or a worktree's folder name or branch (only those worktrees stay).

Tests: six in `crates/git-gui/src/repositories.rs` on fixture repositories
made by real `git` in a temporary directory (main plus three linked
worktrees opened in all 24 orders, an old list merged, detached naming,
removal, the filter, a bare repository and a repository whose main worktree
is gone), and `linked_worktrees_saved_as_repositories_merge_into_their_repository`
in `crates/git-gui/tests/session.rs` through the session. Screenshots of the
dropdown in English and Traditional Chinese, the filter, and a worktree row
opened by a click on its empty right side were taken in a headless GNOME
Shell with a fixture repository whose saved list held a linked worktree as a
repository.
