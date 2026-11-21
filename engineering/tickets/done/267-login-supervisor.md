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

- [x] Every stage's rule has a unit test; the property is asserted on the
      fake clock for every injected outcome.
- [x] The container check `packaging/boot-watch/run.sh` gains a stand-in
      greeter that never becomes `ready` and a supervisor that cannot
      start, and passes in CI: the fall back in the first case, the
      backstop in the second.
- [x] The VM proof is ticket 281's (its notes say what it runs against).

## Notes

2026-11-22, built:

- `crates/login-supervisor`: `better-os-login-supervisor` and
  `better-os-login-status`, no GPUI. The rules are `rules.rs` with no I/O
  (stages 1 to 3 in 12 s with `plymouth deactivate` inside them, three
  attempts of 15 s with the third in safe mode, stops of at most 6 s, the
  fall back, stage 1a, no screen, stage 9 from logind and greetd's tree,
  automatic login, the unreadable state file); the edges are `read.rs`
  (each from a path a test points at a fixture) and `system.rs`; the loop is
  `world.rs`; `/run/better-os/login-path.json` is `state.rs`;
  `org.betteros.LoginSupervisor1` (`LastRecord`, read-only) is `bus.rs`.
- The units, bus policy and the backstop's message in `packaging/greeter/`.
- Records: `manager_ipc::boot_watch` gained `LoginStage` and the outcomes
  `login_screen_restarted` and `text_login_used`, and now holds the records'
  writer both the watch and the supervisor call (moved from
  `manager-daemon`); Home, Health and `doctor` show them with section 6's
  copy (`crates/manager-gui/src/login_watch.rs`,
  `crates/manager-cli/src/main.rs`).
- The spec (section 5.10's units table, stages 1, 4, 5, 6, 9 and F, the
  records and the status command's copy, section 6's "twice" row) and the
  security review were updated first with what building it settled: T0
  from `InactiveExitTimestampMonotonic`, the heartbeat judged by change,
  `/run/better-os/greeter-mode`, the backstop's third condition
  (`/run/better-os/login-held`, without which it would take an automatic
  login's screen at 90 s) and `AccuracySec=1s`, and no `Conflicts=` on the
  supervisor (GDM's `Conflicts=plymouth-quit.service` would let the
  greeter's own `OnFailure=` stop it).

Checked: `tests/property.rs` runs the supervisor's real loop over a fake
world for every row of the spec's stage table and injection table, with
the property asserted on the fake clock, including restarts mid-attempt,
mid-stop and mid-fall back, a stop that never completes (tty2), a refused
tty1, and the worst case with every stop at its limit; `rules/tests.rs`
each rule alone; `tests/units.rs` the unit files; seven deliberate breaks
of the rules (resume ignored, no tty2, a stop with a person possible,
stage 5 late, no safe mode, logind trusted over greetd's tree, no
Plymouth kill) each failed the tests. `packaging/boot-watch/login-path.sh`
is the container check (stand-in greeter that never becomes ready, the
supervisor killed mid-attempt, a supervisor that cannot start); this
machine has no Docker, so its first run is CI's.

The container check's first CI run (run 44946611014) passed case A on
all four runners and found a real fault: systemd 255 triggers
`OnFailure=` on every failure of a `Restart=always` unit, so one
supervisor crash started the backstop's text login on tty2. Fixed with
`RestartMode=direct` (spec, security review and `tests/units.rs`
updated); the check now also kills by process id, since `systemctl kill`
fails on two runners' kernels.

With it, CI run 44950253766 passed on all four runners: the text login
on tty1 27 s after T0 with three attempts (the third in safe mode), the
supervisor killed mid-attempt continuing to the text login 27 to 28 s
after the first T0 with the backstop not run, and a supervisor that
cannot start ending in the backstop's text login on tty2 in 6 to 7 s. What needs
greetd, the real greeter, the package or the VM is in the notes of
tickets 264, 265 and 281.

