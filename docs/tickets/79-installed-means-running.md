# Ticket 79 — An installed component runs, with nothing for the person to type

**Blocked by:** none · **Status:** implemented, not merged

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
  `disable_now`, and `states`, implemented by `SystemdUserUnitController` and by
  `FakeUserUnitController`. The real one makes one `systemctl --user` call per
  unit — `enable --now`, `disable --now`, `show --property=UnitFileState
  --property=ActiveState` — as an argument vector with `LC_ALL=C`, never through
  a shell. A name is checked first: plain `name.service`, no path, no
  whitespace, no leading `-`. `enabled` accepts `enabled`, `enabled-runtime`,
  `linked`, `linked-runtime`, `static`, `indirect`, `generated`, and
  `transient`; `active` accepts `active` and `reloading`. A unit systemd has
  never heard of reads as neither, because `show` exits 0 for it.

- **The driver does the work.** `RealDriver` gained `with_user_units` beside
  `new`. `new`'s signature is unchanged, and a driver built without a controller
  manages no units, which is what every existing construction gets. Removing a
  component disables its units in the Installing stage *before* the package is
  handed to the daemon. Applying settings enables and starts them for install,
  update, enable, and a restore that leaves the component enabled; disabling
  stops and disables them; Verify only looks. A unit that fails fails the step
  with `user_units.failed` and `<unit>: <systemctl's first stderr line>` in the
  detail, so the component is not marked healthy.

- **Health includes unit state.** For every step that leaves its component
  enabled, the health stage requires each unit to be enabled and active.
  `user_units.not_running` names the unit and says whether it is disabled or
  stopped. A dpkg file list that cannot be read, or a state query that fails,
  is `health.undetermined` rather than a pass. Verify starts nothing.

- **Startup heals existing machines.** `Manager::heal_user_units` in
  `crates/manager-core/src/reconcile.rs` enables and starts the units of every
  component recorded as installed and enabled, and only those. The window calls
  it from `reconcile_with_host`; the command line calls it after loading state
  in real mode. Both record an activity entry, the state is saved when the pass
  acted, and a pass with nothing to do changes nothing, not even the revision.
  A package dpkg will not describe is skipped (the health check reports that), a
  component with no units is skipped, and a unit that fails to start records a
  warning, leaves the component `Degraded`, and is not retried in a loop.

- **Wording.** Three new evidence sentences in English and zh-TW:
  `user_units.failed`, `user_units.not_running`, `user_units.started`.

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
  first; the record already is the answer.
- No screen was added or rearranged. Activity rows do not show the evidence
  detail, so a unit name in a `user_units.*` activity row is not on screen;
  failure cards do show the detail.

### Not verified

- No test enables, starts, stops, or disables a real user unit, by design. The
  real controller's invocations are asserted exactly and no shell is reachable.
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
- `systemctl --user show` behaviour for a unit that was never loaded
  (`LoadState=not-found`, empty `UnitFileState`, exit 0) was read on this host
  only.

## Gates

- `cargo fmt --all -- --check` — clean.
- `RUST_FONTCONFIG_DLOPEN=1 cargo +1.99.0 clippy --workspace --all-targets
  --all-features --locked -- -D warnings` — clean.
- `RUST_FONTCONFIG_DLOPEN=1 cargo +1.95 check --workspace --all-targets
  --locked` — clean.
- `RUST_FONTCONFIG_DLOPEN=1 cargo test --workspace --locked --no-fail-fast` —
  exit 0: 203 test binaries, 3235 passed, 0 failed, 0 ignored.
- `bash packaging/generate-third-party-notices.sh --check` — current. No
  dependency changed, so `Cargo.lock` and `docs/third-party-licenses.md` are
  untouched.

