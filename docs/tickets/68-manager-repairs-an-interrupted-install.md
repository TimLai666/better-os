# Ticket 68 — Better Manager can repair a package transaction a crash interrupted

**Epic:** Better Manager (Issue #8) · **User Story:** a person whose machine lost
power in the middle of an install can finish it from Better Manager instead of
a terminal · **Branch:** `ticket-68-69` · **Blocked by:** none · **Status:** ready

## What it delivers

A transaction interrupted by a crash or power loss leaves dpkg with half-
configured packages, and every later apt operation refuses until someone runs
`dpkg --configure -a` as root. The project owner decided on 2026-09-27 that the
privileged daemon offers that repair. It is a new privileged operation, so it
changes the protocol ADR 0007 decided and needs ADR 0014 first.

## Acceptance criteria

- [ ] ADR 0014 records the operation: what it runs (`dpkg --configure -a` and
      nothing that takes an argument from the client), the polkit action and
      its default authorization, how it is detected that a repair is needed,
      and what the daemon refuses.
- [ ] The daemon detects an interrupted state from dpkg's own record rather
      than from an error string, and reports it to clients.
- [ ] The repair is a method with no free-text argument, authorized by its own
      polkit action, serialized with every other transaction, and its outcome
      and log tail are reported like an install's.
- [ ] `better-manager-cli repair` and a control on the screen that shows the
      interrupted state both reach it through `manager-core`; neither runs
      anything privileged itself.
- [ ] Tests use the daemon's fake APT/dpkg driver; the new polkit action ships
      in the daemon package and `verify-deb.sh` checks it.
