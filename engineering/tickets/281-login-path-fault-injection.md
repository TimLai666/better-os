# Ticket 281 — The login path under injected failures: a prompt within 80 seconds, every time

**Spec:** `design/login-screen-spec.md` section 5.10 ("How it is proven"); ADR 0044 (the order, step 3) and its amendment of 2026-11-22
**Blocked by:** 265, 267

## Why

The property the owner asked for is only as good as its proof. Each
failure the spec lists is injected in a VM and the property is checked on
the screen a person would see.

## What to build

- In `packaging/tests/session-vm/`, on Ubuntu 24.04 and Zorin OS 18, a
  runner that boots (or logs out) once per row of the spec's injection
  table (every row, including the supervisor failing to start, crashing in
  a loop and hanging; greetd hanging; a compositor that ignores `SIGTERM`;
  logind hanging; Plymouth hanging before T0; a card with no screen; tty1
  left in graphics mode) and checks: within 80 seconds of the display
  manager starting (90 for the supervisor's own failures), a screenshot of
  the VM's screen showing either the greeter or the text login with its
  message, a login typed with the VM's keyboard on that screen succeeding,
  `plymouthd` not running, and the expected `BETTER_OS_LOGIN_STAGE` line. A
  login over the serial console does not count.
- Investigated and recorded with evidence:
  - whether tty1 comes back to text by itself after `cosmic-comp` and
    `plymouthd` that held it in graphics mode are killed with logind
    stopped, and that the supervisor's `KD_TEXT` reset makes it visible;
  - which signal the supervisor used to see a session's end when logind
    was killed at logout, and that greetd's cgroup gave it;
  - whether the text console draws on tty2 while a process in
    uninterruptible sleep still holds the DRM device.
- The normal boot measured ten times; the time to `ready` recorded.
- First with a minimal test greeter, then with `better-greeter`.

## Done when

- [ ] Every row passes on both bases, twice in a row, with evidence in
      `/home/tim/Developer/better-os-evidence/281/`.
- [ ] Each investigated item has a written result in the spec's section
      5.10 or its "What it cannot promise".
- [ ] The spec's section 7 and the security review's "Not proven" lose the
      deadlines this proves.

## Notes

From ticket 267 (2026-11-22): what the VM runs against.

- `BETTER_OS_TEST_SUPERVISOR_ABORT=<stage code>` (`6` for the compositor)
  is implemented in `better-os-login-supervisor`: set it in a drop-in of
  the supervisor's unit for the "crashing repeatedly" row.
- Proven on fakes only (`crates/login-supervisor/tests/property.rs`): every
  row of the spec's injection table as an outcome the fake world produces,
  the property asserted on the fake clock. Proven on a real systemd in a
  container (`packaging/boot-watch/login-path.sh`, CI): a stand-in greeter
  that never becomes ready ending in the text login within 80 s, the
  supervisor killed mid-attempt continuing with the same T0, and a
  supervisor that cannot start ending in the backstop's text login on tty2
  within 90 s. Not run against the real thing anywhere: greetd, Plymouth, a real terminal's
  `KDSETMODE`/`VT_SETMODE`/`VT_ACTIVATE`, a real DRM card, logind hanging or
  crashing, the watchdog of a hung supervisor (`SIGSTOP`), and the
  backstop's 90 s timer firing (the container check runs the `OnFailure=`
  path only). Those are this ticket's.
- The stage 9 sign the supervisor uses is greetd's tree: the processes in
  `better-os-greeter.service`'s cgroup and their descendants (through
  `/proc/<pid>/task/*/children`), a person's session being one of them
  running as an account other than root and `_better-greeter`. Whether a
  session's processes stay greetd's descendants when `pam_systemd` moves
  them into the session's scope is what this ticket checks with logind
  killed at logout.
