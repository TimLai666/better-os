# Ticket 102 — The Better Awake tray menu is correct and every item works

**Blocked by:** none · **Status:** ready

## Why

On the owner's Zorin 18 (GNOME 46, `zorin-appindicator` showing
StatusNotifierItems), 2026-11-13, with Better Awake 0.2.11:

- The menu the panel showed did not match the service. The service reported
  an active manual session (`query_status`: `active_manual`, indefinite,
  started 01:55), and the tray's own `GetLayout` agreed, but the panel showed
  the not-awake menu ("目前未保持清醒", "開始一段工作階段 ▸", 15 分鐘 … 2
  小時, "直到指定時間"). The panel kept an old layout: the tray does not tell
  the host the layout changed in a way `zorin-appindicator` acts on (check
  `LayoutUpdated` with a bumped revision, `ItemsPropertiesUpdated`, and the
  SNI `NewMenu`/`NewStatus`/`NewIcon` signals, against what the AppIndicator
  extension actually listens to).
- Items with an arrow did not open: "開始一段工作階段 ▸", "延長工作階段 ▸",
  and an item under "直到指定時間" that shows only an arrow and no label.
- Clicking start a second time while a session already ran showed an error.
- After an update the old tray binary kept running (ticket 87 records it).

## What to build

- Read the AppIndicator/KStatusNotifierItem support extension's source (the
  one Zorin ships, `zorin-appindicator@zorinos.com`, and upstream
  `ubuntu/gnome-shell-extension-appindicator`) to see exactly how it fetches
  the layout and submenus (`GetLayout` depth, `AboutToShow`,
  `GetGroupProperties`, `EventGroup`) and which signals refresh it; make the
  tray do what it expects, with tests that replay those calls.
- No item without a label; no arrow on a disabled item; no submenu that the
  host cannot open. If submenus are unreliable in panel hosts, flatten them.
- The menu for "awake" offers End and the durations to change to; the menu
  for "not awake" offers the start durations once, not twice.
- Starting while awake replaces or extends the session per the item chosen,
  never an error; any refusal is shown as a notification in plain words.
- The tray restarts with the package and follows a service restart (ticket
  87's first two points), and its autostart is a systemd user unit bound to
  `graphical-session.target` so ticket 79's restart covers it.
- Verify on a real GNOME session if possible; otherwise in a nested
  `gnome-shell --nested` with the AppIndicator extension enabled, with
  screenshots of both menus.
