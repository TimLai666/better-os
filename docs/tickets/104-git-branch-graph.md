# Ticket 104 — Better Git shows the branch graph

**Blocked by:** 92 (done) · **Status:** merged, not released

## Why

The owner asked on 2026-11-14 for a third view next to "Changes" and
"History": the graph of every branch, with the coloured lines that show
where branches split off and merge, as `git log --graph`, VS Code's Git
Graph, or Fork draw it. GitHub Desktop has no such view; Better Git's
History shows one branch as a flat list.

## What to build

- **A third tab, "Graph" ("分支圖").** The left pane's `SelectorBar` gets
  Changes, History, Graph. Graph uses the whole width of the window: the
  graph and commit list on the left, the selected commit's details and diff
  on the right, as History does.
- **Lanes computed without I/O.** `git-core` gains a lane layout: from
  commits in topological order with their parents (read by `git-platform`
  with `git log --all --topo-order -z` in an explicit format: hash, parents,
  author, date, subject, refs), it assigns each commit a column and each
  edge a path, keeping a branch in the same column while it lives, reusing
  freed columns, and giving each lane a stable colour from the Fluent chart
  palette. Merge and fork edges are drawn as curves. Unit tests cover a
  straight line, a fork, a merge, an octopus merge, a branch that ends,
  criss-cross merges, and a root commit in the middle (unrelated
  histories).
- **Labels.** Each commit row shows its branch, remote branch, tag, and HEAD
  labels as chips; the current branch is marked. A filter chooses all
  branches, local only, or the current branch only, and whether remote
  branches and tags are shown.
- **The same commit actions as History.** Selecting a commit shows its
  files and diff; the right-click menu offers what History offers (checkout,
  create branch or tag here, cherry-pick, revert, reset, copy hash), and
  dragging works the same way.
- **Large repositories.** The list is virtualized and loads in pages of a few
  thousand commits as the person scrolls, with the lane state carried from
  one page to the next, so a repository with 100,000 commits opens without
  stalling the window; the work runs off the UI thread. Refreshes after a
  fetch or commit keep the scroll position and the selection.
- **Search.** Ctrl+F in the graph finds commits by subject, author, or hash
  prefix and scrolls to them.

## Acceptance

- [x] Screenshots of the Graph tab on a repository with several branches,
      merges, tags, and a remote, in light and dark, English and zh-TW.
- [x] Lane layout unit tests for every case listed above.
- [x] Opening the graph of a generated 100,000-commit repository: the first
      rows show in under one second and scrolling does not stall; numbers
      stated.
- [x] Tests use temporary repositories only.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## What was built

Better Git has a third tab, Graph (分支圖, Ctrl+3), with every branch's
commits and the coloured lanes between them.

- **Reading the history.** `git_platform::graph` runs one
  `git log -z --topo-order --decorate=full` in `git_core::log::LOG_FORMAT`
  while the graph is shown, and reads it a page of 3,000 commits at a time from
  the pipe (`LogStream`). What git prints beyond the page waits in the pipe, so
  no page is read twice. The walk's starting commits are pinned at its first
  page (`Git::graph_tips`, `git rev-parse` of the same revisions), and git is
  started from those ids, handed to `git log --stdin` rather than as
  arguments, so a repository with tens of thousands of tags never passes the
  system's argument length limit (tested with 60,000 packed tags). Hiding the tab stops git (after a read in progress);
  showing it again starts git from the pinned ids with `--skip` at the commits
  already read, and the lane layout carries on from where it was. If a branch,
  tag, or HEAD moved meanwhile, carrying on would repeat or drop commits, so the
  graph is read again from the top instead (`GraphPage::moved`; tested by a
  commit made while the tab is hidden). A stream that is dropped, or stops
  on output it cannot read, kills git unless git has already exited and been
  waited for, so a git blocked on a full pipe can never hang the thread that
  drops it (`crates/git-platform/tests/graph.rs` drops a half-read 60,000-commit
  stream with a hand-made broken commit on top). git prints no time for a date
  it cannot read; that commit is dated 0 rather than failing the whole history.
  `git_core::graph::RecordReader` splits the bytes into
  commits as they arrive, across any read boundary. The filter picks the
  revisions: All is `--branches` plus `--remotes` and `--tags` when shown, plus
  `HEAD`; Local is `--branches HEAD`; Current is `HEAD`. `--all` is not used,
  because it walks the stash's internal commits. `HEAD` is left out while it
  has no commit, so an orphan branch beside others still shows them.
  `Git::graph` is the trait method; `MemorySource` is the in-memory source for
  fakes.
- **Lanes.** `git_core::lanes::LaneLayout` takes commits children first and
  answers each with a `GraphRow`: the node's column and lane, and every line in
  the row as `Pass`, `In` (into the node; a fork when from another column), or
  `Out` (to a parent; a merge or a new line when to another column). A commit
  takes the leftmost lane waiting for it, the first parent continues in its
  column and colour, a further parent joins the lane already waiting for it or
  opens one in the leftmost free column, a root ends its lane, and freed columns
  are reused. Lane numbers count up as lanes open and the window turns them into
  the chart hues, so a lane keeps one colour while it lives. The layout's state
  is carried from page to page. `crates/git-core/tests/lanes.rs` covers a
  straight line, a fork, a merge, a merge into a lane already open, an octopus
  merge, a parent named twice, a branch that ends (a root and a merged
  branch), criss-cross merges, a root in the middle (unrelated histories),
  paging, and many branches, and checks on every case that what leaves the
  bottom of one row enters the top of the next in the same columns and lanes.
  `GraphRows` keeps laid-out rows small: only the lines that start or end at a
  node are stored, plus the open lanes every 128 rows, and a row's passing lines
  are rebuilt when it is drawn. 200 branches alive across 50,000 rows store
  under 500,000 lines instead of more than 9 million, and rebuilding the 30 rows
  of a screen deep in a 100,000-commit history takes about 13 µs.
- **Off the window's thread.** The graph's reads run beside the session's job
  queue (`GitSession::next_graph` and `apply_graph`; the window spawns them as
  it does the background fetch), because `git log` takes no lock and a long
  history must not hold up a commit. Opening the feed, reading, and the lane
  layout all happen in the job, and so does the hash index of what it read; the
  window only adds the page. A refresh that reads the whole history again
  swaps it in without rebuilding anything, and the list it replaces is freed
  on a thread of its own.
- **The list.** `graph_pane.rs` draws a `uniform_list`, so only the rows on
  screen are built. Each row is the lane cell, the label chips, the subject,
  the author and age (dropped in that order when the window is narrow), and the
  short hash. The whole row reacts to the pointer, its 4px inset included; the
  inset look is drawn inside it. The graph column shows up to 12 lanes and its
  right edge can be dragged to show more or fewer, as far as the subject keeps
  120px. When lanes are hidden, one overflow column follows the visible ones:
  it holds a small arrow, in the commit's colour when the commit's own lane is
  hidden, and that commit's node, so no commit is ever invisible and none is
  drawn on a lane that is not its own. Labels that do not fit beside the
  lanes are left out whole and counted in a "+N" chip; a click on it opens a
  menu of them, each with its own menu (a branch's menu, or a tag's delete
  and copy). Each chip's width is measured with the window's text system,
  bold for the current branch and with its check mark, so the subject, date,
  and hash keep their room. With nothing selected the
  right side says to select a commit. Building the visible rows asks for the next page when the list
  comes within 600 rows of the end. A refresh reads the graph again only when
  HEAD, a branch, or a tag moved (`graph::fingerprint`), reads as many commits
  as were shown (all of them when the whole history was), and swaps the list in
  one step; the selection follows its commit by hash, and the rows that were at
  the top go back to the top, unless the list was already at the top, where new
  commits then show. A tab that is not shown is marked stale and read when it is.
- **Labels and filter.** `git_core::graph::ref_labels` reads full decorations:
  the current branch (emphasized, with a check mark), other local branches,
  remote branches, tags, and a detached `HEAD`. A remote's symbolic `HEAD`, the
  stash, and other namespaces carry none. The filter (all branches, local
  branches, current branch; remote branches and tags on or off) is remembered
  in `git.json` (`graph_scope`, `graph_remotes`, `graph_tags`).
- **The same commit actions as History.** Selecting a row shows the commit's
  header, files, and diff on the right, as History does. History and the graph
  share one commit menu (`commit_menu.rs`): undo and amend on HEAD's commit,
  reset to, check out, revert, cherry-pick onto a branch, create branch and tag
  here, delete and copy each tag, copy the hash, and view on GitHub. In the
  graph, reset to and revert are offered only for HEAD and its ancestors
  (`GraphView::is_ancestor_of`, a walk of parents through what is loaded, which
  in topological order holds every descendant of a loaded commit).
  Right-clicking a branch chip opens the branch menu. Dragging a commit onto a
  local branch chip, or onto the branch strip that appears at the foot while
  dragging (at most 152px tall, scrolled within), cherry-picks it there. On the
  current branch's unpushed, merge-free commits, dropping on another commit asks
  to squash and dropping on its top edge reorders; both go through History's
  rules and dialogs by mapping the commits to History's list, which the Graph
  tab loads for that. A drop those rules refuse says why in a warning bar (not
  on the current branch's line, already pushed, or a merge between), and the
  hint under the list states the rule.
- **Find.** Ctrl+F (or the search button) opens a find field while the Graph
  tab is shown, from anywhere in the window (the list, the filter, a check
  box, the diff) except another text field. Enter and the
  arrows step to the next or previous commit whose subject or author contains
  the text or whose hash starts with it (four hex digits or more), select it,
  and scroll to it. Looking forward, pages not loaded yet are read until a
  match turns up, and only at the end of the history does the find wrap. The
  field closes with Escape or its Done button, set apart from the field's own
  clear button.
- **Controls.** `better_ui::fluent::lanes::LaneCell` draws a row's lines (cubic
  curves that leave and arrive vertically between columns, straight lines in a
  column) and its node (dot, smaller dot for a merge, ring for HEAD), coloured
  from `ChartHue::SEQUENCE` (the five resource hues; the alarm red and the
  NPU magenta are never lane colours), and clips to a number of visible lanes with the
  edge marker and pinned node (`lanes::clipping`);
  `better_ui::fluent::chip::Chip` is the label chip.
  Both have unit tests for their geometry and colours and are on the gallery's
  Lists and tables page.

### Figures

Generated repositories of 100,000 commits with a branch merged back every 50
commits (the `git-platform` one also keeps all 2,000 of those branches and 100
tags as refs), no commit-graph file, release build, on the development machine
while other worktrees were building. The load average is given with each run,
because the figures move with it.

| Measured | Session (`crates/git-gui/tests/graph.rs`), load 8–9 | `git-platform` only (`crates/git-platform/tests/graph.rs`), load 9–17 |
| --- | --- | --- |
| First 3,000 rows read and laid out, off the window's thread | 482 ms | 744 ms |
| Every one of the 100,000 commits read and laid out | 724 ms | 1.14 s |
| Slowest later page, read off the window's thread | 8.4 ms | 38.7 ms |
| Slowest page added on the window's thread | 7.5 ms | — |
| Lanes rebuilt for 30 rows at row 77,777 (one screen) | 6.6 µs | — |
| Find across 100,000 loaded commits, on the window's thread | 3.6 ms | — |
| After a commit: all 100,001 commits read again (background) | 696 ms | — |
| …and swapped in on the window's thread | 57 µs | — |

Earlier runs of the same tests measured the first page at 279 ms (load 23)
and at 935 ms (load near 40). Almost all of it is git's own topological walk:
without a commit-graph file git reads every commit before printing the first.
With one (`git commit-graph write`), git printed its first page in 16 ms on a
similar generated history. Scrolling builds only the rows on screen, so its
cost does not grow with the history. Run the figures with
`cargo test -p git-gui --test graph --release -- --ignored --nocapture` and
`cargo test -p git-platform --test graph --release -- --ignored --nocapture`.

### Limits

- Lane colours are lane numbers in the order lanes open, so a refresh that adds
  a new branch tip above the others can give the existing lanes different
  colours. Within one reading a lane keeps its colour.
- The find field's placeholder follows the language the window opened in; a
  language changed in Settings shows on it after a restart.
