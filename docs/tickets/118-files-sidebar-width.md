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

- [x] Unit tests: clamping, the stored value, an unreadable stored value
      falling back to the default, double-click reset.
- [x] Sandbox evidence: drag wider, drag narrower to the minimum, restart
      keeps it; screenshots at narrow, default and wide.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.

## What was built

- **The sidebar's edge drags.** An 8 px grab strip on the navigation pane's
  edge, a resize pointer, the divider in accent while hovered, dragged or
  focused. 320 px by default (WinUI's list pane), from 200 px to two fifths
  of the window as the person sees it, so the folder keeps the rest at the
  880 px minimum. Double-click, or Enter on the focused strip, goes back to
  the default and stores no width; Left and Right move it 16 px, as the
  window's other controls are keyboard stops. The width is
  `sidebar_width` in Better Files' preferences (per person, every window,
  restored after a restart and after the update hand-off, which re-reads the
  preferences), written once the drag ends. The compact pane stays 48 px.
- **The preview pane too.** It was fixed at 320 px and the application
  details in its place at 360 px; both now share one draggable width
  (`preview_width`), 240 px to a third of the window.
- **One mechanism.** `layout::PaneEdge` (clamp, store, reset) and
  `pane_edge`/`follow_edge` in `render.rs` serve the sidebar, the preview
  and ticket 119's file operations panel. The kit's `NavigationView` gained
  `open_width` and `edge`.
- **The command bar no longer runs off the window** (found in ticket 119):
  the window measured its surface, which includes the desktop frame's
  shadow, so at about 1,190 px the bar thought it had room for its last
  two buttons. The view now measures what the person sees, and the bar's
  separators are counted; the last buttons move behind "…".
- **Other windows with a navigation pane** (read from the code, not
  changed here): Better Manager, Better Monitor and Better Git use the
  kit's `NavigationView` at its fixed 320 px (48 px compact); Better Git's
  changes list sits in that pane, so it is fixed too.

Tests: `layout.rs` (clamping, stored and unreadable widths, reset at the
880 px minimum, the sidebar and preview bounds). Sandbox (headless GNOME
Shell, fake `tailscale` from the first launch), in `.shots/`:
`t118-sidebar-dragging-wider.png`, `t118-sidebar-wide.png` (460 px),
`t118-sidebar-narrowest.png` (200 px), `t118-sidebar-after-restart.png`
(200 px kept), `t118-sidebar-default.png` (double-click, 320 px, stored as
no width), `t118-command-bar-1186px.png` ("…" in place at 1,186 px).
