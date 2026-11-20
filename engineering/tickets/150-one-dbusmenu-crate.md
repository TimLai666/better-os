# Ticket 150 — One `com.canonical.dbusmenu` crate for the trays and the top bar's tray host

**Blocked by:** none

## Why

`com.canonical.dbusmenu` is how a StatusNotifierItem hands its menu to a tray
host. Better OS has two copies of the serving side today:
`crates/awake-tray/src/dbusmenu.rs` and the `#[interface(name =
"com.canonical.dbusmenu")]` block in `crates/monitor-tray/src/lib.rs`. The top
bar adds the reading side: its tray host shows every tray item's menu
(`design/top-bar-spec.md` sections 5 and 6.1), Better OS's own trays' among
them. The two sides share the protocol's tree, property names, revision
rules, and mnemonic handling, so a third copy would follow, and AGENTS.md
says a rule two places need lives in one place.

Nothing else uses it. The bar exports no menus and no Better OS window does
(decision D5, owner, 2026-11-21): the bar serves no `com.canonical.dbusmenu`
and owns no `com.canonical.AppMenu.Registrar`. If one crate turns out to give
the client nothing it shares with the servers, fold the client into ticket 156
and keep this ticket to the two trays.

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
    `AboutToShow`, and gives up on a call after 500 ms (spec section 5);
  - mnemonic handling (`_Quit`) shared by both ends.
- `awake-tray` and `monitor-tray` move onto the server with no change on the
  bus: their existing session-bus tests and ticket 102's replayed AppIndicator
  host test must pass unchanged.

## Done when

- [ ] One implementation; a test greps the workspace and finds no other
      `com.canonical.dbusmenu` interface definition.
- [ ] Server and client tested against each other on a private bus, including
      a layout change while a submenu is open, ids staying stable, and the
      500 ms give-up.
- [ ] Both trays' existing tests pass unchanged; `zbus` stays on its default
      flavor (`manager-platform/src/flavor.rs`).
- [ ] fmt, clippy on stable, the crates' tests, and the license inventory
      check pass.
