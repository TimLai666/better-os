# Ticket 74 — Better Awake tells the person about a low-battery stop even with no tray

**Epic:** Better Awake (Issue #13) · **User Story:** a person who does not run
the tray still learns that low battery ended their session · **Branch:**
`ticket-74` · **Blocked by:** 66 (same service code) · **Status:** blocked

## What it delivers

The tray raises the low-battery notification, so with no tray running a stop is
only in History and on stderr. The owner decided on 2026-09-27 that the service
raises it itself when no tray is there to do it, and exactly one notification
is shown either way.

## Acceptance criteria

- [ ] The service knows whether a tray is running (for example by the tray
      owning a well-known bus name) without polling.
- [ ] With a tray, the tray notifies and the service does not; without one, the
      service does; never both, never neither.
- [ ] A notification failure is logged and changes nothing else.
- [ ] Tests use a private session bus with a fake notification service and
      write only temporary state.
