# Ticket 154 — `better-ui` exports a window's menus, and Better Git uses it

**Blocked by:** 150, 153

## Why

GPUI has no Linux menu export (`design/top-bar-spec.md` section 9.6), and
Better OS windows draw their own menu rows. For the bar to show a Better OS
window's full menus, the menus must come from one model that both the
in-window row and the export read.

## What to build

- **The menu model** in `better-ui`'s GPUI-free part: menus, items (label,
  shortcut, enabled, checked, radio group, submenu, separator) and an action
  id per item, built by the window from its own state. `fluent::menu`'s
  in-window menu row draws from it.
- **The export**: a window process serves its model with the `dbusmenu` crate
  (ticket 150), registers it with `org.betteros.TopBar1.Menus.Register` from
  its session connection, follows which of its windows is active, and turns
  `Event("clicked")` into the same action the row or key performs.
- **The row while the bar shows the menus** (decision D5, recommendation (a)):
  hidden once the bar has accepted the registration; shown again when the bar
  is off, missing, or has not accepted within one second, and when the bar's
  name leaves the bus. If the owner chooses (b), the row stays and nothing else
  in this ticket changes.
- **The standard menus** (spec section 5.2): the application menu (About,
  Settings…, Quit), File, Edit, View, Window, Help with their fixed items and
  copy, in both languages, in `better-ui`, so every window gets them the same.
- **Better Git** moves its four menus (`crates/git-gui/src/render.rs:583-960`)
  onto the model unchanged and gains Edit, Window, and Help. A test asserts
  that every action in its key map and command bar is in exactly one menu.

## Done when

- [ ] Unit tests for the model, the export (a private bus: layout, property
      updates while a menu is open, click to action), the hide-and-show rule
      for the row, and Better Git's every-command-in-one-menu test.
- [ ] In the headless GNOME Shell with the bar (ticket 153): Better Git's menus
      appear in the bar, its row hides, an item works from the bar, and
      stopping the bar brings the row back within a second. Screenshots in
      `/home/tim/Developer/better-os-evidence/154/`.
- [ ] The gallery shows the in-window row drawn from the model.
- [ ] fmt, clippy on stable, and the crates' tests pass.
