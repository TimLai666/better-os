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
  `better-os-login-fallback@tty8.service` and `chvt 8` (tty8 and tty9 since
  the second review, ADR 0047); when the stop does not complete in 6 s, the
  text login at once; `/run/better-os/login-fallback.active` once the text
  login is seen active; the English issue text (the
  console cannot draw Chinese); the `BETTER_OS_LOGIN_STAGE` journal lines;
  `better-os-login-status` printing the last record (zh-TW where the
  terminal can draw it).
- No account repair at boot (`/etc` read-only); a missing or locked
  `_better-greeter` is a failed stage 5 and a record Better Manager acts on.
- Automatic login (greetd's `initial_session`): at boot, a session of that
  account within 80 s satisfies the property.
- The unit: `Type=notify`, `WatchdogSec=10`, `Restart=always`, start limit
  5 in 120 s, `OnFailure=better-os-login-backstop-failed.service`; the
  backstops' services and the timer (82 s from the supervisor's first
  start) with standard programs only; the fixed list of
  units it may act on; `org.betteros.LoginSupervisor1` read-only.
- Records in `/var/lib/better-os/boot-watch/` in ADR 0033's format, so
  Better Manager's Home and Health and `better-manager-cli doctor` show
  them with the spec's section 6 copy.
- Tests over fakes of logind, systemd, `/sys/class/drm`, Plymouth, the
  cgroup and the clock for every row of the spec's stage table, including:
  a supervisor restarted in the middle of an attempt continues from its
  state file; stopping or restarting the greeter's unit never stops the
  supervisor (the unit files are checked for `PartOf=`, `BindsTo=`,
  `Requires=`); a stop that never completes starts the text login at once;
  the budget never exceeds 80 s on the fake clock.
- From the second review (2026-11-22, `/home/tim/Developer/better-os-evidence/267/review-2026-11-22.md`):
  the greeter's session scope stopped and killed, and the card free, before
  anything starts; "no screen" only when no card shows one, with the
  attempts still run and no marker that silences the backstop; the timer
  counted from the supervisor's first start; `getty@tty1` and
  `plymouth-quit` out of the boot while this display manager is chosen;
  the text login's markers only once it is active, with retries and the
  other terminal; the failed supervisor's backstop on the present; a
  bound on the budget's margin with slow calls and restarts, a randomized
  run, and the restart case's true bound in the spec; a display manager
  started again as a new appearance; nothing stopped when greetd's tree and
  logind are both unreadable; no endless phase with the greeter's account
  unknown; records of a newer version read as unknown; and the autovt
  getty and `chvt` with a hung logind investigated.

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

2026-11-22, the second review's round (all fourteen items, evidence in
`/home/tim/Developer/better-os-evidence/267/review-2026-11-22.md`):

- The greeter's session scope is found, remembered, killed 4 s into every
  stop and waited for with the card free (ADR 0047, **proposed**: it widens
  the supervisor's reach, so the owner accepts or refuses it).
- Card classes, the no-screen path, the timer's ordering, the drop-ins for
  `getty@tty1` and `plymouth-quit`, the markers after the text login is
  active, the failed supervisor's backstop, the 120 s start limit, the
  T0 + 68 s cut-off, the start again, the unknown cases and the lenient
  records are built and tested (`rules/tests.rs`, `tests/property.rs` with
  slow calls, restart downtime and a 1500-seed randomized run,
  `tests/units.rs`); sixteen deliberate breaks of the rules across both
  rounds each failed the tests.
- The two investigations: the text logins moved to tty8 and tty9, past
  logind's automatic VTs; `chvt` with tty1 in process mode under a hung
  logind is bounded in the backstops and stated as a limit, with the
  check written into ticket 281.
- For the owner: the property's bound when the supervisor itself restarts
  during an appearance is 80 s plus its downtime (about 81 s for a crash,
  up to 96 s for a hang, 90 s at boot where the timer caps it); the spec
  states it. The text login is on tty8, not tty1.
- CI run 44957725115 passed the container check on all four runners: the
  text login on tty8 39 s after T0 with every greeter session killed, the
  restart mid-attempt continuing (40 s), a display manager started again
  beginning anew, tty8 refused moving to tty9 (43 s), getty on tty1 and
  plymouth-quit standing aside, the timer right before the supervisor, and
  the failed supervisor's backstop on tty9 in 6 to 7 s with its present-tense
  conditions. That run's `installer` job failed on one shellcheck note in
  `login-path.sh`, fixed in the commit that moved this ticket here.

2026-11-22, the third review round (`/home/tim/Developer/better-os-evidence/267/review-2-2026-11-22.md`),
every item fixed and tested:

- The drop-ins for `getty@tty1` and `plymouth-quit` skip only while the
  supervisor's own state is active, activating or reloading, read with a
  time limit and ordered after it; never by the link. Container: with the
  link set and the supervisor not started, masked, or its unit broken, and
  with another display manager, both run; while it runs, both stand aside.
- Scopes are remembered until seen gone, at every look, with no cap; the
  kill reaches the scopes seen that look too, and closing greeter sessions
  in logind's files are read for their scope (a session that opened just
  before its stop was missed otherwise: found by the randomized run).
  Property: nine logouts, then a greeter whose compositor ignores
  `SIGTERM`.
- The failed supervisor's backstop runs from a timer, at once and every
  30 s, only while the supervisor does not run, and a greeter counts as
  alive only when it says `ready`. Container: a greeter stuck on `starting`
  gets the text login; a ready greeter that goes stale gets it at the
  timer's next look.
- A text login on a machine that had no screen is stopped for the greeter
  only while its own cgroup holds nothing but a root `agetty`.
- The kill goes through pidfds, the scope read again at that moment; DRM
  holders count only when they are the greeter's.
- A restart that cut a stop short gives it its kill and 2 s to reap once
  more. Restated bounds, measured in `tests/property.rs` with every call at
  2 s, latency in the looks and at start, crashes between ticks and in the
  middle of one: 86 s after a crash, 101 s after a hang, 86 s for tty8
  refused; 90 s at boot under the timer. The randomized run (2000 seeds)
  now has 16 s hangs, crashes mid-tick, unknown sessions, a person's
  session and a person's compositor holding a card.
- Investigated: a hung logind keeps the DRM master through its own copy of
  the descriptor; the supervisor may not end logind and no longer waits for
  it; stated in the spec's limits, checked in ticket 281.
- Corrected: no released Better Manager reads these records at all (the
  reader came with ticket 140 after v0.2.16), so nothing released drops
  them; the lenient reading is for versions to come.

2026-11-22, the decision: the owner accepted ADR 0047 ("接受 ADR 0047 嗎？",
answered "接受"). AGENTS.md's rule for the supervisor now has the ADR's
text (the greeter session's scope, the DRM card holders, tty8 and tty9);
the ADR and `engineering/README.md` say accepted.

The "Done when" items, checked against what was built and proved:

- Every stage's rule has a unit test (`src/rules/tests.rs`), and the
  property is asserted on the fake clock for every injected outcome
  (`tests/property.rs`, with the 2000-seed randomized run); the restart
  bounds the spec states are measured there.
- The container check (`packaging/boot-watch/login-path.sh`, run by
  `run.sh`) has the stand-in greeter that never becomes ready and the
  supervisor that cannot start, and passed in CI run 44966908849 on all
  four runners: the text login in the first case, the backstop in the
  second, with every later case of the review rounds.
- The VM proof is ticket 281's; what it checks, and what tickets 264 and
  265 still owe the supervisor, is written in those tickets' notes.

Nothing of this ticket's own is left.
