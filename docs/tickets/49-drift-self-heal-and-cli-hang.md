# Ticket 49 — Drift self-heal, an action for the rest, and the CLI panic that hung

**Branch:** `ticket-49` · **Blockers:** none · **Status:** implemented, not merged

Three defects from one Zorin 18 machine running `v0.2.5`. They arrived together
and are unrelated in cause, so they are written down separately.

## What the machine reported

The state file held, for `better-manager`:

```
installed_version 0.2.3
health            failed
failure           the old plan_rejected
provenance        manager
drift             version_mismatch, host 0.2.5
```

dpkg held 0.2.5. The catalog held 0.2.6. The Updates screen offered **nothing**
for the manager.

Separately, `/usr/bin/better-manager catalog status` printed zbus's *"there is
no reactor running"* from a worker thread and then did not come back inside two
minutes.

---

## 1. Reconciliation self-heals the safe direction

### The defect

The user upgraded through `install.sh`, which is the supported way — and the
only way, because the manager cannot replace its own running binary from the
inside. Reconciliation called the result drift. Drift blocks planning
(`Manager::ensure_no_drift`). So the one component that had an update available
was the one component nothing could be planned for, and the screen that would
have said so said nothing at all. **The manager punished the user for upgrading
it the only way it could be upgraded.**

### The rule now

Every reconciliation decision is made in one closed set of cases,
`HostComparison` in `crates/manager-core/src/lib.rs`. The record's version and
dpkg's answer go in; one of five cases comes out.

| Record | dpkg says | Case | What happens |
| --- | --- | --- | --- |
| nothing | nothing | `Absent` | nothing |
| nothing | an orderable version | `Adoptable` | adopted (ticket 44's behaviour, unchanged) |
| nothing | a version `semver` cannot parse | `Absent` | left alone — a version nothing can order must not be recorded |
| a version | the same version | `Agreed` | nothing |
| a version | **a higher** version | `HostAhead` | **self-heals** |
| a version | a lower version | `Drifted` | blocking finding |
| a version | nothing | `Drifted(MissingOnHost)` | blocking finding |
| a version | unorderable on either side | `Drifted` | blocking finding |

Only `HostAhead` is new. It is not drift because nothing about it is
unexplained: `apt` and `install.sh` are supported, and a package moving
*forward* under dpkg's own ownership is the machine having been upgraded.
Everything else stays blocking, because a package that went backwards, or
vanished, or carries a version nothing can order, means something happened that
the manager cannot account for — and guessing there would replace one unverified
belief with another.

### What a self-heal writes

`adopt_host_version` is one function, used by the self-healing path and by the
requested one, so the two cannot come to mean different things by "adopt":

- `installed_version` ← what dpkg says, upstream part only (an epoch is dropped)
- `provenance` ← `Dpkg`
- `failure`, `recovery` ← cleared. They describe a version that is no longer
  installed. This is the field record's stale `plan_rejected`.
- `health` ← `Healthy`, which is what a fresh adoption gets
- `installed_artifact`, `restore_snapshot` ← cleared. They name a version this
  machine no longer has, and a restore offered from them would fail at the point
  of use.
- `drift` ← `None`, so planning works again
- `enabled` ← **kept**. Whether a component should run is the user's decision and
  dpkg has no opinion about it.
- an activity entry, `host.externally_upgraded:0.2.3->0.2.5`

### The matrix, in tests

`crates/manager-core/tests/lifecycle.rs`, module `host_self_heal`. One table
test walks host version × provenance × health — four host answers plus an epoch
case, against `Manager` and `Dpkg` provenance, against a clean record and one
carrying a real failed install — and asserts for every cell either the adopted
version or an unchanged record plus a finding plus `ManagerError::HostDrift`
from `plan`.

Two existing tests asserted the old behaviour for a host that is *ahead* and
were rewritten to the direction that is still blocking, rather than deleted:
`a_version_the_host_disagrees_about_blocks_planning_until_it_is_resolved` and
`adoption_never_overwrites_a_recorded_version_with_an_older_one`.

---

## 2. A drifted component gets an action

### The defect

Drift was **invisible**. `crates/manager-gui` never mentioned it anywhere: not
on the component page, not on Updates, not in the sidebar. The planner refused
every operation because of it and no screen said why.

### What is there now

`ComponentInfo::drift` carries the record's `DriftKind`, and
`ComponentInfo::drift_notice` turns it into three sentences in the order a
person needs them — what is wrong, which two things disagree, what it costs.
The card is built in the view model, not in the render function, because
deciding that a component cannot be planned for is a decision.

`ManagerApp::drift_card` draws it above the failure card, since a blocking drift
is the reason everything else on the page refuses. It follows ticket 48's rule:
the heading and the three sentences are status, and the single filled control is
the only thing on the card that does anything.

**採用系統版本 / Adopt system state** asks first. Adoption throws away the
recorded restore point, so the first click sets `ManagerApp::adopt_confirm` and
the card grows a sentence saying what will be lost plus a 確認採用 / Adopt it
button. Only that second button calls the action, and a source-level test
asserts the ordering so the confirmation cannot be removed by accident.

The action itself is `Manager::adopt_host_state` in `manager-core`, not
state-file surgery in the window:

```rust
pub fn adopt_host_state(
    &self,
    state: &mut ManagerState,
    id: &ComponentId,
    probe: &dyn PackageStateProbe,
) -> Result<HostAdoption, ManagerError>
```

It refuses a component the catalog does not carry, refuses while a transaction
is running (`ManagerError::ActiveOperation`), refuses a version `semver` cannot
order (`ManagerError::HostVersionUnreadable`, new), is idempotent — a record that
already agrees is not rewritten, the revision does not move and nothing is
logged — and writes `host.state_adopted:0.2.3->0.2.1` when it does act. A host
with no such package empties the record rather than inventing a version.

### The command line

```
better-manager-cli reconcile --adopt <id>
```

`reconcile` with no flag still only reports, and now prints the `--adopt` line
under each finding, so the report is not a dead end. The flag design is a flag
rather than a subcommand because it is a mode of reconciliation and not a
different question: the same probe, the same rules, one component instead of
all of them.

---

## 3. The panic, and the hang

These are two causes with one symptom. Both were reproduced on the host with
current code before either was touched.

### The hang: `/usr/bin/better-manager` is the window

`packaging/build-deb.sh` installed exactly one binary in the `better-manager`
package — `manager-gui`. **The command line was never packaged at all.** The
window ignored every argument and opened a window, so
`better-manager catalog status` ran the manager's window at a terminal, and a
window does not exit. That is the whole hang. There was no deadlock.

Fixed both ways round:

- The package now installs `manager-cli` as `/usr/bin/better-manager-cli`,
  following `better-monitor`, which already ships its window as
  `/usr/bin/better-monitor` and its command line as `/usr/bin/better-monitor-cli`.
  `/usr/bin/better-manager` stays the window, because that is what the desktop
  entry, the component manifest, and every published package already name.
- The window refuses an argument it does not understand, exits 2, and names the
  program that would have answered it. `--version` and `--help` are answered
  where they are asked, because those are the two things a person types at a
  program's name expecting an answer rather than a window.

### The panic: zbus's tokio flavor, unified into every window

Not our own D-Bus client, and not a missing guard. The backtrace names the two
callers:

```
tokio::runtime::handle::Handle::current
tokio::runtime::blocking::pool::spawn_blocking<zbus::address::transport::Transport::connect>
zbus::connection::builder::Builder::build_
gpui_linux::linux::xdg_desktop_portal::XDPEventSource::new     ← thread "Worker-6"
```

```
zbus::connection::builder::Builder::build
accesskit_unix::context::get_or_init_messages                  ← its own thread
```

zbus picks its I/O backend from a **compile-time cargo feature**, and cargo
unifies features across every package one `cargo build` names.
`packaging/build-deb.sh` builds the services and the windows in a single
invocation, and `manager-daemon`, `awake-service`, `monitor-service`,
`touchpad-gesture-service`, `launcher-platform` and `touchpad-platform` all asked
for `zbus/tokio`. So every Better OS **window** was compiled with a zbus that
spawns its internal tasks onto tokio — and gpui's XDG desktop portal and
accesskit both open their own zbus connections from plain threads that no tokio
runtime owns.

Ticket 43's `runtime().enter()` guard in `privileged.rs` could only ever cover
this project's own calls. gpui's thread and accesskit's are not reachable from
here, so the class could not be fixed by extending the guard.

**The fix is to stop asking for the feature.** zbus's default `async-io` flavor
owns its own reactor and can be called from any thread; zbus's own README says
the `tokio` feature is tight integration and not a requirement, and every
service here blocks on its futures from a runtime it owns rather than depending
on zbus spawning into it. Ten manifests changed, `zbus_polkit` with them. The
guard and `manager-platform`'s `tokio` dependency are gone, since the flavor
that needed them is gone.

`crates/manager-platform/src/flavor.rs` keeps it that way: it scans every
`Cargo.toml` in the workspace for a `zbus` or `zbus_polkit` line naming the
`tokio` feature and fails with the offending file and line. The check is on the
manifests because that is where the mistake gets made, and a check that only
failed once the packaging build ran would find it after the release.

### What the change cost on the service side

The tokio flavor was doing one thing the services relied on without saying so:
it made zbus poll a served method body as a **tokio task**, so a handler could
call `tokio::spawn` and `tokio::task::spawn_blocking` and find a runtime. On the
async-io flavor zbus polls those bodies on its own executor, which is not a
tokio thread, and both of those panic looking for one — the panic is swallowed
inside the task, the method never replies, and the *caller* hangs. Four of
`manager-daemon`'s six D-Bus tests hung on exactly this; the two that only read
a property passed, which is how it was found.

The fix is the guard the other side could not have: the runtime here is the
service's own, so it can be named. `ManagerService` and `StorageCoordinator`
capture a `tokio::runtime::Handle` at construction and spawn through it —
`Handle::spawn` and `Handle::spawn_blocking` need no ambient context. The
storage coordinator's handle is an `Option`, because unit tests build one
outside any runtime; without one it runs the same work inline.

`monitor-service`, `awake-service` and `touchpad-gesture-service` needed
nothing: their `tokio::spawn` calls are at startup, inside their own
`#[tokio::main]`, and their handlers await nothing that needs a runtime.

### What this also fixed

Every gpui window in the workspace had a broken XDG desktop portal and broken
accessibility on any machine, not only when a command line was typed at it.
`better-monitor`, `better-files`, `better-launcher`, `better-touchpad`,
`better-awake` and the standalone app chooser were all built in that same cargo
invocation. Nobody had reported it because the panic scrolls past on a desktop
launch and the window still opens.

---

## Gates

Recorded in `delivery-status.md`. The two commands the report named were run
from the built binary on this host under a timeout, and the window was run on
the real Wayland session before and after the zbus change — the panic is in the
before log and the after log is empty.

## Limits

- The zbus flavor change is workspace-wide. Every service now runs zbus on
  `async-io`, which costs one reactor thread per process that it did not cost
  before. The workspace test suite is what proves the services still speak;
  none of the five session services was watched serving on a real bus for this
  ticket beyond what their own tests do — `manager-daemon`'s six-test D-Bus
  suite over a private bus is the closest thing to that proof, and it passes.
- Nothing stops a future handler from calling `tokio::spawn` directly and
  hanging its caller the same way. The two crates that did were fixed and the
  reason is written beside the field they now use, but there is no test that
  fails on a new bare spawn — only the D-Bus suite of whichever service gains
  one.
- `better-monitor`, `better-files`, `better-launcher`, `better-touchpad` and
  `better-awake` also ignore command-line arguments and open a window. Only the
  manager was fixed, because only the manager has a shipped command line to
  point at. The others are named here rather than left for the next report.
- A drifted component is only visible on its own page. The Components list shows
  its status but not the reason, and the Updates screen still simply omits it.
