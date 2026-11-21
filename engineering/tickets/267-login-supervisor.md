# Ticket 267 — The login supervisor: a login prompt within 60 seconds, whatever fails

**Spec:** `design/login-screen-spec.md` section 5.10; `engineering/specs/login-screen-security.md` (the supervisor's limits); ADR 0044; ADR 0033 (the records' format)
**Blocked by:** 260, the owner's decisions L1 (B or C) and L8

## Why

The owner's requirement (2026-11-21): a boot must never stay on the splash
with no login screen (ticket 139). On the Better OS path nothing may be
left to hope: every stage between the display manager's start and a prompt
accepting input has a deadline, and the last fall back is a text login that
says what failed. ADR 0033's watch does nothing when the display manager is
not GDM; this supervisor replaces it on this path.

## What to build

- `better-os-login-supervisor`, a root Rust service in the
  `better-os-greeter` package, with its rules (stages, deadlines, attempts,
  what each outcome does) in a module with no I/O and the process edges
  thin: stage 1 to 3 within 12 s, three attempts of 15 s at the greeter (the
  first, one plain restart, one in safe mode), then the fall back, as the
  spec's table says; a new appearance after every session that leaves
  `seat0` and whenever the greeter's heartbeat goes stale after `ready`.
- Plymouth in every path: `plymouth deactivate` before the greeter starts,
  `plymouth quit --retain-splash` at `ready`, `plymouth quit` on fall back,
  `plymouthd` killed when a command does not return in 5 s.
- The fall back: `better-os-login-fallback@.service` (`agetty` with
  `--issue-file /etc/issue:/run/better-os/login-fallback.issue`), the issue
  text of the spec in the system's language, `chvt 1`, the
  `BETTER_OS_LOGIN_STAGE` journal lines; `better-os-login-status` printing
  the last record.
- The unit: `WatchdogSec=10`, the sandbox of the security review, the fixed
  list of units it may act on; `org.betteros.LoginSupervisor1` read-only.
- Records in `/var/lib/better-os/boot-watch/` in ADR 0033's format, so
  Better Manager's Home and Health and `better-manager-cli doctor` show
  them with the spec's section 6 copy.
- Tests over fakes of logind, systemd, `/sys/class/drm`, Plymouth and the
  clock for every row of the spec's stage table, including a supervisor
  restarted by its watchdog in the middle of an appearance.

## Done when

- [ ] Every stage's rule has a unit test; the property is asserted on the
      fake clock for every injected outcome.
- [ ] The container check `packaging/boot-watch/run.sh` gains a stand-in
      greeter that never becomes `ready`, and passes in CI.
- [ ] The VM proof is ticket 281's.
