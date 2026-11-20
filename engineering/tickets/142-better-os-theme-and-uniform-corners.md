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
3. **A Better OS taskbar** (owner, 2026-11-16): so Better OS looks the same
   on Ubuntu and other distributions and does not depend on Zorin. Today the
   taskbar is Zorin's own `zorin-taskbar@zorinos.com` extension (Ubuntu ships
   Ubuntu Dock instead). On GNOME under Wayland a panel can only be drawn by
   GNOME Shell itself (Mutter has no layer-shell for an outside process), so
   this is shell code too: same ADR question as item 2, and it must be
   designed together with ticket 95's top menu bar so the two do not
   overlap. Owner's direction (2026-11-16) for the left end of the taskbar:
   keep the "Show Applications" button (GNOME's application grid, which the
   owner likes) and add a Better OS button with the Better OS mark (no Zorin
   logo) that opens Better Launcher anchored to it, like ticket 82's
   Start-menu panel. Two shapes are open for the owner to pick at design
   time: (a) the Better OS button sits beside Show Applications; (b) it
   replaces the Zorin start button. (b) needs Launcher complete as a start
   menu; power, restart, lock and log out then live in ticket 95's top bar
   Better OS menu, not in Launcher. The Launcher stays its own Rust window
   that the shell positions (a shell extension cannot host a GPUI window);
   the shell-side code only reports the click and the button's position.
   The component turns Zorin's or Ubuntu's own taskbar off while it
   runs and puts it back on removal. Ticket 95's design
   (`design/top-bar-spec.md` section 10, proposed ADR 0036) gives the taskbar
   the same route as the top bar: a GPUI view the shell starts and makes a
   dock, a second row in ADR 0036's program table, and the bottom strip
   reserved through the same `ReserveStrip`; while the top bar runs, the
   taskbar draws no clock, system menu, or tray. Shapes (a) and (b) above are
   decision D4 there.
4. One radius everywhere: the GTK layer, Better OS windows, and the
   compositor clip all take it from the component's single setting.

## Done when

- Screens of a GTK 4, a GTK 3, a Better OS, a Firefox and an Electron window
  side by side, light and dark, showing the same four corners.
- Removing the component puts every window back exactly as before.
