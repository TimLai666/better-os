# Ticket 267 — The login screen watch covers greetd and falls back to GDM

**Spec:** `engineering/specs/login-screen-security.md` ("What a crash or a hang leaves the person with"); ADR 0033; ADR 0044 ("When it fails on its own")
**Blocked by:** 265

## Why

ADR 0033's watch does nothing when `display-manager.service` resolves to
another display manager, and restarts only `gdm.service` or `gdm3.service`.
A greeter that hangs while still running is invisible to greetd and to
systemd.

## What to build

- In `manager_daemon::boot_watch`: when the display manager is
  `better-os-greeter.service`, "the login screen" is a session of class
  `greeter` on `seat0` *and* a heartbeat in `/run/better-os-greeter/alive`
  younger than 30 s; the same once-per-boot rule, records and evidence as
  ADR 0033; the first step restarts `display-manager.service`, the second
  stops the greeter's unit and starts `gdm.service` for this boot.
- An `OnFailure` start of GDM is recorded too (the watch sees GDM's greeter
  while the alias names Better OS's), so Better Manager's Home and Health
  show "The Better OS login screen didn't start last time…".
- The container check `packaging/boot-watch/run.sh` gains a stand-in
  greeter unit that never writes a heartbeat.
- ADR 0033 is accepted and is not edited; this behaviour is ADR 0044's.

## Done when

- [ ] The container check passes in CI with the stand-in.
- [ ] In the VM, a greeter stopped with `SIGSTOP` is replaced by GDM within
      the deadline and the record says so.
