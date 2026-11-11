# Ticket 77 — A person can review and commit changes in Better Git

**Epic:** Better Git · **User Story:** a person opens a repository, sees what
changed, and commits it without a terminal · **Branch:** `ticket-77` ·
**Blocked by:** none · **Status:** ready

## Why

On the owner's machine GitHub Desktop opens with every label blank. Better Git
is the first-party replacement, a GPUI window like the rest of the suite. This
ticket is the first usable slice. Tickets 78 and 79 follow it.

## Decisions

- **It runs the system `git`.** Every operation is `git` invoked with an
  argument vector, never through a shell, with `-C <repository>`. The person's
  own configuration, credential helper, and SSH keys keep working, so the first
  slices need no sign-in of their own. No Git library is linked: the Rust ones
  are incomplete for push and the complete one is C, which `AGENTS.md` forbids.
- **Three crates**, the way the other components are split: `git-core` (models
  and the parsers for git's machine-readable output, no I/O), `git-platform`
  (runs `git`, behind a trait with a fake), `git-gui` (the window; decisions in
  GPUI-free modules).
- **Output is parsed from porcelain formats only**: `status --porcelain=v2 -z`,
  `diff --no-color` and `diff --cached --no-color`, `log` with an explicit
  format and `-z`. Nothing parses text meant for people, and `LC_ALL=C` is set.
- **A path never becomes an option.** Every path argument follows `--`.

## What it delivers

- Open a repository by path, from the window or as the first argument, and
  remember the repositories opened before.
- A changes list: each changed file with its state (modified, added, deleted,
  renamed, untracked, conflicted) and whether it is staged.
- The diff of the selected file, staged or unstaged, with added and removed
  lines marked. A binary file says it is binary.
- Stage and unstage a file, and all files.
- A summary and description, and Commit. Commit is disabled with nothing staged
  or an empty summary. A failed commit shows git's own message.
- The current branch name in the header.
- zh-TW and English, the shared titlebar, `--help` and `--version` through
  `better_ui::command_line`.

## Acceptance criteria

- [ ] A path that is not a repository says so and opens nothing.
- [ ] The changes list matches `git status` for: modified, added, deleted,
      renamed, untracked, conflicted, a file both staged and modified, and
      names with spaces, non-ASCII characters, and a leading `-`.
- [ ] Staging and unstaging change exactly the chosen file.
- [ ] A commit made in the window appears in `git log` with the summary and
      description given, and the changes list is empty afterwards.
- [ ] A repository with no commits yet works.
- [ ] `git` missing from the machine is reported in the window, not a crash.
- [ ] Parser tests use recorded git output; platform tests run real `git`
      against repositories created in temporary directories, with `HOME` and
      the global and system configuration pointed away from the person's own.
- [ ] The window was opened against a real repository and looked at.

## Later tickets

- **78**: branches (list, switch, create), history with each commit's diff,
  discard changes.
- **79**: fetch, pull, push, with progress and git's error shown; the remote
  state in the header.
- Not planned yet: pull requests, clone from a list of GitHub repositories,
  conflict resolution, stashes.
