# Ticket 61 — Pressing Escape closes Better Launcher, and a test says so

**Epic:** Better Launcher (Issue #2) · **User Story:** a person can dismiss the
launcher overlay with Escape · **Branch:** `ticket-60-62` · **Blocked by:**
none · **Status:** ready

## What it delivers

Better Launcher has no titlebar by design, so Escape is the only way to close
it, and no test covers that key path. After this ticket the key-to-action
decision is a pure function outside GPUI and the overlay calls it, so the
Escape path is tested.

## Acceptance criteria

- [ ] Escape maps to close; the other keys the overlay handles map to their
      current actions, with a test for each.
- [ ] The overlay's behaviour is unchanged.
