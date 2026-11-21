# Ticket 300 — One StatusNotifierItem implementation for every Better OS tray item

**Spec:** AGENTS.md ("A rule that two places need lives in one place both call")
**Blocked by:** none

## Why

Three crates now serve `org.kde.StatusNotifierItem` and register with
`org.kde.StatusNotifierWatcher`, each written on its own:

- Better Awake's tray: `crates/awake-tray/src/item.rs` (the item interface,
  line 28) and `crates/awake-tray/src/sni.rs` (the watcher proxy, line 55;
  registering again whenever a watcher appears, line 170), with its
  `dbusmenu.rs`;
- Better Monitor's tray: `crates/monitor-tray/src/lib.rs` (the item
  interface, line 497; the watcher proxy, line 803), with its menu;
- Better Screenshot's recording item: `crates/screenshot-gui/src/service.rs`
  (the item interface, line 256; registration, line 430), which registers
  once and does not register again when a watcher appears later.

A fix or an addition to one (a watcher that appears after the item, the
tool-tip structure, `NewStatus`) does not reach the others, and the third
copy already lacks the first copy's re-registration.

## What to build

- A shared module in a crate the three already depend on or can depend on
  without GPUI (a new `better-sni` crate, or `better-core` if its no-GPUI,
  no-network rules allow zbus there): the item interface built from a
  description (id, category, title, icon name, tool tip, status), the
  watcher proxy, registration that repeats whenever a watcher owner
  appears, and the dbusmenu interface for the trays that have a menu.
- The three crates use it, each keeping only what its item shows and does.
- Tests on a private bus: registration with a watcher present, a watcher
  that appears later, a title change emitting `NewTitle`, `Activate`
  reaching the owner.

## Done when

- [ ] One implementation; the three crates call it; no other
      `#[interface(name = "org.kde.StatusNotifierItem")]` in the workspace
      (a test greps for it, as `better-core/tests/one_guard_reading.rs`
      does for its rule).
- [ ] The private-bus tests pass; fmt, clippy, and the crates' tests pass.
