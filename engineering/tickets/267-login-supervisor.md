# Ticket 267 — The login supervisor and its backstop: a login prompt within 80 seconds, whatever fails

**Spec:** `design/login-screen-spec.md` section 5.10; `engineering/specs/login-screen-security.md` (the supervisor, the backstop); ADR 0044 and its amendment of 2026-11-22; ADR 0033 (the records' format)
**Blocked by:** 260

## Why

The owner's requirement (2026-11-21): a boot must never stay on the splash
with no login screen (ticket 139). On the Better OS path every stage
between the display manager's start and a prompt accepting input has a
deadline, and the last fall back is a text login that says what failed. The
review of 2026-11-22 found the first design's unit wiring would have
stopped the supervisor on its own retries; this ticket builds the corrected
one.

## What to build

- `better-os-login-supervisor`, a root Rust service in the
  `better-os-greeter` package, **the display manager** (the unit
  `display-manager.service` aliases), with its rules (stages, deadlines,
  attempts, stops, what each outcome does) in a module with no I/O and the
  process edges thin: stages 1 to 3 within 12 s, then three attempts of
  15 s at the greeter (the first, a plain start, safe mode) with a stop of
  at most 6 s between them, then the fall back; it alone starts
  `better-os-greeter.service`.
- Its state (T0 from its first start's `ActiveEnterTimestampMonotonic`, the
  attempt, the stage) in `/run/better-os/login-path.json`, so a watchdog
  restart continues the appearance.
- Stage 1 by polling `/sys/class/drm` (no udev monitor); the real driver
  binding later restarts the greeter only while no person's session runs;
  a card with no connected screen starts the greeter and records "no
  screen", with a new T0 when a screen is connected.
- Stage 9 from logind **and** from greetd's cgroup (a session worker present
  in `better-os-greeter.service`'s cgroup with no fresh heartbeat means a
  person's session; its disappearance with no heartbeat within 15 s means
  it ended), so a logind that crashed at logout (ticket 256) still leads to
  a new appearance.
- Plymouth in every path, each command with a 5 s limit, `plymouthd` killed
  when one does not return; the unit closes the network with
  `IPAddressDeny=any`, never `PrivateNetwork=` (which hides `plymouthd`'s
  abstract socket).
- The fall back: stop the greeter's unit (at most 6 s), `plymouth quit`,
  `KDSETMODE KD_TEXT` and `VT_SETMODE VT_AUTO` on `/dev/tty1`,
  `better-os-login-fallback@tty1.service` and `chvt 1`; when the stop does
  not complete in 6 s, `@tty2` and `chvt 2` at once;
  `/run/better-os/login-fallback.active`; the English issue text (the
  console cannot draw Chinese); the `BETTER_OS_LOGIN_STAGE` journal lines;
  `better-os-login-status` printing the last record (zh-TW where the
  terminal can draw it).
- No account repair at boot (`/etc` read-only); a missing or locked
  `_better-greeter` is a failed stage 5 and a record Better Manager acts on.
- Automatic login (greetd's `initial_session`): at boot, a session of that
  account within 80 s satisfies the property.
- The unit: `Type=notify`, `WatchdogSec=10`, `Restart=always`, start limit
  5 in 60 s, `OnFailure=better-os-login-backstop.service`; the backstop's
  service and 90 s timer with standard programs only; the fixed list of
  units it may act on; `org.betteros.LoginSupervisor1` read-only.
- Records in `/var/lib/better-os/boot-watch/` in ADR 0033's format, so
  Better Manager's Home and Health and `better-manager-cli doctor` show
  them with the spec's section 6 copy.
- Tests over fakes of logind, systemd, `/sys/class/drm`, Plymouth, the
  cgroup and the clock for every row of the spec's stage table, including:
  a supervisor restarted in the middle of an attempt continues from its
  state file; stopping or restarting the greeter's unit never stops the
  supervisor (the unit files are checked for `PartOf=`, `BindsTo=`,
  `Requires=`); a stop that never completes moves the fall back to tty2;
  the budget never exceeds 80 s on the fake clock.

## Done when

- [ ] Every stage's rule has a unit test; the property is asserted on the
      fake clock for every injected outcome.
- [ ] The container check `packaging/boot-watch/run.sh` gains a stand-in
      greeter that never becomes `ready` and a supervisor that cannot
      start, and passes in CI: the fall back in the first case, the
      backstop in the second.
- [ ] The VM proof is ticket 281's.
