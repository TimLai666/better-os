# Ticket 92 — Better Git does everything GitHub Desktop does

**Blocked by:** 80 (done) · **Status:** merged, not released

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

## What was built

Better Git was rebuilt on `better_ui::fluent` and on three reworked crates
plus one new one. The plan was: parsers first (`git-core`), then every git
operation with real-git tests (`git-platform`), then the GitHub client with a
local fake server (`github-api`), then the GPUI-free session and its rules
(`git-gui`), and last the window, looked at block by block in the sandbox.

### Crates

- **`git-core`** (no I/O) gained parsers and rules for: the log in an explicit
  `-z` format with co-author trailers; a commit's files (`--name-status -z`);
  branches (`for-each-ref`, ahead/behind, gone upstreams, default branch,
  GitHub Desktop's branch-name sanitizing); stashes, with Better Git's
  `!!Better_Git<branch>` marker (GitHub Desktop's marker is read too);
  tags; worktrees (`--porcelain -z`); remotes and hosted-repository URLs
  (https, ssh, scp form, Enterprise hosts); conflict kinds (`UU`, `DU`, ...)
  and the operation in progress; conflict-marker counting; image sniffing;
  the side-by-side diff layout; the large-diff limit; and **partial patches**:
  a patch for the ticked lines of a file, either for the index (commit) or for
  the working tree in reverse (discard), made from git's own bytes, never from
  the lossy text on screen.
- **`git-platform`** runs every operation with the system `git` as before
  (argument vectors, `--`, `LC_ALL=C`, `core.fsmonitor` off, `--no-textconv`,
  `GIT_DIR` family removed; hooks off for reads, on for the commands that
  commit, check out, merge, or push, since every repository opened is
  trusted first; see the review section below). New: commit with
  amend, `--no-verify`, `--allow-empty`, and co-author trailers (a failing hook
  returns git's output, hook output included); commit of a *selection* (the
  index is read back from HEAD, whole files are added, partial files get a
  patch with `git apply --cached`, and a selection made on a diff that changed
  meanwhile is refused); discard of files or lines with the current content
  copied to the desktop trash first (`$XDG_DATA_HOME/Trash`, with
  `.trashinfo`, so any file manager restores it); ignore a file or an
  extension; log, commit files and diffs; undo the last commit (also the first
  one), reset, revert (merges with `-m 1`), cherry-pick (optionally onto
  another branch), and reorder/squash through `git rebase -i` whose todo list
  and squash message are written by Better Git (the sequence editor is `cp`
  with a quoted path, so nothing the person typed reaches a shell); branches
  (create from any commit, switch leaving the changes on the old branch as a
  stash or bringing them along, tracking a remote-only branch, detached
  checkout, rename, delete with or without force, merge, squash merge, rebase,
  compare); stash all, restore, drop, files and diffs (untracked files
  included), and finishing or undoing a restore that stopped on conflicts;
  tags; worktrees (add, move, remove); fetch, pull (merge unless the person set
  `pull.rebase`), push to the upstream's name with a force push leased to
  the commit the person saw, delete a remote
  branch, add/remove remotes and change URLs, clone, and creating a repository
  with README, `.gitignore`, and license; conflicts (take ours or theirs,
  including deletions, mark resolved, continue, abort for merge, rebase,
  cherry-pick, and revert); the global `user.name`, `user.email`, and
  `init.defaultBranch` (only those keys). A GitHub token reaches git through
  a credential helper reading a file made for the one command (see the
  review section), and network commands run with `GIT_TERMINAL_PROMPT=0`. The message reaches `git commit` from its own
  thread, closing ticket 77's open question about a long message against a
  chatty hook (a test commits 256 KiB against a hook printing 300 KB).
  `trust.rs` remembers trusted repositories; `keyring.rs` holds the token
  store's contract and an in-memory store for tests.
- **`github-api`** (new, `ureq`, already in the lock): the OAuth device flow
  (code, poll with pending, slow-down, expired, denied, disabled), token
  refresh with the refresh token and no client secret, the account, its
  repositories and organizations, repository details and permissions, branch
  protection, create a repository (personal or organization), fork, pull
  requests (list, create, reviews), check runs and commit statuses combined,
  re-running failed checks (Actions workflow runs through `rerun-failed-jobs`,
  other apps through check-suite `rerequest`), issues, and the `.gitignore`
  and license templates. The client ID is the owner's OAuth App
  (`github_api::CLIENT_ID`), overridable at build time with
  `BETTER_GIT_GITHUB_CLIENT_ID`; a build given an empty one says sign-in is
  unavailable and sends nothing.
- **`better-ui`**: the text box gained a multi-line form
  (`TextBox::multi_line`, with a test for its height), the error bar gained a
  next-step button (`ErrorBar::next_step`), and flyouts, menus, and the combo
  list now take the pointer (`occlude`), because a click on a menu item also
  reached the row under it.
- **`git-gui`**: the GPUI-free modules are `session` (two queues, one for git
  jobs and one for GitHub requests, each one at a time on a background thread,
  answers for a replaced repository dropped by generation), `changes`
  (GitHub Desktop's model: every change against HEAD, a check box per file
  that means "in the commit", ticked lines making a file indeterminate),
  `commit`, `history` (what may be rewritten: no merges, nothing already on
  the upstream), `branches`, `remote` (the sync button and the next step after
  a refusal), `conflicts`, `dialogs` (which dialog, and when its primary
  button works), `github` (sign-in, refresh before use and once after a 401),
  `notifications` (failed checks and reviews on the person's own pull
  requests, each once, sent through `org.freedesktop.Notifications` and shown
  in the window), `repositories` (the list with aliases, search, and the
  missing state), `settings` (stored choices, and starting the editor,
  terminal, Better Files, or browser with argument vectors), `keys`
  (GitHub Desktop's shortcut table with Ctrl), `trust`, `clone`, `publish`,
  and `i18n`. `app.rs` owns the text fields, the timers (sign-in polling and
  the five-minute background fetch), the keyring through GPUI's credentials
  API (the Secret Service via `oo7` on Linux; nothing is read until the
  person has signed in once), the clipboard, and the folder picker;
  `render.rs` draws.

### The window

GitHub Desktop's layout in fluent controls: the menu bar (File, View,
Repository, Branch) in the titlebar with every shortcut shown; a toolbar with
the repository and branch flyouts and the sync button (Publish repository,
Publish branch, Pull ↓n, Push ↑n, Fetch, with when it last fetched and a
progress ring while it runs); a Changes / History selector; the commit form
with co-authors, amend, bypass hooks, allow empty, push after commit, the
50-character hint, the protected-branch warning, and why Commit is off; the
diff with unified and split views, hide whitespace, expand whole file, a
gutter to tick lines and hunk headers to tick hunks, image before/after, and
the large-diff warning; the history list with tags, unpushed arrows, drag to
reorder (between rows) or squash (onto a row), drag onto a branch strip to
cherry-pick, and a context menu with undo, amend, reset, check out, revert,
cherry-pick onto, create branch, create/delete/copy tag, copy SHA, and view
on GitHub; the stash row and stash view with Restore and Discard; the
conflicts list (open in editor, use ours, use theirs, mark resolved with a
question when markers remain, Continue, Abort); errors as an error bar with
one plain sentence, git's text behind Show details, and the next step
(Pull, Force push, Sign in, Fork, Stash, Publish branch); dialogs for every
operation; and the settings page (accounts, integrations, Git, appearance
and language, prompts, the shortcut table). Opening a path for the first time
asks "Do you trust this repository?" and lists any filter, fsmonitor, or
hooks path its `.git/config` names.

### Deliberate choices

- The check boxes no longer stage. As in GitHub Desktop the index is built at
  commit time, so a file staged elsewhere shows as one change against HEAD.
  Ticket 77's "two quick clicks queue the same action twice" is gone with it:
  a click changes only the session.
- Lines cannot be ticked while whitespace changes are hidden (the lines on
  screen are not the lines git applies), as in GitHub Desktop.
- History rewriting is refused for merges and for commits already on the
  upstream; force push asks first and always uses `--force-with-lease` with
  the commit the person saw as the expected value.
- Tags are lightweight and pushed with the next push, as GitHub Desktop does.
- The background fetch skips a round when a job is running and fails quietly.

### Not done, or not verified

- **Nothing was signed in to GitHub.** Sign-in, pull requests, checks and
  re-run, issues, publish, fork, notifications, protected branches, templates
  from GitHub, and token refresh were exercised against a local fake server in
  `crates/github-api/tests/api.rs` and `crates/git-gui/src/tests.rs`, not
  against GitHub. Whether GitHub accepts a refresh without a client secret
  for this OAuth App is the owner's statement, not verified. GitHub
  Enterprise was not tried.
- **The real keyring was not written.** The sandbox session has no Secret
  Service; tests use the in-memory store. The window's keyring path is GPUI's
  credentials API.
- Network progress is a spinner, not a percentage.
- Not exercised on screen: reorder by drag (squash by drag was), cherry-pick
  by drag onto a branch, the image diff, open in editor/terminal/Better Files
  (their argument vectors are unit-tested), the folder picker (the portal is
  not reachable from the nested compositor, as in ticket 77), desktop
  notifications, rebase and cherry-pick conflicts (covered by real-git tests),
  publish and fork.
- Two windows writing the recent list at once can still lose one entry; the
  repository list and the trust store re-read before writing.
- The window was seen only at 960 wide: the sandbox compositor tiles it
  beside another window and would not give it more. 1600 wide was not seen.
  Not seen on a GNOME desktop either (headless sway only).

### Verified

- Checks: `cargo fmt --all -- --check`, `cargo +1.99.0 clippy` and stable
  `cargo clippy --workspace --all-targets --all-features --locked -- -D
  warnings`, `cargo +1.95 check --workspace --all-targets --locked`,
  `cargo test --workspace --locked --no-fail-fast`, and
  `packaging/generate-third-party-notices.sh --check`. `Cargo.lock` gained
  only the workspace's own `github-api` and existing packages as new edges;
  the license inventory was regenerated.
- Tests that touch git run real `git` in temporary repositories with `HOME`,
  the XDG directories, and `GIT_CONFIG_GLOBAL` pointed into the temporary
  directory (`GIT_CONFIG_NOSYSTEM=1`); GitHub tests use a server on
  127.0.0.1; the token store in tests is in memory.
- In the sandbox (HOME and XDG pointed at a scratch directory, a scratch
  repository with a bare `origin`), by hand: the trust question; the changes
  list with modified, deleted, staged rename, untracked, non-ASCII, and
  binary files; unticking one hunk and committing only the other (checked
  with `git show`); history with a co-author; squash by drag (the dialog);
  the branch list; switching with changes left behind, the stash notice,
  the stash view, and restoring it; push, a refused push explained with Pull
  offered, pull, push; merging a branch into a conflict, taking their
  version, and continuing; the menu bar with shortcuts; Ctrl+B, Ctrl+P,
  Ctrl+2, Ctrl+T, Ctrl+Shift+N, Ctrl+Shift+M, Ctrl+Alt+R; the history
  context menu and creating a tag (name sanitized); the split diff; the
  settings pages reading the scratch global git config; switching light to
  dark live; zh-TW and English.

### Screenshots

In `/tmp/claude-1000/-home-tim-Developer-better-os/3b559518-c3b2-4104-bc77-d1c2e845c29a/scratchpad/shots-92/`
(a session scratch directory; copy them before it is cleaned):
`01-trust-question`, `02-changes-list`, `03-diff-unified`,
`04-partial-commit-one-hunk-ticked`, `05-after-partial-commit`,
`06-history-commit-with-co-author`, `07-branch-list`,
`08-switch-leave-or-bring`, `09-stash-waiting-after-switch`,
`10-stash-view`, `11-stash-restored`, `12-pushed`,
`13-push-refused-explained-first-layout` (before the error bar moved git's
text behind Show details), `14-pulled`, `15-branch-menu-with-shortcuts`,
`16-merge-pick-branch`, `17-merge-conflicts-list`,
`18-conflicts-resolved-continue`, `19-merge-continued`,
`20-light-zh-TW-changes-and-diff`, `21-light-zh-TW-split-diff`,
`22-light-zh-TW-settings-accounts`, `23-light-zh-TW-settings-git-config`,
`24-dark-zh-TW-settings-appearance-switched-live`,
`25-dark-zh-TW-history-context-menu`, `26-dark-zh-TW-tag-created`,
`27-light-en-new-branch-dialog-sanitized-name`,
`28-light-en-repository-list-ctrl-t` (all `.png`, 1920×1080 captures with
the window at 960 wide on the right). Screenshots 01–19 are dark and
English.

## Independent review, and what it changed

An independent review (git 2.43, each finding reproduced in scratch
repositories) found five serious and nine moderate problems and a list of
smaller ones. All were fixed. Each fix has a test that runs real `git` in a
scratch repository (or the local fake GitHub server) and fails on the code
before the fix; for the ones marked *checked*, that was confirmed by putting
the old code back and watching the test fail.

### Serious

1. **`diff.noprefix` sent partial commits and discards to the wrong file.**
   Every diff Better Git reads now asks for `a/` and `b/` prefixes
   (`--src-prefix`, `--dst-prefix`), every command overrides
   `diff.noprefix` and `diff.mnemonicPrefix`, and `git apply` gets `-p1`.
   Test: `partial_commit_and_discard_with_diff_noprefix_touch_the_right_file`
   (*checked*).
2. **Reorder and squash used a stale history list**, and rebase silently
   dropped commits made since. The rewrite carries the commit HEAD was on in
   the list and is refused (`ChangedMeanwhile`, explained in plain words)
   when HEAD has moved; `rebase.missingCommitsCheck=error` is set on every
   command and a rebase refused that way is undone; the history list is read
   again whenever a refresh (window focus) finds HEAD moved. Tests:
   `a_rewrite_made_on_a_stale_list_is_refused_and_drops_nothing` (*checked*),
   `a_todo_list_that_leaves_a_commit_out_is_refused_and_undone`,
   `a_drag_on_a_stale_history_is_refused_and_refresh_reloads_it`.
3. **Switching with "leave" or "bring" popped `stash@{0}` even when nothing
   was stashed** (only a submodule's own changes), restoring an unrelated
   older stash. The switch now compares `refs/stash` before and after, and
   pops only the stash it made, found by its commit. Test:
   `switching_with_nothing_stashable_never_pops_an_older_stash` (*checked*).
4. **A squash merge that stopped on conflicts could be neither continued nor
   aborted**, and reopening the window guessed a stash conflict whose
   Continue dropped `stash@{0}`. `SQUASH_MSG` is now an operation of its own
   (`Operation::SquashMerge`): Continue commits with git's prepared message,
   Abort is `git reset --merge`. The guess is gone: conflicts no operation
   owns are shown, and finishing them unstages the files and touches no
   stash. Tests: `a_squash_merge_that_conflicts_can_be_continued_and_aborted`,
   `a_squash_merge_conflict_reopened_continues_without_touching_stashes`,
   `conflicts_left_from_another_stash_never_drop_the_newest_stash`.
5. **Push ran with hooks off, so Git LFS's `pre-push` never uploaded the
   files.** Every repository is trusted before it is opened, so the commands
   that commit, check out, merge, rebase, cherry-pick, revert, pull, or push
   now run the repository's hooks (`post-checkout` and `post-merge` matter to
   LFS too). Reads keep hooks off. Test:
   `push_runs_the_pre_push_hook_that_git_lfs_needs` (*checked*).

### Moderate

6. **The force-push lease had no expected value**, so a background fetch
   made it useless. The push is `--force-with-lease=refs/heads/<name>:<sha>`
   with the commit the remote branch was on when the question was asked
   (kept in the dialog, so a fetch while it is open changes nothing).
   `--force-if-includes` is not passed: git's documentation says it does
   nothing next to an explicit expected value. Tests:
   `force_push_with_lease_overwrites_only_what_was_fetched` (*checked*),
   `push_and_delete_use_the_upstream_name_and_force_keeps_its_lease`.
7. **Pushing and deleting used the local branch name**, and `-u` rewrote the
   upstream on every push. Both now use the upstream's name on its remote
   (a remote whose name holds `/` is matched by the longest name), and `-u`
   is passed only to a branch that has no upstream yet. The delete question
   now says that commits only on that branch go with it. Tests:
   `push_goes_to_the_upstream_name_and_keeps_the_upstream_set`, and the
   session test above.
8. **"Use ours" and "Use theirs" were backwards during a rebase.** Each side
   is now named: the branch and the merged branch for a merge, the commit
   rebased onto and the branch being rebased for a rebase, the commit for a
   cherry-pick or revert, the branch and "the stash" for a stash. The buttons
   read "Use the version from topic", and so on. Test:
   `the_sides_of_a_conflict_are_named_for_each_operation`.
9. **Listing history ran the repository's `gpg.program`**
   (`log.showSignature`). Every command now sets `log.showSignature=false`.
   The trust question now says plainly that a trusted repository can run any
   program its settings or hooks name, and lists as examples filters, diff
   and merge drivers, `gpg.program`, `core.sshCommand`, editors, pagers,
   credential helpers, aliases, included configuration files, and hooks,
   following a worktree's `.git` file to its configuration. Tests:
   `listing_history_never_runs_the_signature_program` (*checked*),
   `trust_lists_the_programs_a_repository_names`.
10. **Writes followed symbolic links out of the repository.** Ignoring a file
    refuses a `.gitignore` that is not a plain file and opens it with
    `O_NOFOLLOW`; the rebase todo file has a random name and is created with
    `create_new`, and the squash message is created the same way after any
    link at its name is removed. Patterns now escape trailing spaces too.
    Tests: `ignore_refuses_a_gitignore_that_is_a_link_out_of_the_repository`,
    `path_patterns_keep_trailing_spaces_and_special_characters`, and the
    squash test below.
11. **The image diff and the conflicts list read through links with no size
    limit**, and the conflicts list read files on every repaint. Files are
    read only when they are plain files, without following a link, through a
    cap (32 MiB for images, 16 MiB for conflicts); conflict markers are
    counted in the job that reads the status, off the window's thread. Test:
    `an_image_that_is_a_link_to_a_device_is_refused_without_reading_it`.
12. **One queue, no cancel, no timeout.** Network commands run in their own
    process group with `--progress`; they are killed when the person presses
    Cancel (shown on the sync button while it runs) or when nothing has been
    printed for two minutes; `ssh` is told to ask no one
    (`SSH_ASKPASS_REQUIRE=force` with `false`, so a password or host-key
    question fails instead of waiting). The background fetch runs beside
    the queue, so a stuck one never holds up a commit, and the person's own
    fetch, pull, or push cancels it. Tests: `a_fetch_that_stalls_is_stopped`,
    `a_fetch_can_be_cancelled`, `ssh_is_told_to_ask_no_one`,
    `a_stuck_background_fetch_does_not_hold_up_a_commit_and_can_be_stopped`.
13. **A choice of lines was checked only against the diff's shape.** It is
    now checked against a fingerprint of every changed line (kind, numbers,
    text), and lines are remembered by their numbers, so expanding the whole
    file no longer loses the choice. Tests:
    `a_change_of_the_same_shape_made_meanwhile_is_refused`,
    `a_selection_made_on_the_whole_file_view_commits_the_same_lines`,
    `the_change_fingerprint_ignores_context_and_sees_every_change`.
14. **During a merge, a partial selection could drop the other side's
    changes.** While a merge or squash merge is in progress every file is
    committed whole and the check boxes and line ticks are locked, as in
    GitHub Desktop. Test: `a_merge_locks_the_choice_of_files_and_lines`.

### Smaller

- `--end-of-options` comes before every ref or revision argument (switch,
  branch create/rename/delete, merge, rebase, log, compare, rev-list,
  rev-parse, revert, cherry-pick, tag create/delete, remote remove, worktree
  add, fetch). `git reset` refuses it (git 2.43), so a reset's revision is
  checked to be a commit id instead.
- A squash that stops on conflicts keeps the message the person wrote:
  `rebase --continue` commits it. The message file is removed when the
  rebase ends or is aborted. Test:
  `a_squash_that_stops_on_conflicts_keeps_the_message_the_person_wrote`.
- Undo commit puts back HEAD's own message, read from git when it runs.
  Test: `undo_puts_back_the_message_of_the_commit_git_has`.
- GitHub: pull requests, reviews, check runs, statuses, organizations, and
  repositories are read page by page (up to ten pages of 100); a rate limit
  (403 or 429 with no requests left or a `Retry-After`) is its own error,
  explained with when to try again; an Enterprise server can be given its
  own OAuth App client ID in the sign-in dialog (kept with the account's
  host, not a secret); GitHub is asked again after a background fetch.
  Tests: `lists_are_read_page_by_page`,
  `a_rate_limit_is_its_own_error_with_when_to_retry`,
  `an_enterprise_host_signs_in_with_its_own_client_id`.
- The token no longer reaches git as `GIT_CONFIG_*` environment variables
  that every program git starts inherits. A credential helper for the
  token's host (all others for that host switched off for the command)
  prints a file made for the one command in a fresh `0700` folder under
  `$XDG_RUNTIME_DIR`, readable only by the user and removed when git exits;
  the command line and the environment carry only its path. The token is
  now sent only when the server asks for it (a 401), as git does with any
  helper. Test:
  `the_token_reaches_git_through_a_helper_and_never_the_environment`.
- Desktop notifications escape `&`, `<`, and `>` and drop control
  characters, so a pull request's title cannot carry markup. Test:
  `notification_text_is_never_read_as_markup`.

### Left as they are

- A background fetch and a local command can still meet on one of git's lock
  files (a ref, `packed-refs`); git then refuses with its own message and the
  next try works. Making them wait for each other would bring back the stuck
  queue.
- The changed-lines fingerprint is taken over the text as shown, which
  replaces bytes that are not UTF-8. Two edits that differ only in such
  bytes, at the same places, would not be told apart; the patch itself is
  still made from git's bytes.

### Screenshots after the review

In the same scratch folder, 1920 wide this time (the window was given the
whole headless screen): `29-review-trust-question-any-program` (the plain
statement and the programs found, `core.sshCommand` and `gpg.program`),
`30-review-rebase-conflict-sides-named` (a rebase: "Use the version from
90c6c49" is the commit rebased onto, "from topic" is the person's branch),
`31-review-fetch-running-with-cancel`, `32-review-fetch-cancelled` (a fetch
through an `ssh` that never answers, stopped with Cancel),
`33-review-enterprise-sign-in-client-id`, and
`34-review-merge-conflict-sides-named`; the trust question was also seen in
zh-TW and dark. Not seen on screen: the force-push question's new text (the
window offers Force push only after a rewrite, and the scratch repository
did not reach that state; the session test covers its lease), and the
locked check boxes (during a merge the window shows the conflicts list in
their place, so the lock is a guard in the session, covered by its test).
