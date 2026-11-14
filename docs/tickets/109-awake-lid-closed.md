# Ticket 109 — Keep awake also covers closing the lid

**Blocked by:** none · **Status:** implemented, not merged

## Why

The owner, 2026-11-14: Better Awake was keeping the machine awake (a
manual, indefinite session), the lid was closed at 00:09, and the machine
tried to suspend anyway. Read on the owner's machine the same day:

- Better Awake holds a logind inhibitor `sleep:idle`, mode `block`
  (`systemd-inhibit --list`).
- logind's `HandleLidSwitch` is `suspend`, and `LidSwitchIgnoreInhibited`
  is at its default, `yes`: closing the lid suspends even while a `sleep`
  inhibitor is held. Only a `handle-lid-switch` inhibitor stops logind from
  acting on the lid (man logind.conf, `LidSwitchIgnoreInhibited=`; man
  systemd-inhibit, `handle-lid-switch`).
- Nothing in `crates/awake-*` mentions the lid.

(That night the suspend also failed because a USB controller had died on an
earlier resume; ticket 101 covers that. Without that failure the machine
would have slept, against the owner's keep-awake session.)

## What to build

- **A session choice: "Also when the lid is closed"** (「闔上螢幕時也保持清醒」).
  While a session with it is active, the service also holds a
  `handle-lid-switch` block inhibitor, so closing the lid does not suspend;
  GNOME still turns the internal screen off. The policy model, status
  document, and protocol gain the field; `effective_policy` reports it; the
  window, the tray's "Current settings", and Better Manager's Keep awake
  page show it in the same words.
- **Default on for sessions the person starts by hand**, as a setting
  ("Keep awake when the lid is closed: on / off") in Better Awake's
  settings; rules (automatic sessions) default off and each rule can turn it
  on.
- **Safety, stated in the UI next to the choice.** With the lid closed and
  on battery, the existing battery stop threshold still ends the session;
  in addition, if the lid stays closed on battery for longer than a set time
  (default 2 hours, editable, "never" allowed) the session ends and the
  machine may sleep. A desktop notification says why a session ended.
- **External monitor and power**: unchanged GNOME behaviour when an external
  display is connected (GNOME already holds its own lid inhibitor then).
- Tests with the fake inhibitor backend for each case: lid choice on/off,
  battery threshold, the closed-lid time limit, rule sessions.

## Acceptance

- [x] With a manual session, `systemd-inhibit --list` (or the fake backend in
      tests) shows `handle-lid-switch` held; ending the session releases it.
      Fake backend only: `a_manual_session_holds_the_lid_switch_and_ending_it_releases_the_lock`
      and the tray's `a_session_started_from_the_tray_holds_the_lid_unless_the_option_is_unticked`.
      A real `systemd-inhibit --list` was not run, by rule.
- [x] Unit and session-bus tests for every case above.
- [ ] The window, tray, and Better Manager show the choice; screenshots.
      Window and tray: done, screenshots below. Better Manager: its Keep
      awake page is still ticket 87's placeholder, so the choice appears
      there when ticket 87 builds the page (see below).
- [x] Tests never hold a real inhibitor or touch the person's real service.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      build-deb plus verify-deb, and the license check pass.

## What was built

- **Policy.** `SessionPolicy` gains `prevent_lid_suspend` (read as `false`
  from anything written before it), and `SessionPolicy::manual_default()` is
  the quick default with it on. Rules keep `quick_default()`, so a rule holds
  the lid only when it says so. `BackendCapabilities` gains `lid_switch`, and a
  backend without it reports `PolicyGap::LidSwitch` instead of claiming the
  lid. A disagreement between active rules over the lid is a conflict like any
  other field (`prevent_lid_suspend`).
- **Inhibitor.** The logind lease asks for `handle-lid-switch` beside `sleep`
  and `idle` in one block lock (`sleep:idle:handle-lid-switch`, in the order
  logind lists it, so lease verification still matches). Ending the session,
  or turning the choice off in a running one, retakes the lock without it.
  A changed lock is taken before the old one is let go: releasing first left
  a moment with no lid lock, and with the lid closed logind suspends in that
  moment (below), which a rule starting or stopping beside a session caused.
  A test records, after every acquire and release, whether a lid lock is held
  and requires it never to drop during a swap. When the new lock cannot be
  taken the old one is kept, unless it holds the lid and the lid is no longer
  wanted, which is let go; every tick tries again until what is held is what
  is wanted. If polkit refuses (`AccessDenied` or
  `InteractiveAuthorizationRequired`), the same lock without the lid tells
  which part it refused: granted, the lid was refused, so `sleep:idle` is held
  and the lid reported as unavailable (`lid_switch` off,
  `PolicyGap::LidSwitch`); refused too, the lock failing is reported and the
  lid capability is left as it is.
- **Closed-lid limit.** A service setting, `awake-service-settings.json`
  beside the service's state (schema 1; a newer file is kept and refused, an
  unreadable or out-of-range one moved aside). Default 2 hours; 15 minutes to
  24 hours, or never. Each tick, while a session holds the lid, the service
  reads logind's `LidClosed` and `Docked` and the power provider's AC state.
  The time is measured on `CLOCK_BOOTTIME` (`/proc/uptime`), so setting the
  date neither shortens nor stretches it. Docked or with an external display
  (logind's `Docked` counts both) the clock does not run: logind then uses
  `HandleLidSwitchDocked`, `ignore` by default, and GNOME keeps the machine
  up. Once the lid has been closed on battery, undocked, for the limit, every
  session holding the lid ends
  with the new cause `lid_closed_on_battery`, and only those. Opening the lid
  or plugging in restarts the clock. A rule that holds the lid is held back
  (counted as refused) until the lid opens or the charger returns, so it is not
  restarted only to be ended again. The battery stop threshold still ends a
  session on its own, lid or no lid.
- **Notification.** The closed-lid stop is announced as `SessionEnded` with
  `lid_limit_minutes`, and raised as a desktop notification ("螢幕闔上並使用電池
  已超過 2 小時，已停止保持清醒"). A tray from this ticket owns
  `org.betteros.AwakeTray1.Protocol3` beside `org.betteros.AwakeTray1` and
  raises it; without that second name the service raises it itself, because a
  tray from before version 3 owns only the first and cannot read the stop.
  Exactly one notification either way (bus tests for both trays).
- **Protocol version 3.** New: the policy and capability fields, the status's
  `lid` (`battery_limit`, `closed`, `stops_at_unix_seconds`), the
  `set_lid_battery_limit` request, and `lid_limit_minutes` on `SessionEnded`.
  The service still accepts version 2 and answers a version 2 request in
  version 2's shape (`AwakeResponse::to_json_for`), with every version 3 key,
  the `lid_switch` gap, and a lid conflict removed. Every event is pushed twice:
  version 2 on `StatusChanged`, which an installed window or tray listens to,
  and version 3 on the new `Event` signal, which the tray now follows. A
  version 2 `ChangeSession` or `UpdateRule` keeps the lid choice already in
  force, so an older tray changing a session's length does not drop it. A
  request that cannot be parsed is refused in the version it was stamped
  with. Checked against the real thing: documents the ticket 109 engine
  answered a version 2 request with (status, rules, history, rule test, a
  status event, a closed-lid end) were parsed by `awake-ipc` built from the
  commit before this ticket, and all parsed; the version 3 documents did not.
  A session a version 2 client starts gets the lid choice on, the manual
  default; a rule it creates keeps the rule default, off.
- **New clients and an old service.** A window or tray from this ticket reads
  the service's `ProtocolVersion` property first; a service an update has not
  restarted yet (0.2.12) says 2, and every request then goes in version 2's
  shape, without the lid fields. The property is read from the service each
  time, not from zbus's property cache, which a restarted service never
  updates; the tray reads it again whenever the service's name gets a new
  owner (bus test: the same name owned first by a version 2 service, then by
  a version 3 one). Where the property cannot be read, a reply
  stamped version 2 that refuses the version or calls a field or variant
  unknown (0.2.12 parses before it checks the version, so a version 3 request
  with lid fields comes back as malformed) also switches it, and the request
  is sent again. The version 2 reply is read with every version 3 field at its
  default. Checked with `awake-ipc` built from `v0.2.12`: it parses every
  downgraded start, change, rule create and update, status, and end request,
  and every refusal it gave version 3 requests switches the client
  (`.shots/v0212-parses-legacy.txt`, `.shots/fallback-decisions.txt`). The tray keeps doing so until the service leaves and comes back on
  the bus, and while it does it follows the legacy `StatusChanged` signal
  (ignored otherwise, since a current service sends both). The lid choice
  reads as unavailable in both: the tray shows "闔上螢幕時也保持清醒：目前無法
  使用" in Quick options and "闔上螢幕時睡眠: 目前無法使用" in "目前的設定";
  the window disables the choice and the closed-lid limit and says the
  service has to restart first. `set_lid_battery_limit` is refused as
  `awake.ipc.error.needs_newer_service`.
- **Tray.** Quick options gains "闔上螢幕時也保持清醒", ticked by default.
  "目前的設定" gains "闔上螢幕時睡眠: 已擋下 / 維持原本行為" and, while the lid
  is held, "闔上螢幕並使用電池時: 2 小時後停止" (or "不會因此停止"); while the
  lid is closed on battery and the clock runs, a countdown instead
  ("15:42 停止（還有 1 小時 58 分鐘）" in the window's words).
- **Window.** Session defaults and Quick sessions carry the choice (on by
  default) with the safety stated beside it, and the external-display note.
  Session defaults now says which sessions it applies to: the ones this window
  starts or changes; the tray's Quick options have their own tick boxes, and
  the lid one is on by default in both. The safety sentence says only what
  applies: the battery threshold the session carries, or that it does not
  stop for battery; the closed-lid limit, or that there is none and the
  battery can run out; nothing about a battery on a machine without one; and
  that ending with the lid closed sleeps at once. Status shows the countdown
  while the clock runs and the limit as a rule otherwise.
  The preferences file moves to schema 2; a schema 1 file is read with the
  choice on. Battery & Safety sets the closed-lid limit (30 minutes, 1, 2, 4,
  8 hours, never). Status lists "闔上螢幕時睡眠" with the other policy rows and
  the limit while the lid is held. The rule editor has "這條規則生效時" with
  the choice (off for new rules), the rule list states it per rule, History
  names the new cause. Every new string exists in both locales, worded the
  same as the tray.
- **Fixed on the way.** Every rule edit saved from the window showed
  `awake.gui.error.unexpected_reply`: the window sent rule edits through a
  call that expected a status and the service answers them with the rule
  list. The window now treats any reply that is not a refusal as done and
  re-reads both, as it already did.
- **External display.** Unchanged: GNOME holds its own lid inhibitor then, and
  this one changes nothing about that. The window says so beside the choice.
- **Better Manager.** Its Keep awake page is a placeholder until ticket 87.
  When ticket 87 builds it, the page shows "闔上螢幕時也保持清醒" and the limit
  in these words, read from the same status fields.
- **Manifest.** Declares the settings file, names `handle-lid-switch` in the
  `logind-inhibitor` permission, and gains a release note.

Screenshots, in a private headless GNOME Shell 46 with the AppIndicator
extension, on a private bus, with the service run against the fake inhibitor
backend (`.shots/` in the ticket worktree; the demo service has the lid
closed on battery, so the countdown shows): `01`–`07` the tray (awake menu,
"目前的設定" with the lid rows, the not-awake menu, Quick options ticked and
unticked, a session started with it unticked holding `sleep:idle` only),
`08`–`17` the window (Status rows, Session defaults, Battery & Safety and the
limit set to never and back, the rule editor and the saved rule).
`v2-client-parse.txt` is the version 2 parse check.

**Releasing the lid lock with the lid already closed suspends at once.** Read
in systemd v255's source: closing the lid installs a post event source
(`button_install_check_event_source`, `src/login/logind-button.c`) that runs
`button_recheck` after every turn of logind's event loop for as long as the
lid stays closed; each recheck calls `manager_handle_action` for the lid
(`src/login/logind-action.c`), which skips only while a `handle-lid-switch`
block lock is held, then suspends (`LidSwitchIgnoreInhibited=yes` ignores the
`sleep` lock). Releasing the lock closes its fifo, which wakes the loop, so
the next recheck suspends. The exceptions are logind's own: docked or an
external display (`HandleLidSwitchDocked`, default `ignore`), a configured
`HandleLidSwitchExternalPower`, and the hold-off after boot or resume. So
ending a session, its end time arriving, or either stop with the lid closed
puts the machine to sleep right away. The window says so next to the choice
("保持清醒結束時，如果螢幕還闔著，電腦會馬上睡眠（接著外接螢幕時除外）。"). The
tray menu has no room for the sentence and does not carry it.

Not done or not seen: no real logind inhibitor was taken, so
`systemd-inhibit --list` on a real machine and a real lid closing with the
machine staying up have not been observed, nor the immediate suspend above on
a machine. Screenshots `18`–`21` show a tray and window from this ticket
against a service answering as version 2 (the demo engine behind a version 2
facade that refuses version 3 and any document carrying a lid field). A notification raised by a real GNOME
for a closed-lid stop has not been seen; the proof is a fake notification
service on a private bus.

