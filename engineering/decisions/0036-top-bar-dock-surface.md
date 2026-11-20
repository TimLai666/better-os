# ADR 0036: The top bar is a Better OS window that the shell makes a dock

## Status

Accepted by the owner on 2026-11-21, the day it was proposed for ticket 95,
together with shape A of `design/top-bar-spec.md`'s decision D1. It
widens the GJS bounds of [ADR 0012](0012-touchpad-gesture-backend.md) and
[ADR 0018](0018-shared-shell-adapter-extension.md) for one new relay of the
shared extension, which is why it is an ADR and not a commit. Nothing it
describes is built until it is accepted (ticket 151 is blocked on it).

## Context

The owner asked for a menu bar across the top of the screen, as on macOS
(ticket 95), and for a Better OS taskbar at the bottom (ticket 142, item 3),
designed together. `design/top-bar-spec.md` is the design; section 9 has the
evidence summarised here.

On GNOME 46 nothing but GNOME Shell can draw a panel. Mutter 46 on the owner's
machine advertises no layer-shell protocol (the full list of Wayland globals is
in the spec), and its maintainers do not intend to support third-party panels
(mutter issue 1053). A normal Wayland window takes focus when clicked, shows in
the taskbar, Alt+Tab, and the overview, reserves no space (a Wayland window has
no struts in Mutter: the base `meta_window_real_update_struts` returns `FALSE`),
and cannot place itself.

Mutter does offer the shell a way to turn a window into a panel. A program the
shell starts through `Meta.WaylandClient` is a client the shell can name, and
for a window of that client `meta_wayland_client_make_dock` sets the window type
to `META_WINDOW_DOCK`. Mutter 46's `window.c` then does not focus it on a click
("Don't focus panels--they must explicitly request focus"), does not focus it
when it maps, gives it `skip_taskbar`, `skip_pager`, and `always_sticky`, and
puts it in the dock layer, or below a fullscreen window on its monitor. Zorin's
desktop icons extension uses the same route for its desktop window. On GNOME 46
only a client the shell spawned can be made a dock: `new_indirect` and
`setup_fd`, which would let a process started elsewhere connect as such a
client, exist in `libmutter-14` but are not in its typelib, so GJS cannot call
them.

So either JavaScript draws the bar, or JavaScript starts the program that
draws it. Both need more than ADR 0018 allows.

## Decision

**The top bar is drawn by `better-top-bar-view`, a GPUI program, which the
shared extension starts as a Wayland client and makes a dock. A new relay,
`org.betteros.ShellAdapter1.Panels`, does that, places the window, reserves its
strip of the screen, hides GNOME's own top panel and keyboard indicator while
the bar runs, reports what the bar needs to know about windows, monitors, input
sources, and notifications, and performs the window and input-source actions
the bar offers. Everything the bar decides is decided in Rust by
`better-top-bar`, the relay's counterpart. The relay is active only while that
service owns `org.betteros.TopBar1` and runs from
`/usr/libexec/better-top-bar`.**

The interface is the table in `design/top-bar-spec.md` section 9.4. No method
takes a string.

### The sentences this changes

ADR 0018, "What it may not do":

- "Spawn or execute anything." becomes: **Spawn or execute anything, except
  that the panel relay may start the programs in the closed table below, with
  the fixed argument list given there, through `Meta.WaylandClient`, at most
  one instance of each at a time, and only when its counterpart asks.** It
  never restarts one by itself: when a view exits it reports `ViewEnded` and
  forgets it, and the counterpart decides whether to ask again.
- "keep any list, map, or record beyond its bus handles, the counterpart's
  unique name, and whether the takeover is on." becomes: **…beyond its bus
  handles, the counterpart's unique name, whether each takeover is on, the
  `Meta.WaylandClient` and subprocess handle of each running view, and one
  chrome actor per monitor and edge whose thickness the counterpart set.** It
  keeps no list of windows, applications, notifications, or input sources: each
  is read from the shell when it is reported or acted on.

ADR 0018, "What the JavaScript may do":

- "Perform an action GNOME Shell already performs, named as itself, on an
  object the shell already has, identified by the shell's own id." gains:
  **and, for the panel relay, `make_dock` and `move_frame` on a window of a
  view it started (`owns_window` must be true), and one invisible,
  non-reactive chrome actor per monitor and edge added with
  `Main.layoutManager.addChrome(actor, {affectsStruts: true,
  trackFullscreen: true, affectsInputRegion: false})`, the way GNOME's own
  panel reserves its strip.** An index the counterpart sends (a monitor, an
  input source, an engine property, a notification action) is resolved against
  the shell's current list at call time and refused when it is out of range or
  its serial is stale.
- "Take one of GNOME's own handlers off the signal it is connected to and put
  it back…" gains: **and hide `Main.layoutManager.panelBox` and
  `Main.panel.statusArea.keyboard`'s container, and show them again exactly as
  they were, on the counterpart's request, when its name goes, and on
  `disable()`.**
- "Report, as a signal, what the shell saw" now covers, for the panel relay,
  the focused window (its id, application id and name, PID, client type, X11
  id, GTK bus name and menubar path, and what it allows), the monitors, which
  monitor is in fullscreen, GNOME's input sources and the engine's properties,
  and GNOME's notifications (app, title, body, time, urgency, action labels).
  Every value is copied unchanged and sent to the counterpart alone. A
  notification's text is personal data; it crosses only to the checked
  counterpart, never broadcast, and the service keeps it in memory only.

AGENTS.md's language rule, "it may store nothing, it may execute nothing", gets
the same exception in the same words, and names the closed table.

ADR 0012's amendment ("It may not … execute anything") is unchanged for the
touchpad adapter; this exception is for the shared extension's panel relay
only.

### The closed table of programs

| Program | Arguments | Counterpart that may ask | Added by |
| --- | --- | --- | --- |
| `/usr/libexec/better-os/top-bar-view` | none | `org.betteros.TopBar1`, owned by `/usr/libexec/better-top-bar` | this ADR |

Ticket 142's Better OS taskbar, when it is built, adds one row (its view, its
counterpart) by amending this ADR; it reuses `ReserveStrip`, `PlaceView`, and
the dock step unchanged.

### What stays exactly as ADR 0018 says

Each relay acts only while its counterpart owns its name and runs from its
packaged path; every method refuses any other caller; signals go to the
counterpart only; the extension reads only its interface XML files and the
counterpart's `/proc/<pid>/exe`; no method takes free text; nothing is written
to any setting. With the counterpart gone, GNOME's panel and indicator are
shown again, the strips are removed, and the view, which talks only to the
service, exits when the service's name goes.

### Why the view is a separate process from the service

The service is a systemd user unit like every Better OS background service:
it is enabled by installing, comes back at login, and is restarted by the
package's postinst after an update. The view must be the shell's child to be a
dock, so it cannot be the unit. When its program is replaced it exits (an
in-place `exec` would lose the shell's Wayland connection, whose file
descriptor `wayland-client` marks close-on-exec), and the service asks the
shell for a new one, which meets AGENTS.md's rule that every process runs the
new program within seconds of an update.

## Consequences

- The bar is Fluent and every rule in `design/top-bar-spec.md` is Rust with
  tests in `top-bar-core`. The right side (tray host and watcher, network,
  sound, battery, input method, clock, notifications) is built in Rust rather
  than reused from GNOME's panel.
- A new test, `crates/top-bar-service/tests/extension.rs`, asserts the panel
  relay against the extension file in the allow-list shape ADR 0018 set: the
  spawn appears once, with a constant argument array equal to the table; the
  relay assigns only the fields named above; every method checks the caller;
  no input argument is `s`; every hidden GNOME actor is shown again on name
  loss and on `disable()`.
- During GNOME's overview and application grid the bar is covered like every
  window. A fullscreen window covers it, and its strip is not reserved, without
  any code (Mutter's dock layer and `trackFullscreen`).
- A GNOME major release can rename `panelBox`, the keyboard indicator, or the
  message tray; the relay reports what it found in `Capabilities`, refuses the
  suppression it cannot do, and the bar says so rather than drawing over
  GNOME's panel. The extension's metadata names GNOME 46 only.
- `Meta.WaylandClient.spawnv` gives the view the shell's own environment. The
  view must not read anything from its environment beyond what GPUI and the
  session bus need; ticket 152 asserts that it reads no other variable.

## Alternatives not taken

- **GNOME's own top panel, moved back to the top, with menus drawn in
  JavaScript** (spec section 9.5). Zorin's taskbar puts GNOME's panel back at
  the top with one setting, `stockgs-keep-top-panel`, and the whole right side
  is then GNOME's. But the Better OS menu and the application menus would be
  `PopupMenu` trees the extension builds from dbusmenu data it receives: a kept
  tree, labels it draws, and widgets it lays out, which is a JavaScript
  application, not a relay. It would also not look like the rest of Better OS.
- **An XWayland dock** (`_NET_WM_WINDOW_TYPE_DOCK` with struts): needs no shell
  code, but runs the bar through X11 inside a Wayland session, makes its
  scaling depend on Mutter's XWayland scaling mode, and is the route GNOME is
  leaving. Not tried.
- **A normal GPUI window the shell only moves and keeps above.** Without the
  dock type it takes focus on every click, so the "focused application" would
  always be the bar.
- **Waiting for a GNOME that offers layer-shell.** Its maintainers have said it
  will not.
- **Making the bar only on COSMIC** (ticket 143), where layer-shell exists. The
  owner's desktop is GNOME today.

## Amendment, 2026-11-21: no application menus

Amendment, 2026-11-21, after the owner's decision D5 in
`design/top-bar-spec.md`: the bar carries no application menus at all, and
every window keeps its own menu row. The decision above is unchanged. One part
of what it allows existed only for application menus and is not built: in
"Report, as a signal, what the shell saw", the focused window's **X11 id** and
its **GTK bus name and menubar path**. They were there so the bar could find a
window's menu through `com.canonical.AppMenu.Registrar` and `org.gtk.Menus`,
which nothing now does. The relay's `FocusChanged` carries the window id,
application id and name, PID, client type, and what the window allows, and
nothing else (spec section 9.4); ticket 151's allow-list test holds it there.
The rest of the relay table, the closed program table, and every sentence this
record changes in ADR 0018 and AGENTS.md stand as accepted.

The same day the owner decided (D6) that old X11 tray icons are shown in the
bar through a Better OS bridge (spec section 9.6, ticket 149). The bridge is a
user unit that talks to XWayland and the session bus; it asks nothing of the
shell, so it widens nothing here.
