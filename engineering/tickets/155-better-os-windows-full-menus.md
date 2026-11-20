# Ticket 155 — Better Files, Better Monitor, and Better Manager hand their full menus to the bar

**Blocked by:** 154

## Why

`design/top-bar-spec.md` section 5.2: every Better OS window that has
commands shows them as menus, and every command is in exactly one menu with
its shortcut. Ticket 154 builds the model and moves Better Git; this ticket
does the other windows.

## What to build

- **Better Files**: File, Edit, View, Go, Window, Help, holding every command
  of its command bar, context menus, and key map (spec 5.2), on `better-ui`'s
  menu model; an in-window menu row drawn from the same model when the bar is
  not there.
- **Better Monitor**: File (Export…), Edit, View (its views and grouping),
  Window, Help.
- **Better Manager**: File, Edit, View (one item per page, the same names and
  order as its navigation), Window, Help.
- Better Launcher and the app chooser register nothing (spec 5.2).
- Each window's every-command-in-one-menu test, in its crate.

## Done when

- [ ] The three tests pass and list the commands they found.
- [ ] In the headless GNOME Shell with the bar: each window's menus appear,
      a command from each menu works from the bar, and each window shows its
      own row when the bar is off. Screenshots, light and dark, in
      `/home/tim/Developer/better-os-evidence/155/`, Better Files compared with
      `design/top-bar/rest-betteros-*.html` and `app-menu-*.html`.
- [ ] fmt, clippy on stable, and the crates' tests pass.
