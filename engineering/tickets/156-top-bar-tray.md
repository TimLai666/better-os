# Ticket 156 — The tray moves to the top bar while it is on

**Blocked by:** 150, 152

## Under ADR 0038's option D (2026-11-21)

The owner chose the Better OS session on `cosmic-comp` beside GNOME; the bar
is built for the session first (ticket 152). In the session no AppIndicator
extension holds `org.kde.StatusNotifierWatcher`, so the bar's watcher owns it
from the start of the session and is never handed back: the Better Defaults
entry "Tray icons in the top bar" and the headless GNOME Shell check below are
GNOME's half and wait with ticket 151. The host, the clicks, the tray menus,
the drawing, and the card rows are unchanged and checked in the session
(ticket 201's VM) with Better Awake's and Better Monitor's trays and a
libappindicator test item.

## Why

Owner, 2026-11-12: while the top bar is on, every StatusNotifierItem shows in
it and not in the bottom taskbar; when it is off they go back; the switch is
one setting applied and restored through Better Defaults
(`design/top-bar-spec.md` section 6.1). Owner, 2026-11-21 (decision D6): old
X11 tray icons show there too, through ticket 149's bridge, which reaches the
bar as ordinary StatusNotifierItems through the watcher built here.

## What to build

- **Host and watcher** in `top-bar-service`: `org.kde.StatusNotifierWatcher`
  (`RegisterStatusNotifierItem`, `RegisterStatusNotifierHost`,
  `RegisteredStatusNotifierItems`, `IsStatusNotifierHostRegistered`,
  `ProtocolVersion`, the item and host signals), taken the moment Zorin's or
  Ubuntu's AppIndicator extension releases it and released before it is put
  back; and the host side reading each item's properties and signals
  (`NewIcon`, `NewAttentionIcon`, `NewStatus`, `NewTitle`, `NewToolTip`,
  `NewMenu`), menus through the `dbusmenu` client (ticket 150).
- **Clicks** (spec 6.1's table): primary `Activate` or the menu by
  `ItemIsMenu`; secondary opens the item's `Menu`, or calls
  `ContextMenu(x, y)` when the item has none; middle `SecondaryActivate`;
  scroll `Scroll`; `x`, `y` the item's bottom edge in logical screen
  coordinates.
- **Tray menus** (spec section 5), the one kind of menu whose contents come
  from another program: the icon slot only when an item has an icon or a
  check, check and radio items, submenus with `AboutToShow`, separators,
  hidden and disabled items, an empty submenu drawn disabled, truncation at
  360px with the full text as tooltip and accessible name, mnemonics used for
  type-to-select, live `LayoutUpdated` and `ItemsPropertiesUpdated` while the
  menu is open, and the 500 ms 這個應用程式沒有回應。 / This app isn't
  responding. row. (Moved here from the deleted ticket 153, whose application
  menus decision D5 removed; tray menus need the same drawing.)
- **Drawing** (spec 6.1): icons by name or pixmap at 16 logical px (nearest
  size at scale 2), status, attention dot, tooltips, accessible names; at most
  six on the bar (four below 1280px), the rest and every `Passive` item in the
  overflow flyout; the person's drag order kept in
  `$XDG_STATE_HOME/better-os/top-bar/tray.json`.
- **The Better Defaults entry "Tray icons in the top bar"**: takes
  `zorin-appindicator@zorinos.com` or `ubuntu-appindicators@ubuntu.com` out of
  `org.gnome.shell enabled-extensions` and puts it back exactly where it was,
  through `manager_platform::shell_extensions` and Better Defaults' capture,
  apply, verify, and restore; "nothing to do" when neither is present.
- **The Top bar card's tray rows** (spec 12): the tray note and the count of
  icons that did not come back after the switch.

## Done when

- [ ] Unit tests: watcher bookkeeping (an item that leaves, a duplicate
      registration, an item registered by object path rather than bus name, as
      `/org/ayatana/NotificationItem/...` on the owner's machine), the click
      table for items with and without a menu, the tray menu model from a
      dbusmenu tree (hidden, disabled, check, radio, empty submenu,
      truncation), the overflow rule at each width, the drag order, and the
      defaults entry's capture and restore on an isolated dconf.
- [ ] In a headless GNOME Shell 46 with Zorin's AppIndicator extension and its
      own taskbar loaded (sandbox rules): Better Awake's and Better Monitor's
      trays and a libappindicator test item move from the taskbar to the bar
      when the bar turns on, their menus work from the bar, and they go back to
      the taskbar when it turns off. Screenshots in
      `/home/tim/Developer/better-os-evidence/156/`, the overflow compared with
      `design/top-bar/tray-overflow-*.html`.
- [ ] Which items on the owner's own machine re-register is listed in Notes
      from a read-only `RegisteredStatusNotifierItems` taken before and after
      the owner turns the bar on (ticket 159 does it with the owner).
- [ ] fmt, clippy on stable, and the crates' tests pass.
