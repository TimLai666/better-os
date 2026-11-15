# Ticket 87 — Control center: Keep awake

**Blocked by:** 84

## Why

Better Awake's settings window moves into the control center (ADR 0017). Two
things the tray promises do not work today: its "Until a time…" opens a window
that has no way to start such a session, and the Quick sessions list the
window edits is never read by the tray, which hard-codes its lengths
(`awake-tray/src/menu.rs:330-333`). The audit also found a D-Bus error string
as the only content when the service is down (A1, A2), pages that stay
clickable without a service (A3), five buttons per row (A4), and switches
drawn as text buttons (A5).

## What to build

Spec section 6, and the Better Awake rows of section 12, every one of them.

- **Service owns the offer.** Add to `awake-ipc` and `awake-service` the
  requests to read and replace the quick-session lengths, the default length,
  the session defaults (policy and battery stop), with the same validation the
  window applies today (`MAX_PRESETS` 8, battery 1–99 or none). Bump the
  protocol version. On first start the service imports
  `$XDG_STATE_HOME/better-awake/awake-gui-preferences.json` once (presets,
  default, defaults; not language or theme) and leaves the file in place. A
  file with a newer schema is not imported and the service says so in its
  status.
- **Tray reads the offer.** The tray's Start menu lists the service's lengths
  in their order, its default first-click length is the service's default,
  and it refreshes when the service announces a change. Its window entries
  (Open, Change session…) run `better-manager --page keep-awake`, and Until a
  time… runs `better-manager --page keep-awake/until`, which opens the page
  with the time dialog (ticket 84 routes it). Keep
  every existing tray test passing and add tests for the new menu source.
- **Kit:** a `TimePicker` in `better_ui::fluent` (hour and minute, 24-hour,
  keyboard rules from WinUI's `TimePicker`), with tests and a gallery page.
- **Page** in `manager-gui/src/pages/keep_awake/`: Status, Rules (with the
  rule editor as a sub-page), History, Diagnostics, as the spec says,
  including Until a time, the per-reason overflow menu, the rule expander with
  its toggle, schedule times through the time picker, and every state of spec
  section 2 (not installed, too old, service not running with Start service).
  All decisions in the GPUI-free model, moved from `awake-gui/src/model.rs`
  with its tests.
- Register search entries, the nav dot, the Home items (service not running,
  session active, backend unavailable), and the Home quick toggle.

## Acceptance

- [x] Each Better Awake row of spec section 12 is shown working, including the
      two ✱ rows: a session started from "Until a time" ends at that time
      (test with a clock seam), and a length added on the page appears in the
      tray menu (test against the menu value, and seen in the sandbox with
      `dbus-run-session`).
- [x] A preferences file from today's `awake-gui` is imported once, a second
      start does not import again, and a newer-schema file is left alone
      (tests).
- [ ] Audit A1–A8 each closed with a screenshot. A1–A5 and A8 are; A6 and A7
      are named in the spec without their wording (the audit itself is not in
      the repository), so they are not claimed (see What was built).
- [x] Light and dark, zh-TW and English, 1600 and 960, screenshotted with the
      service running inside the sandbox's session bus.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## Notes

`awake-gui` keeps building until ticket 91 removes it, so both can be
compared side by side.

## Found on the owner's machine (2026-11-13)

Better Awake's service holds a logind inhibitor for `sleep:idle`
(`systemd-inhibit --list` showed `Better Awake ... sleep:idle block`), which
stops the system from suspending but does not stop GNOME from blanking and
locking the screen: GNOME decides idle through `org.gnome.SessionManager`
(`Inhibit` with the idle flag), not through logind's idle inhibitor. A person
who asks Better Awake to keep the display on gets a dark, locked screen after
`org.gnome.desktop.session idle-delay` (600 s on that machine). The keep-awake
policy that includes the display must also hold a GNOME session inhibitor
(flag 8, idle) for as long as it lasts, with a test against a fake session
manager.

## Found on the owner's machine (2026-11-13): "End session" did nothing

After Better Awake was updated from 0.2.10 to 0.2.11 the service was
restarted (ticket 79), but the tray still running was the 0.2.10 binary
(`/usr/bin/awake-tray (deleted)`), started two hours earlier, and its "End
session" had no effect. Replacing it with the new tray fixed it. Two faults:

- An update must restart the tray as well as the service. The tray is an
  autostart program, not a unit, so nothing restarts it; either make it a
  systemd user unit bound to `graphical-session.target` (then ticket 79's
  restart covers it) or have the update step restart it.
- The tray must follow the service across a restart (watch
  `NameOwnerChanged` for `org.betteros.Awake1`, re-read status and
  re-subscribe), and a menu action that fails must say so instead of doing
  nothing.
- "End session" ends only a manual session (`EndManualSession`); when the
  session running is a rule's, the menu must say so and offer "Pause rules".
- The tray shows "延長工作階段 ▸" as a greyed submenu during an indefinite
  session (`enabled false` on the submenu and its 15 min / 30 min / 1 hour
  children, read from the running tray's `GetLayout` on 2026-11-13). A person
  sees an arrow that does nothing. Extending has no meaning for "until I end
  it", so the item is left out for an indefinite session, and for a timed one
  it is enabled; a disabled item never carries a submenu arrow.

Ticket 102 closes the first two points (the tray is a user unit bound to
`graphical-session.target` and follows `org.betteros.Awake1` across a restart,
and a failed menu action raises a notification) and the greyed "延長工作階段"
(left out for an indefinite session, enabled for a timed one).

## What was built

- **Protocol version 4** (`awake-ipc`): `QueryQuickSessions`,
  `SetQuickSessions`, the `QuickSessions` reply and a `QuickSessionsChanged`
  event on a signal of its own. The offer (`QuickSessions`: lengths, default
  index, session defaults) is validated with the old window's rules: one to
  eight lengths (`MAX_QUICK_LENGTHS`), none twice, the default one of them,
  battery stop 1–99 or none. Every other document is version 3's shape, so a
  version 3 client is answered by restamping and the `Event` signal stays
  stamped 3; version 2 is answered as before. Shipped lengths are the ones the
  tray always showed (Indefinitely, 15, 30, 60, 120 min; default 1 hour), so
  nobody's menu changes until they change it.
- **Service** (`awake-service`): the offer lives in
  `awake-quick-sessions.json` (its own schema 1, so a rollback still reads
  the closed-lid setting). With no such file the service imports
  `awake-gui-preferences.json` once (presets, default, session defaults; not
  language or theme), writes the offer with what the import found, and leaves
  the old file untouched; a second start never imports again. A newer-schema
  or unreadable old file is not imported, and the offer carries
  `import_notice`, which the page shows on Status.
- **Tray**: reads the offer at start, after the service comes back, and on
  `QuickSessionsChanged`; Start lists its lengths in order, a new top item
  "Keep awake for {default}" starts the default in one click, Change length
  offers the same lengths. Against a version 3 service it keeps the fixed
  lengths. Open, Change session and the security confirmation run
  `better-manager --page keep-awake`, Until a time runs
  `better-manager --page keep-awake/until`; if Better Manager is missing or
  refuses the page (older than it) the tray opens `awake-gui` instead.
- **Kit**: `better_ui::fluent::time_picker` (24-hour hour and minute columns,
  looping selectors, WinUI's keys) with tests and a gallery page "Date and
  time"; `SelectorItem::tooltip`; light-dismiss panels now draw above a
  content dialog (a list or time picker inside a dialog opened under it).
- **Page** `manager-gui/src/pages/keep_awake/`: Status (state card with its
  one control, Until a time dialog, reasons with one `…` menu and End asked
  once in place, what's in effect with the lid rows, conflicts expander,
  quick-session lengths, defaults for new sessions with the lid choice of
  ticket 109 and the battery stop, battery and safety with the closed-lid
  limit), Rules (command bar, expander per rule with its toggle, priority,
  test, `…` menu, delete confirmation), the rule editor as a sub-page with
  time pickers for schedules, History (sortable table), Diagnostics; the
  states not installed, too old, service not running (with Start service
  through `manager-core`'s user-unit pass) and reading. `Ctrl+1`…`Ctrl+4`.
  `model.rs` moved from `awake-gui` with its tests; `decide.rs` holds the
  page's own rules. Search entries, the pane dot, three Home items and the
  Home quick toggle.

Verification: workspace tests, a private-bus test of the page's client
against the real service, a private-bus tray test that a length another
client adds appears in the menu, a version 3 service test, engine tests for
the import and for an Until session ending at its time with a clock seam.
In the sandbox (headless GNOME Shell, fake inhibitor backend):
`.shots/{en,zh}-{light,dark}-{1600,960}-*.png`, `a*-*.png` for the audit
rows, and `tray-menu-after-adding-45-minutes.txt` (the running tray's
`GetLayout` after 45 minutes was added on the page).

Not claimed: audit A6 and A7 (wording not in the repository); the service
not running state was screenshotted with Better Awake in Better Manager's
record taken from the host's dpkg (the state shown is correct for that
record).
