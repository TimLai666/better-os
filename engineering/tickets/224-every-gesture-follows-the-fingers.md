# Ticket 224 — Every system gesture follows the fingers, can be turned back, and settles

**Spec:** better-os-session.md S6 ("From the service to the surfaces", the latency budget)
**Blocked by:** 223, 210, 212, 214, 216

## Why

The owner's requirement (2026-11-21): every gesture has the transition macOS
has. With ticket 223 the gestures reach Better OS; this ticket makes each
surface follow them.

## What to build

- **The transition signals** on `org.betteros.TouchpadGestures1`
  (`TransitionBegin`, `TransitionUpdate`, `TransitionEnd`, the kinds as a
  closed enum) as the spec's contract, emitted from the recognizer's
  continuous progress; the commit decision (threshold, velocity) in
  `touchpad-gestures`, one place.
- **Each surface follows**: the overview and the application-windows mode
  (ticket 212's interpolation), Better Launcher (ticket 210: the panel grows
  in with the pinch), the desktop (thumb and three spreading: windows move
  aside to show ticket 214's desktop, through the workspace driver or the
  overview's layout), and the notification center (ticket 216's slide); the
  workspace switch through ticket 223's driver. Each accepts the signals only
  from the service's executable (ADR 0035's check), settles with one spring
  in `better-ui` started at the release velocity, catches a settle on
  `hold_begin`, and follows by opacity with reduced motion on.
- **Measure** the shipped path as ticket 202 measured the prototype, and
  record it in `engineering/performance/session-gestures.md`.

## Done when

- [ ] Unit tests for the commit rule at each threshold and velocity, a
      reversal past zero, a hold during a settle, and each surface's
      interpolation.
- [ ] In the VM with replayed recordings: each gesture opened halfway and
      turned back, completed slowly, and flicked; screen recordings in
      `/home/tim/Developer/better-os-evidence/224/`.
- [ ] The added latency within the budget the owner set (S5) at p95, or the
      ticket stays open with the numbers.
- [ ] Flow 2 run again on this compositor passes (spec, "Ready to become the
      default" item 2).
- [ ] AGENTS.md's gesture follow-ups gain the session's state (ADR 0040's W3
      if accepted, a dated line otherwise).
- [ ] fmt, clippy on stable, and the crates' tests pass.
