# Ticket 103 — Dropdowns close on a second click, tabs look like tabs, and Better Git's dialogs say what is wrong

**Blocked by:** none · **Status:** merged, not released

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
  The owner then clarified: it is the dropdown menus and buttons in general,
  where only the text seems to react. A likely cause in the kit, from
  reading `fluent/menu.rs`: a menu row is drawn with `mx(4)` and `my(2)`
  margins inside the panel's padding, its highlight comes from the menu's
  `highlight` state set by `on_hover(true)` and never cleared on
  `on_hover(false)`, and the click handler is on the row only. A pointer in
  the 2–4px gaps between rows or at the panel's edge (4–8 physical pixels at
  the owner's 200% scale) still sees the last row highlighted, but the click
  lands on the panel and does nothing. Buttons and other controls get the
  same check.
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
  Menu and list rows: the hit area covers the full row height and width
  with no gaps between rows (the inset look is drawn inside the hit area),
  and a highlight is shown only while the pointer is actually over that
  row (cleared on hover-out), so what looks clickable is clickable. Test at
  scale 1 and 2.

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

## What was built

**The kit (`better-ui`).**

- **Second press closes.** `fluent/light_dismiss.rs` is the one fix: while a
  light-dismiss panel is open, `anchored_panel` draws an empty, occluding
  shield over the control the panel hangs from, so the press that closes the
  panel (`on_mouse_down_out`) never also becomes the control's click. The
  shield covers only that control; a press on any other control still closes
  the panel and reaches it. `Flyout`, `MenuFlyout` (and with it
  `SplitButton`, the command bar's More button, and the breadcrumb overflow),
  and the combo box list float through it. The floating navigation pane's ☰
  is inside the pane and already toggled it. `AutoSuggestBox` closes its list
  on a press in its field and opens it on the next (`field_pressed`).
- **A press in a panel opened from a panel is inside.** Every kit panel
  records itself as drawn (`panel_marker`), and a panel's press-outside
  handler ignores a press in a panel drawn after it (`pointer_in_child_panel`).
  This was the owner's "New repository does nothing": the repository
  flyout's Add menu reaches past the flyout's edge, a press there closed the
  flyout on mouse down, and the menu item was gone before its click. Submenus
  had the same dead zone.
- **`DropDownButton`** (new, `fluent/button.rs`): a button that only opens a
  menu, with a chevron, or `DropDownButton::more` (ellipsis, no chevron).
  Shown in the gallery.
- **The highlighted area is the click area.** `fluent/hit_area.rs`: a kit row
  is a hit area that fills the row edge to edge and meets the next row, with
  the inset shape drawn inside it and highlighted through the hit area's
  group (`hit_row`, `group_look`, `RowInset`). Moved onto it: menu and
  submenu items (were `mx 4`/`my 2` margins), combo items (`mx 5`/`my 2`),
  suggestions (`mx 4`/`my 2`), navigation items (`mx 4`/`my 2`), and
  `ListView` rows (a 2px gap). A menu item's highlight now clears when the
  pointer leaves it (`MenuOpen::pointer_left`), so no row stays highlighted
  while the pointer is between rows or outside the menu. `check_rows` is the
  click test helper: at scale 1 and 2 it presses each row at its left edge,
  centre, right edge, and the gap between label and trailing text, walks
  every physical pixel line of a stack, and checks the highlight lies inside
  the hit area; every kit row goes through it, and a row drawn the old way
  fails it.
- **`TextBox::error`**: the reason a value cannot be used, under the field,
  in the critical color after an error glyph (gallery: "With a reason").
- **`TabView`**: a middle click closes a closable tab; tabs that do not fit
  scroll sideways (the wheel scrolls them), the selected tab is scrolled into
  view when the selection changes, `+` stays pinned at the right, and the
  close and `+` buttons can have tooltips (`tooltips`).
- The breadcrumb's overflow menu hangs from the `…` item, so only that item
  is shielded while it is open.

**Every app, per app.**

- Better Git: list rows (`render::row`) are full-width hit areas with the
  fill inset 4px/1px inside, and the lists lost their 2px dead gaps
  (repository, branch, pull request, clone, branch picker, changes, history,
  stash, commit files, cherry-pick targets, welcome recents). Its title-bar
  menus, toolbar flyouts, and the Add split button get the shield.
- Better Manager: the component filter, the component page's Overview /
  Versions / Files tabs, and the activity filter were rows of buttons; they
  are `SelectorBar`s (`view_switcher`). The component card's "…" was
  gpui-component's popup menu, which reopened on a second press; it is the
  kit's `DropDownButton::more`.
- Better Files: the document tab strip was buttons; it is the kit's
  `TabView` (select, close, middle-click close, +, their tooltips), with
  "Reopen closed tab" beside it.
- Better Touchpad: the Gestures page's profile scope ("All touchpads" / one
  pad) switches what the page shows, so it is a `SelectorBar`; the value
  choices (acceleration, click method, shape, direction, fingers, keys) stay
  value controls.
- Better Monitor: the resource list's tiles were 2px apart with dead gaps;
  each tile is now a hit area with the tile drawn inside.
- Better Launcher, Better Awake, App Chooser: audited, nothing to change.
  Awake's button pairs and weekday toggles are value choices, not tabs; the
  launcher's appearance menu already sits in its own `relative` holder, so the
  shield covers only its button.

**Better Git.**

- **Dialogs say why.** `dialog_check.rs` decides whether the primary button
  may be pressed and under which field the reason goes; it adds what only
  the disk knows for a new repository (the folder is a repository already,
  has files, or is a file). The button is disabled, the reason is under its
  field in both languages (`phrases.rs`). "This can't be empty" waits until
  the field was typed in (or Enter was pressed). The check is kept until the
  dialog or its text changes, so the disk is not read on every frame.
- **Who is committing.** Before a commit the job asks
  `git var GIT_AUTHOR_IDENT`/`GIT_COMMITTER_IDENT` in the repository itself,
  so its own and `includeIf` settings count (`git-platform`
  `watching::identity_known`, Git's own strict rule). A new repository asks
  inside the new folder right after `git init`, and only when it will make a
  first commit; without an identity it takes away what it just made. With no
  identity the job does not run: Git's settings are read again, and only then
  does the identity step open, prefilled from Git, then the signed-in
  account's profile, then its no-reply address (`identity.rs`). A value Git
  already has is shown as kept and cannot be edited; only the missing ones
  are written, with `git config --global`, and then the waiting commit or
  creation runs. A commit refused for identity anyway opens the step instead
  of Git's text; one asked about after the person opened another repository
  is dropped. The account's profile is read again when a stored account is
  restored.
- **Amend at once.** `drive` now runs `after_change`, so any click or key
  that changes the session reaches the text fields in the same frame.
  `amend_last` fills from the history when it has been read; otherwise the
  form says "Reading the last commit…", amend stays ticked, and `git log -1`
  (`Op::LastCommit`) fills it. The checkbox, the History context menu, and a
  new Repository › "Amend commit…" item all call it.
- **No "done" banners.** `Notice::Done` is gone. What is left (pull request or
  issue created, published, forked, stash waiting, checks re-run, signed in,
  the watch fallback) is a toast at the bottom right (`toast.rs`) that hides
  about 6 s after it appears, never while the pointer is on it, and keeps its
  link or Restore button. Problems keep their bar.
- **Live changes.** `live.rs` watches the working tree one folder at a time,
  skipping what `git ls-files --ignored --directory` lists, plus this
  worktree's git directory and the shared refs (`git rev-parse --git-dir
  --git-common-dir`); only HEAD, the index, refs, `packed-refs`, and the
  operation files count, never locks or objects. A change asks for a refresh
  300 ms after things go quiet (at most 1 s while they do not), through the
  session's queue, so it never overlaps a Git operation. A thread gathers
  the events and asks `git check-ignore --stdin` once per batch, so an
  ignored file (a log, a new ignored folder) asks for nothing. A new folder
  or a changed `.gitignore` or `info/exclude` re-reads the plan; the old
  watch is dropped before the new one is made, and the watch timer stops
  while no repository is open. Every Git command runs with
  `GIT_OPTIONAL_LOCKS=0`, so a status never rewrites the index the window
  watches (with a file dated in the future it did, and each write started the
  next refresh). When a watch cannot be added the window refreshes every
  3 s and says so once in a toast. `live_window.rs` sets the watch up off the
  painting thread whenever the open repository changes.
- **Toolbar shortcuts.** Open in {editor} (the editor from Settings, by
  name), Show in Better Files, Open in Terminal, and View on GitHub (hosted
  repositories only) sit after Fetch/Push whenever a repository is open,
  with labels, or icons with "name (shortcut)" tooltips when the labels would
  not fit: the toolbar's width is estimated from the repository and branch
  names and the labels themselves (`shortcuts::compact`). The buttons give
  way before Settings, which is never pushed off.
- **Worktrees in the lists.** The repository list shows each repository's
  linked worktrees under it (branch, folder), read for the whole list in the
  background when the list opens; choosing one opens it. In the branch list a
  branch checked out in another worktree says "Open in its worktree" and
  choosing it opens that worktree (`worktree_links.rs`,
  `GitSession::switch_branch`).

**Dependencies.** `git-gui` now depends on `notify`, already in the workspace
for Better Files. `better-ui`'s tests use `gpui` with its `test-support`
feature (a dev-dependency only), which brings test-only crates (`proptest`
and what it needs) into `Cargo.lock`; nothing shipped links them.
`docs/third-party-licenses.md` was regenerated.

**Evidence.** GPUI interaction tests in `better-ui`
(`fluent/interaction_tests.rs`, GPUI's test window with simulated presses):
a flyout and a menu flyout open, close, and open again on their own control;
a press on the line between two menu items invokes one; a press in a menu,
and in a nested flyout, past the outer flyout's edge reaches its item. With
the shield or the child-panel check taken out, the matching tests fail. Unit
tests in `better-ui` (light dismiss per control, the hit
test at scale 1 and 2 for every kit row, menu highlight clearing, the
suggestion field) and `git-gui` (dialog reasons and fields, identity, toast
clock, notices, shortcuts, worktree links, the watch's filter and debounce,
polling); session tests against real Git in a temporary home (amend before
and after the history is read, commit and new repository without an
identity, a name Git already has kept, an empty new repository needing no
identity, a stale identity question dropped, worktrees, a real watch seeing
a create, edit, stage, and delete while an ignored `target/`, a new ignored
`build/`, and an ignored log churn unseen, and a refresh that never writes
the index it watches when a file is dated in the future); and
`git-platform` tests for `identity_known`, `git_dirs`,
`ignored_directories`, and `ignored_paths`. Sandbox screenshots of each change are in the
worktree's `.shots/` (not committed).

**Not done or not seen.** The command bar's More menu and the breadcrumb
overflow use the same shielded `MenuFlyout` but were not pressed in the
sandbox. The toast was not seen on screen, since nothing in the sandbox
produces one without GitHub. "Refresh every 3 s while the window is
visible": GPUI does not report visibility, so the fallback refreshes every
3 s while the window is open. The tab strip's wheel scrolling was not tried
on screen. Seen in passing and not changed: in a narrow window the commit
button's "Commit to {branch}" label runs past the changes pane when the
branch name is long.
