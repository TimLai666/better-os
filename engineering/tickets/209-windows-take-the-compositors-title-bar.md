# Ticket 209 — Better OS windows take the compositor's title bar and the session's one corner radius

**Spec:** better-os-session.md S5 (Better OS windows row; C5)
**Blocked by:** 200

## Why

C5: in the session Better OS windows use `cosmic-comp`'s server-side title
bar, so they match every other window. Kept as on GNOME, the GTK frame drew a
close button only, because COSMIC's GTK `button-layout` is `'appmenu:close'`
(ADR 0038, screenshot 02). The compositor already clips every floating and
tiled window to the theme's small radius (ADR 0038), which is ticket 142's
item 2 in the session.

## What to build

- **In `better-ui`**: in the Better OS session (`better_core::desktop`), open
  windows with `WindowDecorations::Server` and do not load
  `better_ui::system_frame`; `fluent::chrome::TitleBar` draws no window
  buttons and no drag region there and, where a window puts content in the
  title bar (tabs, search), keeps that content as an ordinary row under the
  compositor's title bar. Under GNOME nothing changes. A compositor that
  refuses server-side decoration (`zxdg_decoration_manager_v1` absent or
  answering client-side) falls back to today's frame, logged.
- **The radius**: the session's theme defaults (ticket 203) set COSMIC's
  `corner_radii` so `radius_s` is ticket 148's window radius; Better OS
  windows draw their own content to the same radius.
- Every window's `window_chrome::root` and `TitleBar` use checked: one test
  per GUI crate that in the session nothing draws window buttons.

## Done when

- [ ] Unit tests for the decoration choice per session and the fallback.
- [ ] In ticket 200's nested compositor or ticket 201's VM: Better Files,
      Git, Monitor, Manager, and Launcher with COSMIC's title bar (minimize,
      maximize, close), all four corners at the one radius, light and dark,
      next to a GTK 4, a GTK 3, Firefox, and an Electron window. Screenshots in
      `/home/tim/Developer/better-os-evidence/209/`.
- [ ] fmt, clippy on stable, and the crates' tests pass.
