# Ticket 321 — Better Monitor's application row tooltip covers its context menu

**Blocked by:** none

## Why

Found on 2026-11-22 while taking ticket 272's screenshots. On Better
Monitor's Processes page, right-clicking an application row opens the row's
context menu at the pointer, and the row's own tooltip (the application's
name and why its processes are grouped, "Grouped together because they run
in the same system service…") opens at the same moment, on top of the
menu's first items (End task, Force stop). The person cannot read or reach
the items under it until the pointer moves. A tooltip should not show while
a menu from the same row is open, or should sit where it covers nothing the
menu offers.

## What to build

- Whichever owns the rule (the row's tooltip in `monitor-gui`'s tables, or
  the kit's tooltip and menu) closes or suppresses a row's tooltip while a
  context menu is open, and does not open it again until the menu closes;
  if the kit is the right place, every window's list rows get it.

## Done when

- A test shows the tooltip is not drawn while the row's menu is open; a
  sandbox screenshot of the right-click on an application row of stand-in
  processes shows the menu uncovered, light and dark.
