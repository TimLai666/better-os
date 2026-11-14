# Ticket 118 — Better Files' sidebar can be made wider or narrower

**Blocked by:** none · **Status:** open

## Why

The owner asked on 2026-11-14 to set the sidebar's width themselves. It is
fixed at 220 px (`crates/files-gui/src/render.rs` ~686), which cuts long
folder and device names short.

## What to build

- The edge between the sidebar and the content can be dragged: a hit area
  wider than the visible divider, a resize cursor, a highlighted divider while
  hovered or dragged, as in Windows 11 File Explorer.
- The width has a minimum (labels and icons still readable) and a maximum
  (the content keeps a usable width); the window's minimum size is respected.
- Double-clicking the divider returns to the default width.
- The width is remembered with Better Files' other preferences, per person,
  shared by every window, and restored after a restart and after the
  update handoff (ticket 105).
- The preview pane's edge gets the same treatment if it is fixed today.
- Keyboard: the divider is reachable and arrow keys resize it, if Better
  Files' other controls are keyboard-reachable in the same way.
- Check the other Better OS apps with a navigation pane (Manager, Monitor,
  Git's changes list) and list in the ticket which are fixed-width, without
  changing them here.

## Acceptance

- [ ] Unit tests: clamping, the stored value, an unreadable stored value
      falling back to the default, double-click reset.
- [ ] Sandbox evidence: drag wider, drag narrower to the minimum, restart
      keeps it; screenshots at narrow, default and wide.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
