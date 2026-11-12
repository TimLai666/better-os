# Ticket 77 — A person can review and commit changes in Better Git

**Epic:** Better Git · **User Story:** a person opens a repository, sees what
changed, and commits it without a terminal · **Branch:** `ticket-77` ·
**Blocked by:** none · **Status:** released in v0.2.9

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

## What was built

- **`better_ui::fluent`** holds `DESIGN.md`'s tokens for every window that
  moves to the WinUI look: the light and dark palettes (picked from
  gpui-component's current mode with `fluent::palette(cx)`), the type ramp,
  the 4px spacing grid, the two radii, the standard control and row sizes, and
  `apply_to_component_theme`, which points gpui-component's text fields and
  scrollbars at the same palette. A test pins the palettes to the table in
  `DESIGN.md`. Nothing Better Git specific is in it.
- **`crates/git-gui`** (library `git_gui`, binary `better-git`). The
  decisions are in GPUI-free modules: `session` (the open repository, its
  status, the selection and its diff, the commit form, the recent list, the
  error on screen), `i18n` (zh-TW and English), `recent` (the list in
  `$XDG_STATE_HOME/better-os/git-recent.json`, written beside its name and then
  renamed). Every git call is a `Request` the session queues; the window runs
  one at a time on a background thread and hands the `Outcome` back, so no
  `git` process runs on the thread that paints and two writes never race for
  git's index lock. Tests run the same requests with `run_pending`.
- Rules the session holds, each with a test against a fake `Git` and, where
  it matters, against real `git` in a temporary repository: Commit is
  offered only with something staged and a summary that is not blank; a
  commit empties the form and reads the status again; every `GitError` becomes
  the info bar's message; a file staged and changed again is indeterminate and
  its checkbox stages the rest; the checkbox never moves the selection; a
  selection whose file is gone after a refresh is cleared, and one whose side
  has nothing left shows the other side; unstaging a staged rename names both
  of its paths, because naming only the new one leaves the old one deleted in
  the index; a path that is not a repository leaves the open repository as it
  was; an answer for a repository that was closed meanwhile is dropped.
- The window follows `design/compiled-spec.md`: command bar with the
  repository button, the branch, and Open…; the 320px changes pane with the
  header checkbox, 36px rows, state letters, the selected row's fill and
  accent bar, and the commit form pinned to the bottom; the diff card in
  monospace 13/20 with two line-number columns, tinted added and removed rows,
  and sideways scrolling for long lines; the no-repository column with the
  recent list; the Critical info bar; and the clean, binary, no-change, and
  nothing-selected states. `better-git <path>` opens a repository, and
  `--lang` and `--appearance dark|light|system` are accepted beside `--help`
  and `--version`; `--` takes a path that starts with `-`.

### Deliberate choices

- **A conflicted file cannot be staged** from the window, and its diff is
  not read: staging it would mark the conflict resolved, and git's combined
  diff for it is a format `git-core` does not parse. Conflict resolution is a
  later ticket. The row says "Conflicted", and a commit attempted during a merge
  shows git's own refusal.
- **The checkbox is drawn by the window**, because gpui-component's has no
  indeterminate state.
- **The repository button goes back to the repository chooser**, so it is an
  action and not a decoration; a repository list in a flyout can replace it.
- **A partly staged file shows its unstaged side** when selected. The session
  can show the other side (`show_side`), but there is no control for it on
  screen yet.
- gpui-component's icon set has no branch glyph, so the window carries a small
  SVG of its own, drawn for this window.
- Enter in the summary field commits, when Commit is enabled.

### Not verified or not matching the spec

- The window was opened in a headless sway session (wayhand sandbox) against a
  scratch repository with a modified, renamed, deleted, binary, partly staged,
  non-ASCII, and leading-dash file, in dark and in light, in English and in
  zh-TW: a diff was selected and scrolled sideways, files were staged, two
  commits were made (one with Enter) and appear in `git log`, the list emptied,
  and a folder that is not a repository showed the info bar. It has not been
  looked at on a GNOME desktop.
- **The Open… folder picker was not seen working.** In the sandbox it showed
  nothing and reported no error: gpui asks the desktop portal, which lives on
  the host's session bus and not in the nested compositor. Opening by argument
  and from the recent list was used instead.
- A focused field gets gpui-component's accent ring on all four edges, not
  WinUI's 2px accent bottom edge. The light theme's fields use the window
  background as their fill rather than the control fill.
- A folder caption is cut at its leading end by character count (28), not by
  measured width, so a caption can still be clipped by the row on a narrow
  font.
- The diff card is a virtual list, so a very large diff stays responsive, but
  its line numbers scroll sideways with the text instead of staying put.
- The palette values are `DESIGN.md`'s, which that file says are recalled and
  still to be checked against the WinUI source.
- Packaging (a desktop entry for `io.betteros.Git`, a manifest, `.deb`) is out
  of this ticket.

### Independent review, and what it changed

A reviewer who saw only the ticket and the diff ran the product's exact `git`
commands against a repository whose own configuration named programs to run.

Fixed, each with a test that fails without the fix:

- **Opening a repository ran what the repository configured.** `status`,
  `diff`, `add`, and `restore` started its `core.fsmonitor` program, its
  `post-index-change` hook, and its diff `textconv` command. Every command now
  turns `core.fsmonitor` off, every command except `commit` turns hooks off,
  and `diff` runs with `--no-textconv`.
- A file that changed type (a file that became a symlink) had no readable
  diff: git prints two diffs for the one path and the parser refused the
  second header.
- Pressing Commit while another repository was opening lost the commit and
  left the Commit button disabled in the new repository. Commit is now
  withheld while an open is waiting or running.
- An untracked folder that is a repository of its own showed "no changes";
  it now shows git's error.
- A repository git refuses for its owner (`safe.directory`) was called "not a
  repository"; it now shows git's explanation.
- `GIT_DIR` and its relatives in the environment redirected every command to
  another repository; they are removed.

Left open, for the next Better Git ticket:

- A `filter.<name>.clean` command named by the repository's attributes still
  runs when git reads a working-tree file. git has no switch for it. The
  choices are a "trust this repository" question before the first read, or
  accepting it in an ADR.
- The summary and description stay editable while a commit runs, and what is
  typed then is cleared when the commit lands.
- Two quick clicks on one checkbox queue the same action twice instead of
  toggling back.
- A diff has no size limit: selecting a very large file reads all of it into
  memory, and each render walks every line to find the widest.
- Two windows saving the recent list at once can lose one window's entry.
- Whether a commit message over 64 KB can deadlock against a hook that prints
  as much before git reads stdin was not tested.
- The parser samples are written by hand in git's format, not recorded from a
  run, except for the cases the platform tests take from a real `git`.
- The fixes above were verified by the author of the fixes and have not had a
  second independent review.
