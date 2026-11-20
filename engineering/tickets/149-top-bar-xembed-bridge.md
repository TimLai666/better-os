# Ticket 149 — Old X11 tray icons in the top bar: the XEmbed bridge

**Blocked by:** 156

## Under ADR 0038's option D (2026-11-21)

Built for the Better OS session first, where the bar is a layer surface on
`cosmic-comp` (ticket 152). What changes there:

- **Noticing XWayland**: `gnome-session-x11-services.target` does not exist in
  the session. `cosmic-comp` starts XWayland itself (seen rootless, ADR 0038);
  whether eagerly or on demand, and how `DISPLAY` reaches the user manager, is
  ticket 201's fact. The bridge's waiting states keep their shape with the
  session's signal in place of GNOME's target: the session launcher's imported
  `DISPLAY` (ticket 203) and the X connection closing. No timer, as before.
- **The selection** is free in the session (no AppIndicator extension), so the
  bridge holds it whenever the bar runs; the "held by another program" state
  stays for a program that takes it.
- **Clicks and coordinates**: forwarded clicks under `cosmic-comp`'s XWayland
  (X events and XTest), and the conversion from logical bar coordinates to X
  root coordinates with `cosmic-comp`'s XWayland scaling
  (`descale_xwayland`), replace the Mutter facts in the first step; they are
  checked in ticket 201's VM.
- **The final check** with real XEmbed programs is done in the session; the
  "gone back to Zorin's AppIndicator extension" half waits with ticket 151.
- Unchanged: the crate, the unit, docking, capture, the items, `x11rb`, the
  card rows, the unit's sandbox.

## Why

Owner, 2026-11-21 (decision D6 in `design/top-bar-spec.md`): old X11
(XEmbed) tray icons are shown in the top bar too. Zorin's AppIndicator
extension, which shows them today through GNOME Shell's `Shell.TrayManager`,
is off while the bar is on (ticket 156), which frees the X11 tray selection.
A small Rust bridge of Better OS's own takes it on XWayland and offers each
docked icon to the bar as a StatusNotifierItem, as KDE's `xembedsniproxy`
does. Spec section 9.6 is the design, with `xembedsniproxy`'s method cited by
line; sections 6.1 and 12 have the states and the copy.

## What to build

- **First, the facts spec section 14 leaves to this ticket**, in a sandbox
  under AGENTS.md's rules (`systemd-run --user --scope -p MemoryMax=4G -p
  MemorySwapMax=0`, a no-op `ibus-daemon` first on `PATH`, private
  `dbus-run-session`, every `XDG_*` directory temporary, ended by PID), in a
  headless GNOME Shell 46 with XWayland on demand: that
  `gnome-session-x11-services.target` goes active when XWayland starts and
  inactive when it stops, with `DISPLAY` in the user manager's environment
  (or, if the sandbox has no user manager of its own, the closest equivalent,
  and say which); that a forwarded click reaches a GTK 2 and a GTK 3 XEmbed
  icon while the pointer is over a Wayland surface, through X events and
  through XTest, and where each program's own menu opens and whether its grab
  works; that logical coordinates convert to X root coordinates in Mutter's
  physical layout mode at scale 2 and in logical layout mode; that Mutter
  draws nothing for the opacity-0, input-less container; that `GetImage` on a
  redirected icon window returns its picture. Record each in Notes. If clicks
  cannot reach an icon, stop and bring it to the owner before building the
  rest: an icon that shows and cannot be clicked is worse than the card's
  honest state.
- **Crate `top-bar-xembed`**, binary `/usr/libexec/better-os/top-bar-xembed`,
  user unit `better-top-bar-xembed.service` in the `better-top-bar` package,
  `PartOf=` and `After=better-top-bar.service`, enabled and started by
  `manager-core` from dpkg's file list like every component unit, with the
  package's maintainer scripts from `packaging/maintainer/user-units.sh`. Its
  GPUI-free core holds every rule below with tests; the X11 and D-Bus edges
  are thin.
- **Waiting for XWayland without starting it** (spec 9.6): follow
  `gnome-session-x11-services.target`'s `ActiveState` through the systemd user
  manager's `PropertiesChanged`, read `DISPLAY` from its `Environment`, and
  connect only then; drop every item and wait again when the target stops or
  the connection closes; report "no XWayland" when the target does not exist
  and `DISPLAY` is unset. No timer anywhere.
- **The selection**: `_NET_SYSTEM_TRAY_S<n>` claimed without forcing,
  checked with `GetSelectionOwner`, `MANAGER` sent on the root window;
  `_NET_SYSTEM_TRAY_VISUAL` (32-bit TrueColor when the screen has one),
  `_NET_SYSTEM_TRAY_ORIENTATION` horizontal, `_NET_SYSTEM_TRAY_ICON_SIZE` 32;
  XFixes selection-owner events while another program holds it; held only
  while `org.kde.StatusNotifierWatcher` and `org.betteros.TopBar1` have an
  owner; released, with every icon undocked to the root window, on their loss,
  on `SelectionClear`, and before exit.
- **Docking**: `SYSTEM_TRAY_REQUEST_DOCK`; an override-redirect 32 × 32
  container on the tray visual, off screen except during a forwarded click,
  opacity 0, empty input region; reparent, save-set, `XEMBED_EMBEDDED_NOTIFY`,
  `composite_redirect_window` manual; resize requests up to 32 × 32 allowed,
  moves ignored; undock on `UnmapNotify` and `DestroyNotify`.
- **Capture by Damage**: one `Damage` per icon at `NonEmpty`, at most one
  `GetImage` per 100 ms per icon, one capture 500 ms after docking, depth 1,
  16, 24, 30, 32 converted to straight ARGB32 in network byte order, the
  corner-colour mask for 24-bit pictures, the bar's fill as the container
  background (following the appearance), fully transparent captures ignored,
  larger than 32 × 32 scaled down keeping aspect.
- **The items**: one session-bus connection per icon, registered with the
  watcher by bus name; `Id` from `WM_CLASS` with `-2`, `-3` for repeats;
  `Title` from `_NET_WM_NAME`, `WM_NAME`, or the class; `Category`
  `ApplicationStatus`, `Status` `Active`, `ItemIsMenu` false, no `Menu`;
  `Activate`, `SecondaryActivate`, `ContextMenu`, `Scroll` forwarded as
  buttons 1, 2, 3, 4 to 7 at the converted point, as X events when the window
  or a child selected button events, else XTest, with the Shape-aware click
  point; `SYSTEM_TRAY_BEGIN_MESSAGE` balloons dropped.
- **X11 from Rust**: `x11rb` with `RustConnection` and the `composite`,
  `damage`, `xfixes`, `shape`, and `xtest` features. Confirm that
  `Cargo.lock` gains no package; if it changes, regenerate
  `docs/third-party-licenses.md` in the same commit and say what was added and
  under which licence.
- **The Top bar card's X11 rows** (spec 12): no XWayland, selection held by
  another program, the bridge's unit failed with Try again through
  `UserUnitController`, in both languages.
- **The unit's sandbox**: the X socket and the session bus, nothing else;
  record which `systemd.exec` keys a user unit can take on Ubuntu 24.04 and
  which were left out and why.

## Done when

- [ ] The facts above are recorded in Notes with how they were seen.
- [ ] Unit tests in the core: the waiting states (target absent, inactive,
      active, stopped while icons are docked), selection ownership (free,
      held by another, released by it, taken from us), dock and undock
      bookkeeping, damage coalescing at 100 ms, every pixel conversion
      (depth 1, 16, 24 with and without a uniform corner colour, 30, 32
      premultiplied), scaling above 32 × 32, the fully transparent capture,
      `Id` and `Title` rules, the button mapping, and coordinate conversion in
      both layout modes at scale 1 and 2.
- [ ] Against a private `Xvfb` (started under the sandbox rules, ended by PID;
      no XWayland and no compositor needed) and a private session bus with a
      fake watcher: a test XEmbed client docks, its picture arrives as
      `IconPixmap` and changes when it redraws, a click arrives at the client
      as the right button, it undocks on exit, a second selection owner keeps
      the bridge out until it leaves, and no request is sent while nothing
      changes (counted on the connection over 10 idle seconds).
- [ ] Seen with real XEmbed-only programs in the sandbox's headless GNOME
      Shell 46 with the bar from tickets 152 and 156: at least one GTK 2 or
      GTK 3 `GtkStatusIcon` program and one other toolkit's (Wine's
      `explorer.exe` tray or an Xlib test client), each icon shown in the bar,
      clicked, its own menu opened, and gone back to Zorin's AppIndicator
      extension when the bar is turned off. Screenshots in
      `/home/tim/Developer/better-os-evidence/149/`.
- [ ] The idle and animated costs measured (wakeups and CPU over a minute with
      nothing changing, and with an icon redrawing continuously) and recorded
      in Notes.
- [ ] fmt, clippy on stable, the crates' tests, `verify-deb.sh` (the new unit
      carries the maintainer scripts), and the license inventory check pass.
