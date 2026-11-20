# Ticket 153 — The top bar shows applications' menus

**Blocked by:** 150, 152

## Why

The bar shows the focused window's own menus where the application hands them
over (`design/top-bar-spec.md` section 5): Better OS windows through a Better
OS registration, GTK windows through the link Mutter keeps, X11 programs
through the AppMenu registrar.

## What to build

- **The sources, in the spec's order** (section 5.1), in `top-bar-service`:
  - `org.betteros.TopBar1.Menus.Register(o path)` from a window's own
    connection, matched by the caller's PID
    (`GetConnectionUnixProcessID`) and `app_id` to the focused window the
    relay reports; a registration ends when the caller leaves the bus;
  - GTK: the relay's `gtk_bus_name` and `gtk_menubar_path` read as
    `org.gtk.Menus` (`Start`, `End`, `Changed`) with actions activated through
    `org.gtk.Actions.Activate`, including action state for check and radio
    items;
  - `com.canonical.AppMenu.Registrar` owned by the service when no other
    program owns it (`RegisterWindow`, `UnregisterWindow`, `GetMenuForWindow`,
    `WindowRegistered`, `WindowUnregistered`), matched to the relay's X11 id;
    when another program owns it, used as a client and said on the Top bar
    card; released when the bar is turned off;
  - nothing: the actions every window has.
- **Drawing** (section 5.3): the menu titles after the application name, menus
  with icons slot, shortcuts, check and radio, submenus, separators, hidden
  and disabled items, truncation, type-to-select, the 500 ms "isn't
  responding" row, live updates while open.
- **More** (section 5.4): menus that do not fit move into 更多 / More from the
  last.

## Done when

- [ ] Unit tests for the source order, PID and `app_id` matching (two
      windows of one process, a process that registers twice, a registration
      whose owner left), GMenuModel and dbusmenu conversion into the bar's
      menu model, and the More rule at each width.
- [ ] In a headless GNOME Shell 46 (sandbox rules): a GTK 4 `GtkApplication`
      test window with a menubar, a GTK 3 one, an XWayland program registered
      through the registrar, and a test program registered through
      `Menus.Register` each show their menus and activating an item reaches
      the program. Screenshots in
      `/home/tim/Developer/better-os-evidence/153/`.
- [ ] fmt, clippy on stable, and the crates' tests pass.
