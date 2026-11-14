# Ticket 112 — Tray submenus stay open

**Blocked by:** none · **Status:** merged, not released

## Why

The owner reported on 2026-11-14 that in the menus of Better OS tray icons, a
submenu unfolds and immediately folds back up, every time.

Confirmed cause for Better Monitor's tray: the menu carries the live reading
lines (`crates/monitor-tray/src/menu.rs` `build`, ~629-650), so every new
reading makes `Tray::redraw` (`crates/monitor-tray/src/lib.rs` ~86-112) see a
different menu, bump the revision and emit `LayoutUpdated`
(`announce`, ~185-189). GNOME's AppIndicator extension rebuilds the whole menu
on `LayoutUpdated`, which closes any open submenu under the pointer.

Better Awake's tray already avoids refreshing on a submenu's `AboutToShow`
(`crates/awake-tray/src/dbusmenu.rs` ~226-246) and sends
`ItemsPropertiesUpdated` when only wording changes, but whether its countdown
or rule updates still collapse a submenu has not been checked. Project memory
`gnome-appindicator-caches-dbusmenu-ids` applies: the extension caches item
ids.

## What to build

- In every Better OS tray (Monitor, Awake, and any other dbusmenu exporter in
  the workspace), a change that keeps the menu's structure sends
  `ItemsPropertiesUpdated` for the changed items only, never `LayoutUpdated`.
- `LayoutUpdated` is sent only when items are added, removed or reordered, and
  never while the host has a submenu open if it can be deferred (track
  `opened`/`closed` events or `AboutToShow` per id) — send it once the menu
  closes.
- Item ids stay stable for the same item across rebuilds.

## Acceptance

- [x] Reproduced in a headless GNOME Shell with the AppIndicator extension
      before the fix: a submenu of each tray that has one collapses within a
      reading interval; screenshots or a recording frame sequence in `.shots/`.
- [x] After the fix, the submenu stays open for at least 30 seconds while the
      readings or countdown keep changing, and the changed lines update in
      place.
- [x] Unit tests: a reading change produces a properties update and no layout
      update; a structural change produces a layout update; a layout change
      while a submenu is open is deferred until it closes.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was found

The extension (`zorin-appindicator@zorinos.com/dbusMenu.js`) rebuilds a row
whenever it is told a `type` or `children-display` property, changed or not
(`_onPropertyChanged` calls `_replaceSelf`). On `LayoutUpdated` it reads
exactly those two properties for every item, so every submenu row is rebuilt
closed. It also keeps the labels it has for ids it already knows, so before
the fix the Monitor menu's readings stopped changing while the menu was open.

- Better Monitor sent `LayoutUpdated` for every new reading. Its items now have
  an id per role, a new reading is an `ItemsPropertiesUpdated` naming only the
  properties that changed, and a reading that appears or goes away is a layout
  change held while a submenu is open: the rows the panel has keep showing new
  values, and the layout follows at the first poll at least 300 ms after the
  submenu closes, or at once when the whole menu opens or closes.
- Better Awake already sent wording changes as properties, but the whole set,
  `type` included; it now sends only what changed. Its layout changes (a
  session ending, a second reason, a rule starting) collapsed an open submenu;
  they are now held while a submenu is open and sent 300 ms after the last one
  closes, or at once when the whole menu opens or closes. While one is held,
  the rows the panel has stay, a row whose action the new menu no longer
  offers (extending a session that ended) is greyed out, and choosing one that
  raced the greying says why in a notification instead of doing nothing.
  Rows take the new wording only where the two layouts line up one to one:
  at the same level, from the first row on, while each pair is the same kind
  of row with the same action, or for a row with no action the same field
  name before its colon. Past the first pair that does not line up (a second
  reason pushing the rows below it down), the rows keep what they said until
  the layout goes out. Awake's item ids stay stable only while the menu's
  structure is unchanged: a new structure comes under new ids, because its
  items have no identity across layouts.

Which submenus are open is taken from the host's `opened` and `closed`
events, put back in order by the D-Bus serial of each call (the handlers run
on separate tasks and can finish out of order). A submenu stops counting as
open five minutes after it last said it opened, since a row the host destroys
never says it closed and a host may never send the whole menu's events. A
call from another connection than the last, or a new watcher appearing (the
panel restarted), starts again with nothing open and sends a held layout.

Checked in a headless GNOME Shell 46 with the extension, against a fake Awake
service and a sandboxed Monitor service: both submenus stayed open for over 30
seconds with the lines inside and above them changing; before the fix both
folded up within a second of the first change.
