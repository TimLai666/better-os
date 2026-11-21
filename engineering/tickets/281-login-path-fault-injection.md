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
