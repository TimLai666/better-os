# Ticket 63 — The left and right arrow keys move Better Launcher's selection

**Epic:** Better Launcher (Issue #2) · **User Story:** a person moving through
the launcher grid with the keyboard can go left and right as well as up and
down · **Branch:** `ticket-63` · **Blocked by:** none · **Status:** ready

## What it delivers

Up and down move the selection in Better Launcher's grid; left and right do
not, although `key_action` maps them to the selection. The search field is the
likely taker. After this ticket the cause is found and left and right move the
selection, while the search field still gets its own text editing where that
does not conflict.

## Acceptance criteria

- [ ] The cause is identified with evidence, not assumed.
- [ ] Left and right move the selection one cell in the grid, and at the ends
      of a row behave the way up and down behave at the ends of the list.
- [ ] Typing in the search field still works; if left and right can no longer
      move the text cursor, the ticket says so and why that trade is right.
- [ ] A test covers the key path, and the behaviour is confirmed by running
      the launcher in a nested or headless compositor.
