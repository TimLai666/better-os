# Ticket 177 — CI never passes a test that did not run

**Blocked by:** none

## Why

Found on 2026-11-21: tests that need a program return early with "skipping"
when it is missing, and still count as passed. CI had no `dconf-service`, so
ticket 207's private-bus tests never ran there until its worker made them
fail under `CI=true`. The GJS extension tests
(`crates/window-attention-service/tests/extension.rs`,
`crates/touchpad-session/tests/extension.rs`, which assert AGENTS.md's bounds
on the shell extensions) still skip quietly, because CI does not install
`gjs`. Other `*_or_skip!` helpers need only `dbus-daemon`, which CI has.

## What to build

- One helper every such test uses: a missing prerequisite is a skip on a
  developer's machine and a failure when `CI=true`.
- The CI image installs what the tests need (`gjs` at least; list each
  prerequisite in the workflow with the tests that need it).
- A check in CI that the test log contains no skip line.

## Done when

- Removing a prerequisite from the CI install step turns CI red.
