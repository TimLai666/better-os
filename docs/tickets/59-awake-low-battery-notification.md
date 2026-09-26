# Ticket 59 — Better Awake tells the person when low battery ends a session

**Epic:** Better Awake (Issue #13) · **User Story:** a person whose keep-awake
session was stopped by low battery learns it from a desktop notification ·
**Branch:** `ticket-59` · **Blocked by:** none · **Status:** ready

## What it delivers

A low-battery stop writes history and prints to stderr. The service emits no
signal for a stop made by its own tick, and the tray drops `SessionEnded`. After
this ticket the service emits `SessionEnded` with its cause for every session it
ends on its own, and the tray raises one desktop notification through
`org.freedesktop.Notifications` for a low-battery stop.

## Acceptance criteria

- [ ] Every session the service ends without a client request emits
      `SessionEnded` with its cause, once.
- [ ] The tray raises one notification per low-battery stop, in zh-TW or
      English following the tray's existing label selection, naming the
      battery threshold that was crossed.
- [ ] No notification daemon, or a failed call, is logged and does not stop
      the tray.
- [ ] Tests use a private session bus or a fake notifier. No test raises a
      notification on the developer's desktop.
- [ ] The AGENTS.md follow-up about the unwired notification can be removed.
