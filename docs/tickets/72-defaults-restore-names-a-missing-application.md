# Ticket 72 — Restore says when the application it would put back is gone

**Epic:** Better Defaults (Issue #10) · **User Story:** a person whose previous
image viewer has been uninstalled is told so on restore and can choose another
· **Branch:** `ticket-71-72` · **Blocked by:** none · **Status:** ready

## What it delivers

Restore writes the captured desktop entry whether or not that application is
still installed, and reports what the verifying read saw. The owner decided on
2026-09-27 that restore checks the shared application catalog first: a
previous target that no longer exists is its own reported class, nothing is
written for it, and the person can choose another application or clear the
default.

## Acceptance criteria

- [ ] Restore looks the captured desktop entry up in the shared application
      catalog at restore time; a missing entry is reported as "the previous
      application is no longer installed", naming it, and nothing is written
      for that integration.
- [ ] The review screen and the CLI show that class distinctly, and offer the
      catalog's applications for that type or clearing the default (ticket
      71's operation).
- [ ] A group restored type by type (ticket 58) reports the missing class per
      type and restores the others.
- [ ] Tests use a temporary catalog and `XDG_CONFIG_HOME`.
