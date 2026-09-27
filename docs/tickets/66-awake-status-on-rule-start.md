# Ticket 66 — Better Awake's menu shows a session a rule started on its own

**Epic:** Better Awake (Issue #13) · **User Story:** a person whose rule just
started keeping the machine awake sees it in the tray menu without waiting ·
**Branch:** `ticket-66` · **Blocked by:** none · **Status:** ready

## What it delivers

Ticket 59 made the service push a status after a tick that ends a session. A
tick that only starts a rule's session still pushes nothing, so the tray menu
lags until the next unrelated change. After this ticket any tick that changes
the set of sessions or the refused rules pushes one `StatusChanged`, and a tick
that changes nothing pushes nothing.

## Acceptance criteria

- [ ] A tick that starts a rule session pushes exactly one `StatusChanged`.
- [ ] A tick that changes nothing pushes nothing.
- [ ] A tick that both ends and starts sessions pushes the `SessionEnded`
      events first and one status after them.
- [ ] Tests run on a private session bus and write only temporary state.
