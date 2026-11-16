# Ticket 142 — A Better OS appearance component: every window has the same four rounded corners

**Blocked by:** owner decision on the compositor part (see below); ticket 141
for Better OS's own windows

## Why

The owner noticed on 2026-11-16 that some applications round all four
window corners and others only the top two (Better OS's own windows included),
and asked that every window have all four corners rounded with one radius.
Any theme doing this must ship as part of Better OS — installed, updated and
removed by Better Manager like every component — not as a one-off change to
the machine.

## Who draws a window's corners

On GNOME 46 under Wayland every application draws its own frame:

- GTK 4 / libadwaita: all four corners rounded (theme radius).
- GTK 3: top rounded, bottom square by default (theme CSS decides).
- Better OS (GPUI, `better_ui::system_frame`): top measured from GTK, bottom
  square (ticket 141 fixes this).
- Firefox, Electron (Chromium), Qt: their own frames, out of reach of any GTK
  theme.

So a theme alone cannot make every window the same; only the compositor sees
every window.

## What to build

1. **`better-appearance` component** (new package and manifest): a GTK 3 and
   GTK 4 theme layer that sets one corner radius on all four corners of CSD
   windows (and keeps square corners when maximized/tiled/fullscreen), built
   on top of the person's current theme rather than replacing its colours.
   Installing applies it through manager-core (the person's previous theme
   settings are captured and restored on removal, ticket 85's rules), and the
   radius is one setting. Better OS windows read the same radius.
2. **Compositor rounding for everything else** — Firefox, Electron, Qt —
   which needs GNOME Shell to clip each window to the radius. That is shell
   code: the GJS exception (ADR 0012, AGENTS.md) forbids an extension holding
   a value or deciding anything, so this needs a new ADR the owner approves
   (a relay that applies a clip the Rust service decides, or a different
   route), plus measurements of the shader's cost on the owner's machine.
3. One radius everywhere: the GTK layer, Better OS windows, and the
   compositor clip all take it from the component's single setting.

## Done when

- Screens of a GTK 4, a GTK 3, a Better OS, a Firefox and an Electron window
  side by side, light and dark, showing the same four corners.
- Removing the component puts every window back exactly as before.
