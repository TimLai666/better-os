# Ticket 109 — Keep awake also covers closing the lid

**Blocked by:** none · **Status:** ready

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

- [ ] With a manual session, `systemd-inhibit --list` (or the fake backend in
      tests) shows `handle-lid-switch` held; ending the session releases it.
- [ ] Unit and session-bus tests for every case above.
- [ ] The window, tray, and Better Manager show the choice; screenshots.
- [ ] Tests never hold a real inhibitor or touch the person's real service.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      build-deb plus verify-deb, and the license check pass.
