# Ticket 87 — Control center: Keep awake

**Blocked by:** 84 · **Status:** ready

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

- [ ] Each Better Awake row of spec section 12 is shown working, including the
      two ✱ rows: a session started from "Until a time" ends at that time
      (test with a clock seam), and a length added on the page appears in the
      tray menu (test against the menu value, and seen in the sandbox with
      `dbus-run-session`).
- [ ] A preferences file from today's `awake-gui` is imported once, a second
      start does not import again, and a newer-schema file is left alone
      (tests).
- [ ] Audit A1–A8 each closed with a screenshot.
- [ ] Light and dark, zh-TW and English, 1600 and 960, screenshotted with the
      service running inside the sandbox's session bus.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
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
