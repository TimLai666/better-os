# Ticket 216 — The notification center opens on the right edge of the session

**Spec:** better-os-session.md S4, S6 (notification center)
**Blocked by:** 215

## Why

Ticket 161's second half: a panel the height of the work area on the right
edge, as on macOS, opened by the bell and the clock, Super+V and Super+M,
and a gesture. In the session it is a layer surface fed by ticket 215's
service.

## What to build

- **The center** as ticket 161's design (and its N2 answer: one list, the
  calendar at the top if so decided): a layer surface on the top layer,
  right edge, no exclusive zone, keyboard on demand, light-dismiss; the list
  grouped by application, Clear all, Do not disturb.
- **Opening**: the top bar's bell and clock (ticket 158 in the session),
  Super+V and Super+M in the session's defaults, Escape and a click outside to
  close; a slide that follows `TransitionUpdate` progress for kind
  notification center (accepted in a test; the gestures are tickets 224 and
  225).
- Accessible names, roles, and copy in both languages.

## Done when

- [ ] Unit tests for grouping, Clear all, and the open and close rules.
- [ ] In the VM session with ten notifications from three applications, the
      center opened each way and closed each way; screenshots light and dark
      in `/home/tim/Developer/better-os-evidence/216/`.
- [ ] fmt, clippy on stable, and the crates' tests pass.
