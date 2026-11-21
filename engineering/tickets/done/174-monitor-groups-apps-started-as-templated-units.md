# Ticket 174 — Better Monitor groups applications started as `app-<id>@<random>.service`

**Blocked by:** none

## Why

Found on 2026-11-21 in ticket 143's COSMIC VM. systemd names an application's
unit `app-<id>@<random>.service` when a desktop starts it as a templated
unit, and COSMIC does. `strip_launch_suffix` in
`crates/monitor-views/src/grouping.rs` does not strip the `@<random>` part, so
the application id keeps it and the Processes page groups the application
wrongly.

## What to build

- Parse the unit name by systemd's own rules for application units (the
  `app[-<launcher>]-<ApplicationID>[@<RANDOM>].service` and
  `…-<RANDOM>.scope` forms in systemd's desktop environment integration
  document), unescaping the id, in the one grouping function.

## Done when

- Tests for each form, with ids that contain dashes and escaped characters.
