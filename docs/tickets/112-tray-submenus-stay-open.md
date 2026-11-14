# Ticket 112 — Tray submenus stay open

**Blocked by:** none · **Status:** open

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

- [ ] Reproduced in a headless GNOME Shell with the AppIndicator extension
      before the fix: a submenu of each tray that has one collapses within a
      reading interval; screenshots or a recording frame sequence in `.shots/`.
- [ ] After the fix, the submenu stays open for at least 30 seconds while the
      readings or countdown keep changing, and the changed lines update in
      place.
- [ ] Unit tests: a reading change produces a properties update and no layout
      update; a structural change produces a layout update; a layout change
      while a submenu is open is deferred until it closes.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
