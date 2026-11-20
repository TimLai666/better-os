# Ticket 150 — One `com.canonical.dbusmenu` crate for trays, windows, and the top bar

**Blocked by:** none

## Why

`awake-tray` and `monitor-tray` each serve their own `com.canonical.dbusmenu`
(`crates/awake-tray/src/dbusmenu.rs`, `crates/monitor-tray/src/lib.rs`), and
the top bar (`design/top-bar-spec.md` sections 5, 6.1, 9.6) needs the same
protocol twice more: Better OS windows export their menus with it, and the bar
reads every application's and tray item's menu with it. AGENTS.md: a rule two
places need lives in one place, and a third copy is not allowed.

## What to build

- A crate `dbusmenu` with no GPUI dependency:
  - a menu tree type (id, label, enabled, visible, toggle type and state,
    shortcut, icon name, children, separators) and the revision bookkeeping;
  - a **server** on zbus serving `GetLayout`, `GetGroupProperties`,
    `GetProperty`, `Event`, `EventGroup`, `AboutToShow`, `AboutToShowGroup`,
    and emitting `LayoutUpdated` and `ItemsPropertiesUpdated`, keeping item ids
    stable across rebuilds (ticket 112's lesson: GNOME's AppIndicator caches
    ids);
  - a **client** that reads a remote tree with `GetLayout`, follows
    `LayoutUpdated` and `ItemsPropertiesUpdated`, sends `Event("clicked")` and
    `AboutToShow`, and gives up on a call after 500 ms (spec 5.3);
  - mnemonic handling (`_File`) shared by both ends.
- `awake-tray` and `monitor-tray` move onto it with no change on the bus:
  their existing session-bus tests and ticket 102's replayed AppIndicator host
  test must pass unchanged.

## Done when

- [ ] One implementation; a test greps the workspace and finds no other
      `com.canonical.dbusmenu` interface definition.
- [ ] Server and client tested against each other on a private bus, including
      a layout change while a submenu is open and ids staying stable.
- [ ] Both trays' existing tests pass unchanged; `zbus` stays on its default
      flavor (`manager-platform/src/flavor.rs`).
- [ ] fmt, clippy on stable, the crates' tests, and the license inventory
      check pass.
