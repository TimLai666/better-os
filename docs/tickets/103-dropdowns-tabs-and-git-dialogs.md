# Ticket 103 — Dropdowns close on a second click, tabs look like tabs, and Better Git's dialogs say what is wrong

**Blocked by:** none · **Status:** ready

## Why

The owner reported on 2026-11-14, using the 0.2.12 apps:

- In every app, a dropdown that is open does not close when its own button
  is clicked again; only a click somewhere else closes it.
- The controls that switch between tabs look like buttons.
- In Better Git, "New repository" does nothing when clicked.
- Whether Better Git can find all of a repository's worktrees.

Reproduced the same day with the installed Better Git 0.2.12 in a sandbox,
with HOME and XDG directories in a temporary folder:

- **Second click.** Clicking "Current repository" opens its flyout; clicking
  it again leaves the flyout open. The kit's light-dismiss panels close on
  `on_mouse_down_out` (`better-ui` `fluent/dialog.rs` `Flyout`,
  `fluent/selection.rs` combo list, `fluent/menu.rs` menu,
  `fluent/navigation.rs` floating pane). A press on the control that opened
  the panel is "outside" the panel, so the panel closes on mouse down, and
  the control's click handler then opens it again.
- **New repository.** The dialog opens. Pressing "Create" with an empty or
  otherwise invalid name does nothing at all: `confirm_dialog`
  (`git-gui/src/app.rs`) returns silently when `dialogs::validate` finds a
  problem, and the dialog never shows the problem. With no Git identity
  configured, creating fails with Git's "Author identity unknown"; the
  error bar shows Git's raw text and tells the person to run
  `git config --global`, a terminal command.
- **Worktrees.** Repository › Worktrees… (Ctrl+Alt+W) lists every worktree
  of the open repository and opens, renames, removes, or adds one. The
  repository list and the branch list do not show them: a branch checked
  out in another worktree is listed as an ordinary branch, and switching to
  it fails in Git.
- **Amend.** The owner, 2026-11-14: choosing "Amend last commit" does not
  put the last commit's message into the commit form; it appears only after
  "Fetch origin". `Session::amend_last` (`git-gui/src/session.rs`) fills
  the form and bumps its generation, but `GitApp::act`/`act_with`
  (`git-gui/src/app.rs`) call `drive` and `cx.notify` without
  `after_change`, and `sync_form`, which copies the form into the text
  fields, only runs from `after_change` after a background job finishes.
  When the history has not been loaded yet, `amend_last` also returns early
  without setting `amend`, and the checkbox handler only switches tabs to
  load it, so nothing fills the form when the history arrives.
- **"Fetch: done" banner.** The owner, 2026-11-14: after a fetch, a green
  "擷取：完成" bar stays at the top of the window until it is closed, and
  is not needed, since the toolbar's fetch button already shows the result
  ("上次擷取：剛才"). It is `Notice::Done(kind)` (`git-gui/src/session.rs`
  around lines 2711 and 2765), drawn by `render.rs` as a Success `InfoBar`
  with a close button and no timeout, after every operation kind.
- **Clicks on the blank part of an item.** The owner, 2026-11-14: the
  "New repository" click that did nothing was on the blank part of the item,
  which was highlighted under the pointer; a click on the words works. The
  owner asks that every app be checked: the whole highlighted item is the
  click target, never only its text. In the sandbox the same day, clicks on
  the blank parts of Better Git's three "New repository…" entries (the
  welcome card, File menu, and the repository flyout's Add menu) all opened
  the dialog, so the failing item is another one; the audit below finds it.
- **Uncommitted changes not picked up.** The owner, 2026-11-14: Better Git
  does not seem to see uncommitted changes. The status read itself is
  complete (`git status --porcelain=v2 -z --branch --show-stash
  --untracked-files=all`, `git-platform/src/lib.rs` around line 1067), but
  the only thing that re-reads it is the window becoming active
  (`observe_window_activation` in `git-gui/src/app.rs` around line 275, then
  `Session::refresh`). A file saved in an editor while Better Git stays
  visible and unfocused beside it, or saved without Better Git ever losing
  focus (a terminal command, a build), never shows up until the window is
  activated again or some job such as a fetch finishes.
- **Tabs.** Better Git's "Changes / History" and the branch flyout's
  "Branches / Pull requests" use `SelectorBar` and look like tabs in the
  sandbox. The owner's report covers all apps, so the controls that switch
  views elsewhere (Better Touchpad's segmented buttons, any row of buttons
  used as tabs in Better Awake, Better Files, Better Manager's interim pages)
  are the likely ones.

## What to build

**The kit (`better-ui` `fluent`):**

- Every light-dismiss panel (flyout, combo box list, menu flyout, split and
  dropdown button menus, the floating navigation pane, the auto-suggest
  list) closes when its own control is pressed again, and opens again only
  on the next press. Fix it once in the kit (for example the control
  records that the panel was dismissed by the same pointer press and its
  click handler then does not reopen it), with a unit test per control.
- Every app-specific popover that does not use the kit's panels gets the
  same behaviour (`grep` every `on_mouse_down_out` in `crates/`).
- A row that switches views is a `SelectorBar` (or `TabView` for closable
  document tabs, `NavigationView` for pages), never buttons. Audit every app
  and replace button rows used as tabs. A segmented choice of a value
  (Better Touchpad's speed presets) stays a value control, but must not be
  confused with tabs: if it switches what the page shows, it is a
  `SelectorBar`.

- **The highlighted area is the click area, in every app.** Audit every
  element in every app crate (and `better-ui`) that changes its look under
  the pointer (`.hover(`, `on_hover`, a highlight state) or reacts to a
  click (`on_click`, `on_mouse_down`, `on_mouse_up`): the handler sits on
  the same element that draws the highlight, that element fills the row
  (no click target that is only the label, an icon, or a child with its own
  width), and no child that occludes or stops propagation leaves a dead
  spot inside it. Add a kit test helper that clicks a row at its left edge,
  centre, right edge, and the gap between label and trailing text, and use
  it for every kit row (menu items, list and table rows, navigation items,
  settings cards, combo items, breadcrumb items, selector items, tabs).
  List every place fixed, per app, in What was built.

**Better Git:**

- A dialog's primary button is disabled while the input is invalid, and the
  reason is shown under the field it concerns (`InfoBar` or field message),
  in both languages. Enter in a field does the same.
- When Git has no identity, Better Git asks for a name and email the first
  time it would commit (GitHub Desktop's "Configure Git" step), prefilled
  from the signed-in GitHub account, and writes them with
  `git config --global` itself after showing what it will write. It never
  tells the person to type a command.
- Amend: the message and co-authors appear in the form at once, from the
  checkbox, the History menu, and the Repository menu. Every session change
  made by a click or key reaches the text fields in the same frame (the
  action path runs `after_change`, not only finished jobs). When the history
  is not loaded yet, the form says it is reading the last commit, reads it
  directly (`git log -1`), and fills in when it arrives; amend stays ticked.
  Tests for each entry point.
- No "done" banners. An operation whose result the window already shows
  (fetch, pull, push, commit, switch, merge, rebase, stash, tag, branch and
  worktree changes, discard) shows no bar when it succeeds; the toolbar
  button, the lists, and the history are the confirmation, as in GitHub
  Desktop. A notice that carries something the window does not show (a
  pull request or issue created with its link, published to a URL, forked,
  a stash waiting, checks re-run) appears as a short-lived toast that hides
  itself after about 6 seconds, stays while the pointer is on it, and keeps
  its link button. Errors keep their bar until closed.
- Live changes: the open repository's working tree and its `.git` (HEAD,
  index, refs, the operation-in-progress files) are watched with the
  `notify` crate already in the workspace (as `files-platform/src/watch.rs`
  does), and a change triggers a debounced status refresh (about 300 ms)
  that keeps the selection and scroll position. Directories Git ignores
  (`node_modules`, `target`, and the rest of `git check-ignore`) are not
  watched, so a large project does not exhaust inotify watches; if a watch
  cannot be added (limit reached, a network filesystem), Better Git falls
  back to refreshing every 3 seconds while the window is visible and says
  so once in the status area. Activation still refreshes. The refresh never
  overlaps a running Git operation and never runs more than one at a time.
  Tests: a file created, edited, deleted, and staged from outside shows up
  without any click; an ignored directory's churn causes no refresh.
- Shortcut buttons that are always there (owner, 2026-11-14, with a
  screenshot of GitHub Desktop's "No local changes" page): "Open in
  {editor}" (Ctrl+Shift+A), "Show in Better Files" (Ctrl+Shift+F), "Open in
  Terminal" (Ctrl+`), and "View on GitHub" (Ctrl+Shift+G, only for a
  repository hosted on GitHub or GitHub Enterprise) sit in the toolbar's
  right side, after Fetch/Push, whenever a repository is open, with or
  without changes. Each has its icon and label; below about 1200px window
  width they become icon buttons with tooltips naming the action and its
  shortcut. The editor's name comes from Settings (as GitHub Desktop's
  "Open in GNOME Text Editor"). The existing "No local changes" page keeps
  its suggestion cards as well.
- Worktrees: the repository list shows each repository's linked worktrees
  under it (path and branch), and choosing one opens it. In the branch list
  a branch checked out in another worktree says so ("Open in its worktree")
  and choosing it opens that worktree instead of failing.

## Acceptance

- [ ] Each kit control: open with a click, close with a second click on the
      same control, open again with a third; screenshots in the sandbox.
- [ ] Every app's view-switching rows are tab controls; before and after
      screenshots per app.
- [ ] Better Git: "Create" with an empty name, a name that exists, and a
      path that is not empty each show their reason; with no identity the
      identity step appears and the repository is created afterwards.
- [ ] Better Git: ticking "Amend last commit" right after opening a
      repository fills the summary and description in the same moment;
      screenshot before any fetch.
- [ ] Better Git: with the window open and untouched, a file edited from a
      shell appears in Changes within a second; screenshots before and after.
- [ ] Better Git: a repository with two linked worktrees shows them in the
      repository list and marks their branches in the branch list.
- [ ] Tests never touch the person's real HOME, `~/.gitconfig`, or keyring.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
