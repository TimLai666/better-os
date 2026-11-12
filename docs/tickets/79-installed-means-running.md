# Ticket 79 — An installed component runs, with nothing for the person to type

**Blocked by:** none · **Status:** merged, not released

## Why

On the owner's machine Better Awake, Better Monitor, and Better Touchpad were
installed and shown as enabled and healthy, and none of their background
services had ever run. Better Awake's tray icon never appeared, because the
tray asks the service on start, gets no answer, and exits; the touchpad
gestures never worked. The packages install their systemd user units without
enabling them, by design: enabling is Better Manager's step. That step was
never written. `DesiredOperation::Enable` in `manager-core` sets
`record.enabled = true` and nothing else, and install and update do the same.

The owner's rule: installing a component makes it work. Nobody is asked to
run a command.

## What to build

1. **Which units a component has.** The `.service` files its package
   installed under `/usr/lib/systemd/user/`, read from dpkg's file list
   (`PackageStateProbe::installed_files`). No list of unit names is kept by
   component ID, and the manifest schema does not change.
2. **A user-session unit controller** in `manager-platform`, next to the dpkg
   probe: a trait with `enable_now(&[unit])`, `disable_now(&[unit])`, and
   `states(&[unit]) -> (enabled, active)` per unit; a real implementation
   that runs `systemctl --user` with an argument vector, no shell, unit names
   validated as plain `name.service`; and a fake for tests. It needs no
   privilege and never goes through `manager-daemon`.
3. **The driver uses it.** `RealDriver` gains it through a builder method
   added beside `new` (`new`'s signature does not change). After a successful
   install, update, enable, or restore that leaves the component enabled, its
   units are enabled and started before the health stage; disable stops and
   disables them; remove stops and disables them before the package goes. A
   unit that fails to start fails the step with evidence
   `user_units.failed` and the unit and `systemctl`'s message in the detail,
   in both languages in the window, and the component is not marked healthy.
4. **Health includes it.** For an enabled component, the health check also
   requires every unit to be enabled and active. A unit that is not is
   reported by name, not as a generic failure.
5. **Existing machines heal on their own.** When Better Manager starts (GUI
   and CLI), every component it records as enabled whose units are not
   enabled and running has them enabled and started, and the activity log
   says so. That is the recorded intent, so it is not asked again. This is
   what fixes the owner's machine after the update, with nothing typed.
6. Wiring in `manager-gui` and `manager-cli` so the real controller is used
   wherever `RealDriver` is built.

## Out of scope

- The windows' look (the Fluent pass is its own ticket).
- Units of the system bus or system manager; only user units exist today.

## Acceptance

- [x] With the fake controller and fake dpkg file lists: install, update,
      enable, restore, disable, remove, and a start failure each do what
      section 3 says, and health reports a stopped unit by name.
- [x] Startup reconciliation enables and starts the units of an enabled
      component and leaves a disabled one alone.
- [x] The real controller is exercised against a throwaway user manager or,
      where that is not possible in tests, its argument vectors are checked
      exactly; it never runs a shell.
- [x] The units of `better-awake`, `better-monitor`, `better-storage`, and
      `better-touchpad` are found from the real dpkg file lists of the 0.2.9
      packages (`dpkg-deb -c` on the release assets).
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## What was built

Branch `ticket-79`, not merged. Every command below was run from this worktree.

### What is there

- **Which units a component has.**
  `crates/manager-platform/src/user_units.rs` reads
  `/usr/lib/systemd/user/*.service` out of the file list dpkg reports for the
  package (`PackageStateProbe::installed_files`). No unit names are kept by
  component ID, and the manifest schema is unchanged. A unit on the system bus,
  a D-Bus activation file, a drop-in directory, and the unit directory itself
  are not units. The published packages each install one user unit:
  `better-awake.service`, `better-monitor.service`, `better-storage.service`,
  and `better-touchpad-gestures.service`; `better-manager-daemon`'s unit
  belongs to the system manager and is deliberately ignored.

- **A user-session unit controller.** `UserUnitController` has `enable_now`,
  `disable_now`, `states`, and (added in review) `enable_and_restart`,
  implemented by `SystemdUserUnitController` and by `FakeUserUnitController`.
  The real one makes one `systemctl --user` call per unit — `enable --now`,
  `enable` then `restart`, `disable --now`, and `show` for `UnitFileState`,
  `ActiveState`, `Result`, `ExecMainCode`, and `ExecMainStatus` — as an
  argument vector with `LC_ALL=C`, never through a shell, and gives up on a
  call after 10 seconds. A name is checked first: plain `name.service`, no path,
  no whitespace, no leading `-`, and no template without an instance
  (`name@.service`). `enabled` accepts `enabled`, `enabled-runtime`, `linked`,
  `linked-runtime`, `static`, `indirect`, `generated`, and `transient`; `masked`
  is read separately. A unit systemd has never heard of reads as neither
  enabled nor running, because `show` exits 0 for it.

- **The driver does the work.** `RealDriver` gained `with_user_units` beside
  `new`. `new`'s signature is unchanged, and a driver built without a controller
  manages no units, which is what every existing construction gets. Removing a
  component disables its units in the Installing stage *before* the package is
  handed to the daemon. Applying settings enables and starts them for install,
  enable, and a restore that leaves the component enabled; update and such a
  restore restart them; disabling, and a restore to a disabled snapshot, stop
  and disable them; Verify on an enabled component starts what is stopped and
  then checks. A unit that fails fails the step with `user_units.failed` and
  `<unit>: <systemctl's first stderr line>` in the detail, so the component is
  not marked healthy.

- **Health includes unit state.** For every step that leaves its component
  enabled, the health stage requires each unit to be enabled and either
  running or finished on its own with success. `user_units.not_running` names
  the unit and says whether it is masked, disabled, still starting, failed, or
  stopped. A dpkg file list that cannot be read, or a state query that fails,
  is `health.undetermined` rather than a pass.

- **Startup heals existing machines.** `Manager::heal_user_units` in
  `crates/manager-core/src/reconcile.rs` enables and starts the units of every
  component recorded as installed and enabled, and only those. It is two
  halves: `run_user_unit_healing` talks to systemd and changes no state, and
  `record_user_unit_healing` writes the result into the state as it is when the
  answer lands. The window runs the first on a background thread from
  `reconcile_with_host`; the command line runs both after loading state in real
  mode. The state is saved when the pass changed it, and a pass with nothing to
  do changes nothing, not even the revision.
  A package dpkg will not describe is skipped (the health check reports that), a
  component with no units is skipped, and a unit that fails to start records a
  warning, leaves the component `Degraded`, and is not retried in a loop.

- **Wording.** Five evidence sentences in English and zh-TW:
  `user_units.failed`, `user_units.not_running`, `user_units.started`,
  `user_units.masked`, and `user_units.no_session`.

### Decisions worth knowing

- `PlanStep` gained `#[serde(default)] leaves_enabled`, decided where the step
  is planned: Install, Update, and Enable true; Disable and Remove false;
  Verify from the record; Restore from the snapshot. Without it the driver
  would re-derive intent from a record the step is about to change. The cost is
  that a plan persisted before this change reads as `false` and manages no
  units; a plan only means something for the run that made it, and no migration
  guesses.
- Unit health lives in the driver's CheckingHealth stage and not in
  `better-core`'s `verify_package`. That rule is dpkg's file list, shared with
  the daemon, and a user-session question does not belong in root's service.
- Startup healing is keyed on the record, not on a manifest flag, because the
  record is the person's decision. There is no per-component "runs by default".

### Not done

- The packages still install their units disabled. Enabling remains the
  manager's step; this ticket makes the manager take it.
- System units are out of scope; only user units are managed.
- No retry or backoff. A unit that fails at startup stays degraded until the
  next start or install.
- The command line heals on every real invocation, `list` and `status`
  included, because "Better Manager starts" is every command. Nothing asks
  first; the record already is the answer. What it says goes to standard error,
  and nothing it meets stops the command.
- No screen was added or rearranged. Activity rows still do not show a
  failure's detail; a `user_units.*` row names its unit in the sentence
  instead.
- A component left `Degraded` by a failed start goes back to `Healthy` only
  when a later pass starts the unit itself. If the unit came up some other way,
  the component stays `Degraded` until a health check.
- A unit whose start condition is not met (`Condition...=` in the unit file)
  reads as stopped, not as finished, so the pass would try to start it at every
  launch. None of the published units has a condition.

### Not verified

- No test enables, starts, stops, or disables a real user unit, by design. The
  real controller's invocations are asserted exactly, its watch after a start
  and its reading of systemctl's answers are tested through a scripted runner,
  and no shell is reachable.
  The owner's machine is the first real `systemctl --user enable --now` this
  path will make.
- The whole proof is against fakes: `FixedPackageStateProbe`,
  `FakeHealthProbe`, `FakeUserUnitController`, and a succeeding privileged
  executor. No real install, update, restore, disable, or remove ran on this
  host; packaging changes need a disposable environment per `AGENTS.md`.
- The window's startup heal and its driver wiring are proven by
  `manager-core`'s tests, the command line's own unit test, and source-scan
  tests in `manager-gui`. No test constructs `ManagerApp`.
- Whether GNOME's session shows Better Awake's tray once the service is
  started was not observed. Nothing here ran on a Zorin 18 desktop.
- `systemctl --user show` for a unit that was never loaded
  (`LoadState=not-found`, empty `UnitFileState`, exit 0), for one that was
  never started (`Result=success`, `ExecMainCode=0`), and for one that exited 0
  (`ExecMainCode=1`, `ExecMainStatus=0`), and the bus error
  (`Failed to connect to bus: No medium found`, exit 1), were read on this host
  only, under systemd 255. Nothing has watched a real unit crash as it starts.
- The window's background startup pass is checked by reading `app.rs`, not by
  running a window. A transaction started in the seconds while it runs is not
  blocked: the pass's result is dropped when it lands on an active
  transaction, but a unit it is starting at that moment is still started.

### Independent review, and what it changed

An independent review of the first version found twelve problems, and all
twelve are fixed. Each fix has a test that fails without it. Where the fix
needed a new seam first, that was shown by putting the old behaviour back one
item at a time and running the test against it.

- **"Check again" could never succeed.** Verify only looked, so an enabled
  unit that was stopped failed every check and nothing on screen could start
  it. Verify on an enabled component now starts what is stopped, then checks.
- **Update and restore did not restart.** `enable --now` on a running unit
  starts nothing, so the old program kept running after an update. They now
  enable and `restart`, which also starts a unit that was not running.
- **A restore to a disabled snapshot left the units running.** It now stops
  and disables them, the mirror of a restore that leaves the component enabled.
- **A removal the daemon refused left the units stopped** under a record that
  still said enabled. The units that were enabled before are started again.
- **A service that exits 0 on purpose counted as broken.** The touchpad
  gesture service ends successfully when it has nothing to do, and its unit is
  `Restart=on-failure`, so it was "repaired" at every launch and failed every
  check. Inactive with `Result=success`, `ExecMainCode=1`, and
  `ExecMainStatus=0` now means the unit finished, which is fine.
  `Result=success` alone is not enough, because a unit that never started
  reports it too.
- **A crash right after starting was missed.** After each start the controller
  looks at the unit every 200 ms for up to two seconds. `activating` means
  wait, `failed` or stopped is a failure, and still `activating` at the end is
  a failure that says so.
- **The window started units on its drawing thread, with no time limit.** The
  pass now runs on a background thread, and its result is written into the
  state the window has when it lands. Every `systemctl` call is abandoned after
  10 seconds. Activity rows name the unit.
- **No user session was treated as a broken component.** An unreachable
  session bus or a missing `systemctl` is now its own kind of failure. The
  startup pass skips and changes nothing. Disable and remove fail with
  `user_units.no_session` and say no user session is available, and a removal
  stops before the package is handed over. In the command line a failed startup
  pass is a warning and never stops the command. A start that succeeds returns
  a component the pass had degraded, with no other failure recorded, to
  healthy.
- **The startup pass ran during a transaction.** It now does nothing while one
  is in progress.
- **A unit someone disabled by hand is still enabled again**, because the
  record says enabled; the activity entry now says it was done because of
  Better Manager's record. A masked unit is never started and is reported
  once, not at every launch.
- **`foo@.service` was accepted** as a unit name. A template with no instance
  is now refused.
- **Nothing stopped a test from reaching the real user manager.** Setting
  `BETTER_OS_TEST_SYSTEMCTL_GUARD` to a file makes the real controller write
  each call there and run nothing. The command-line tests that run the real
  binary in its real mode set it, and assert that nothing was written.

## Gates

- `cargo fmt --all -- --check` — clean.
- `RUST_FONTCONFIG_DLOPEN=1 cargo +1.99.0 clippy --workspace --all-targets
  --all-features --locked -- -D warnings` — clean.
- `RUST_FONTCONFIG_DLOPEN=1 cargo +1.95 check --workspace --all-targets
  --locked` — clean.
- `RUST_FONTCONFIG_DLOPEN=1 cargo test --workspace --locked --no-fail-fast` —
  exit 0 after the review: 3263 passed, 0 failed, 6 ignored. The six need the
  network, hardware, or a live session and were already ignored.
- `bash packaging/generate-third-party-notices.sh --check` — current. No
  dependency changed, so `Cargo.lock` and `docs/third-party-licenses.md` are
  untouched.

