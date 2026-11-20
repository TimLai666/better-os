# Ticket 176 — What Better Defaults applied is put back when its component is removed

**Blocked by:** none

## Why

AGENTS.md: uninstalling removes everything the component added, and the
keybindings and defaults it set are restored to what they were. Ticket 207's
worker found on 2026-11-21 that `manager-core` never calls the defaults
engine on removal: the only uninstall-time restore is Better Touchpad's
before-removal step, and ticket 129 adds one for Better Files' activation
file. A component whose manifest declares other integrations (keybindings,
desktop settings, launcher entries) leaves them applied after it is gone.

## What to build

- Removing a component restores every integration the engine recorded as
  applied for it, through the engine's own restore (the changed-externally
  rule holds: a value the person changed afterwards is left and named), from
  whichever session the removal runs in (ticket 207: a record names its
  database).
- The same when the package is removed without Better Manager (apt, Software):
  the per-person step tickets 90 and 129 built for their own cases is the
  pattern; one mechanism for every component, not one copy per package.
- Disabling a component: decide and record whether its integrations are
  restored on disable and applied again on enable (ticket 129 decides this
  for the activation file; the rule should be one rule).
- Better Touchpad's own capture records which dconf database it was taken
  from, as ticket 207 did for the engine's records, so a restore from the
  other session reaches the right one.

## Done when

- Tests: remove from the same session, from the other session, with a value
  changed externally, with no session at all (pending, finished at the next
  start), and by apt in the container end-to-end check.

## Notes
- 2026-11-21, from ticket 129's second review: besides before-removal there
  is now a second per-package program, `/usr/lib/better-os/on-disable/<package>`
  (manager-core's `run_disable_hook`, the same `refuse_program` rules, exit
  3 and 4, `--undo` on enable), and Better Manager's start finishes every
  component's pending put-back or take-back by the record
  (`defaults_core::removal::finish_in_session`). Better Files is the only
  package shipping it (its program is `defaults_core::removal`'s, from
  `better-files-before-removal`). This ticket's one mechanism should take
  both in, so no package ships its own copy.
