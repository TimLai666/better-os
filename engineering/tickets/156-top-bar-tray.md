# Ticket 156 — The tray moves to the top bar while it is on

**Blocked by:** 150, 152

## Why

Owner, 2026-11-12: while the top bar is on, every StatusNotifierItem shows in
it and not in the bottom taskbar; when it is off they go back; the switch is
one setting applied and restored through Better Defaults
(`design/top-bar-spec.md` section 6.1).

## What to build

- **Host and watcher** in `top-bar-service`: `org.kde.StatusNotifierWatcher`
  (`RegisterStatusNotifierItem`, `RegisterStatusNotifierHost`,
  `RegisteredStatusNotifierItems`, `IsStatusNotifierHostRegistered`,
  `ProtocolVersion`, the item and host signals), taken the moment Zorin's or
  Ubuntu's AppIndicator extension releases it and released before it is put
  back; and the host side reading each item's properties and signals
  (`NewIcon`, `NewAttentionIcon`, `NewStatus`, `NewTitle`, `NewToolTip`,
  `NewMenu`), menus through the `dbusmenu` client.
- **Drawing** (spec 6.1): icons by name or pixmap at 16 logical px (nearest
  size at scale 2), status, attention dot, the four clicks and scroll,
  tooltips, accessible names; at most six on the bar (four below 1280px), the
  rest and every `Passive` item in the overflow flyout; the person's drag order
  kept in `$XDG_STATE_HOME/better-os/top-bar/tray.json`.
- **The Better Defaults entry "Tray icons in the top bar"**: takes
  `zorin-appindicator@zorinos.com` or `ubuntu-appindicators@ubuntu.com` out of
  `org.gnome.shell enabled-extensions` and puts it back exactly where it was,
  through `manager_platform::shell_extensions` and Better Defaults' capture,
  apply, verify, and restore; "nothing to do" when neither is present.
- **The re-registration count** on the Top bar card (spec 6.1) and the XEmbed
  note (decision D6).

## Done when

- [ ] Unit tests: watcher bookkeeping (an item that leaves, a duplicate
      registration, an item registered by object path rather than bus name, as
      `/org/ayatana/NotificationItem/...` on the owner's machine), the overflow
      rule at each width, the drag order, and the defaults entry's capture and
      restore on an isolated dconf.
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
