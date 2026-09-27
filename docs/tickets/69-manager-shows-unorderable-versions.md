# Ticket 69 — Better Manager says when a package's version cannot be compared

**Epic:** Better Manager (Issue #8) · **User Story:** a person whose component
was installed with a version the manager cannot order sees that it exists and
why it is not managed · **Branch:** `ticket-68-69` · **Blocked by:** none ·
**Status:** ready

## What it delivers

A dpkg version that is not a semantic version keeps a package invisible:
reconciliation adopts only what `semver` can parse, and nothing reports the
rest. The owner decided on 2026-09-27 that such a package is shown as a doctor
finding: seen, explained, and not managed.

## Acceptance criteria

- [ ] `better-manager-cli doctor` and the GUI's equivalent list each Better OS
      package dpkg holds with a version `semver` cannot parse, with the version
      as dpkg reports it and what to do about it.
- [ ] Nothing is adopted, planned, or changed for such a package.
- [ ] A package whose record exists and whose host version becomes unorderable
      keeps its existing blocking drift finding; this ticket adds only the
      unrecorded case.
- [ ] Tests cover an epoch (`1:0.2.8`), a Debian revision (`0.2.8-1ubuntu1`),
      and a tilde version.
