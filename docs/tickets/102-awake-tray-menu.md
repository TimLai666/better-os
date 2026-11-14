# Ticket 102 — The Better Awake tray menu is correct and every item works

**Blocked by:** none · **Status:** merged, not released

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

## What the host does (read from the source)

`zorin-appindicator@zorinos.com/dbusMenu.js` on the owner's machine and
upstream `ubuntu/gnome-shell-extension-appindicator` `dbusMenu.js` behave the
same in every part below.

- Every layout read is `GetLayout(0, -1, ["type", "children-display"])`
  (`_layoutUpdateUpdateAsync`). Only an id the host has never seen is then
  filled with `GetGroupProperties([ids], [])` (`_doLayoutUpdate`,
  `_requestProperties`). An id it already holds gets only the `type` and
  `children-display` that reply carried; its label, `enabled`, and toggle state
  are never read again unless an `ItemsPropertiesUpdated` arrived while the
  menu was closed.
- `LayoutUpdated` and `ItemsPropertiesUpdated` that arrive while the menu is
  closed are remembered and acted on when it next opens (`_onSignal`,
  `set active`). Opening sends `AboutToShow(0)` and reads the layout again if
  the answer is true; unfolding a submenu sends `AboutToShow(id)`.
- A click is `Event(id, "clicked", <i 0>, time)`. The SNI side refreshes a
  property for each `New<Property>` signal (`_onSignalAsync`).

## Root cause

The tray numbered every layout 1..N. The awake and not-awake layouts reuse the
same ids for different items, so the host kept the not-awake labels on the
awake layout's ids: id 4, "開始一段工作階段" in the not-awake menu, became the
"開始於" line, kept its old label and its `children-display: submenu` (a
standard item sent neither property to overwrite it), and lost its children, so
it showed an arrow that opened nothing. Ids 6–9 kept "15 分鐘 … 2 小時" at the
top level, and id 11, a separator before, became the Extend submenu with no
label. That is every symptom above. `LayoutUpdated` with a bumped revision was
already being sent; the host acted on it and still kept the stale properties.

## What changed

- Every changed layout is published under ids no host has seen (ids only move
  forward), so the host creates new items and reads all their properties. A
  change of wording or state alone (a countdown, a checkmark, the armed
  override) keeps the ids and is sent as `ItemsPropertiesUpdated`, so an open
  submenu is not rebuilt shut. Every item states its `type`. A click from the
  layout replaced a moment ago still does what it showed.
- A disabled submenu and an empty one are not sent at all, and the separators
  around a hidden item collapse. The indefinite session's "延長工作階段" is gone;
  a timed one's is an enabled submenu.
- The awake menu offers "改成其他長度" (持續 / 15 / 30 分鐘 / 1 / 2 小時, minus
  the length the session already has) and
  "結束工作階段". A length chosen while a session runs is a `ChangeSession` on
  it, measured from now, keeping its reason and policy; a start the service
  refuses as a duplicate (a menu older than the service) is re-read and sent as
  that change. Any refusal is said in a desktop notification in plain words.
- Only the whole menu opening refreshes from the service (`AboutToShow(0)`).
- The four lines saying what the session holds back (system sleep, display
  sleep, automatic lock, battery protection) are in a "目前的設定" submenu, so
  the awake menu and any submenu inside it fit an 800-pixel-high screen without
  GNOME scrolling them. They use the window's own words (已擋下 / 維持原本行為,
  螢幕休眠, 電量停止門檻, and 這台機器不適用 on a machine with no battery).
  "變更工作階段…" is now "工作階段設定…", so it does not read as another way to
  change the length.
- Opening Better Awake brings a missing tray back by asking the bus for
  `org.betteros.AwakeTray1`, whose activation file starts
  `better-awake-tray.service`; the window never runs `systemctl`. It asks only
  while the unit's enablement link is in
  `~/.config/systemd/user/graphical-session.target.wants/`, so a component
  switched off stays off, and it spawns the program only where no unit is
  installed (a development build).
- A change or extension the service refuses because the session just ended is
  read again and sent as a start; the wording for that refusal does not talk
  about ending. Menu rebuilds tell the host in the order they were made. A tray
  that replaces one still exiting asks for its bus name again for a few seconds
  instead of giving the low-battery notification up.
- Every call the tray makes gives up after 5 seconds (`method_timeout` on its
  connection; the first one at login, which may start the service, after 25)
  and is then reported as the service not answering, so a lost reply cannot
  hang a click or the loop that follows the service. Menu clicks run on the
  tray's own runtime rather than inside zbus's executor.
- The tray serves its item and menu from its connection's builder, and the
  service does the same with its object and name. Objects added to a
  connection that is already running start zbus's object server lazily, and
  a call that arrives before it has subscribed is dropped without a reply;
  that hung the bus tests about once in ten runs and could leave a panel's
  first call unanswered.
- The tray follows the service across a restart (`NameOwnerChanged`, status
  read again, signals still arriving) and registers again whenever a
  StatusNotifierWatcher appears, instead of exiting when none is there yet.
- The tray is the `better-awake-tray` systemd user unit, bound to
  `graphical-session.target`, in place of the XDG autostart entry, so Better
  Manager's update restarts it with the service. `ExecStartPre` ends a tray an
  older autostart entry started, so an upgrade does not leave two icons. It is
  restarted after 5 seconds when it fails, at most five times in five minutes.

## Acceptance

- [x] Host behaviour read from both sources (above).
- [x] Tests replay the host: `crates/awake-tray/tests/session_bus.rs`, section
      "The AppIndicator host, replayed", drives the tray over a private bus
      exactly as `dbusMenu.js` does and requires the drawn menu to equal the
      tray's own. Against ids reused per layout, three of them fail with the
      owner's stale menu.
- [x] No unlabeled item, no arrow on a disabled item, no submenu that opens
      nothing (checked on every drawn menu in those tests).
- [x] Starting while awake changes the session; a refusal raises a
      notification (tests).
- [x] Tray unit, service restart, watcher restart (tests, `verify-deb.sh`).
- [x] Seen in a headless `gnome-shell` 46.0 with `zorin-appindicator` loaded
      from `/usr/share/gnome-shell/extensions`, on a private bus with a fake
      inhibitor backend: the not-awake menu, its start submenu, the awake menu
      after starting, changing the length, extending, ending, the quick option
      checkmark, the rule submenu and its nested pause submenu, resume, and the
      two-step override were each clicked and screenshotted.

"開啟 Better Awake…" was clicked from a tray started as its unit runs it and
opened the window. "直到指定時間" and "工作階段設定…" open the same window and
were not clicked.
