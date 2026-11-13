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

**Better Git:**

- A dialog's primary button is disabled while the input is invalid, and the
  reason is shown under the field it concerns (`InfoBar` or field message),
  in both languages. Enter in a field does the same.
- When Git has no identity, Better Git asks for a name and email the first
  time it would commit (GitHub Desktop's "Configure Git" step), prefilled
  from the signed-in GitHub account, and writes them with
  `git config --global` itself after showing what it will write. It never
  tells the person to type a command.
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
- [ ] Better Git: a repository with two linked worktrees shows them in the
      repository list and marks their branches in the branch list.
- [ ] Tests never touch the person's real HOME, `~/.gitconfig`, or keyring.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
