# ADR 0022: Better Manager executes for real by default

## Status

Accepted on 2026-08-01 (tickets 10, 13 and 14). Written as a record on
2026-11-15 from the delivery status decision log.

## Decision

- The CLI and the window apply transactions for real by default. Simulation is
  an explicit demo mode that says so on screen. A missing privileged service is
  an error, never a reason to simulate.
- A transaction can be cancelled only while it is still downloading. Once the
  plan has gone to the privileged service, no cancel control is offered.

## Why

A manager that quietly simulated would report a change that never happened.
After the plan reaches the service the host may already have changed, and a
cancel button there would promise a restoration nothing performs.

## Options rejected

- Mock execution as the default with real execution opt-in: hides a missing
  daemon behind a success.
- Cancelling at any stage: cannot be honored once dpkg runs.

## Consequences

- Tests and demos choose the mock path explicitly (ADR 0005, ADR 0007).
- Recovery from a run that was interrupted is the daemon's repair (ADR 0014),
  not a cancel.
