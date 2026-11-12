# Ticket 92 — Better Git does everything GitHub Desktop does

**Blocked by:** 80 (done), the owner's GitHub OAuth App client ID for sign-in · **Status:** ready

## Why

The owner wants Better Git to replace GitHub Desktop and rejected the first
window (ticket 77) as half finished. On 2026-11-12 the owner decided: sign in
to GitHub with an OAuth App the owner registers; no AI-generated commit
messages or AI conflict help.

## What to build

`crates/git-gui`, `crates/git-core`, `crates/git-platform`, rebuilt on
`better_ui::fluent` (ticket 80) and complete against GitHub Desktop
(desktop/desktop and docs.github.com/en/desktop, read 2026-11-12):

- **Repositories.** Repository list with search, aliases, and a missing-repo
  state; add local, create new (with .gitignore and license templates),
  clone by URL and from the signed-in account's repositories (GitHub.com and
  Enterprise), publish a local repository to GitHub, fork when pushing to a
  repository the account cannot write to; remove from the list.
- **Changes.** Changed files with check boxes, a filter, and the
  indeterminate state; partial commit by line and by hunk; discard a file,
  selected files, all changes, or selected lines, with discarded content kept
  recoverable; unified and split diff, hide whitespace, expand whole file;
  image diffs; oversized file warning; ignore a file or extension.
- **Commit.** Summary and description, co-authors, amend the last commit,
  commit to a protected branch warning, bypass hooks with git's own hook
  output shown when one fails, allow empty commit, push after commit.
- **History.** The branch's log with each commit's files and diff; undo the
  last commit, reset to a commit, revert, cherry-pick (also by drag onto a
  branch), reorder and squash by drag, amend; create and delete tags; check
  out a commit (detached HEAD warning); create a branch from a commit; copy
  SHA and tag.
- **Branches.** Create, switch (leave changes or bring them), rename, delete
  local and remote, publish, compare with another branch, merge into
  current, rebase current, update from the default branch, stash all
  changes and restore or discard it, worktrees (add, switch, rename, remove).
- **Remote.** Fetch (also periodic in the background), pull, push, force
  push with a warning; every rejection explained in plain words with the
  next step; remote settings (change URL, add and remove remotes).
- **Conflicts.** A conflict list for merge, rebase, cherry-pick, and stash
  pop: per file open in editor, use ours, use theirs, mark resolved;
  continue and abort.
- **GitHub.** OAuth device-flow sign-in and sign-out, token in the desktop
  keyring through the Secret Service, never in a file; pull requests for the
  repository (list, check out, create, view on GitHub), CI checks on the
  current branch with re-run, create an issue, notifications for failed
  checks and reviews on the person's pull requests.
- **Integrations and settings.** Open in the external editor and the
  terminal, show in Better Files, Git config (name, email, default branch),
  appearance (dark, light, system), language, confirmation prompts.
- **Keyboard.** GitHub Desktop's shortcut table, with Ctrl on Linux.
- **Safety.** Everything ticket 77's review required stays: no repository
  program runs while reading; the remaining `filter.<name>.clean` gap is
  closed by asking "Do you trust this repository?" the first time a
  repository is opened, remembered per path.

## Acceptance

- [ ] Every item above works in the window against real git in a scratch
      repository (and against GitHub for the GitHub items once the client ID
      exists), each shown by a screenshot.
- [ ] Light and dark, English and zh-TW, 1600 and 960 wide.
- [ ] Unit and integration tests for every git operation; tests never touch
      the person's real repositories, `~/.gitconfig`, keyring, or GitHub
      account.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
