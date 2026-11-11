# Ticket 69 — Better Manager says when a package's version cannot be compared

**Epic:** Better Manager (Issue #8) · **User Story:** a person whose component
was installed with a version the manager cannot order sees that it exists and
why it is not managed · **Branch:** `ticket-68-69` · **Blocked by:** none ·
**Status:** implemented, not merged

## What it delivers

A dpkg version that is not a semantic version keeps a package invisible:
reconciliation adopts only what `semver` can parse, and nothing reports the
rest. The owner decided on 2026-09-27 that such a package is shown as a doctor
finding: seen, explained, and not managed.

## Acceptance criteria

- [x] `better-manager-cli doctor` and the GUI's equivalent list each Better OS
      package dpkg holds with a version `semver` cannot parse, with the version
      as dpkg reports it and what to do about it.
- [x] Nothing is adopted, planned, or changed for such a package.
- [x] A package whose record exists and whose host version becomes unorderable
      keeps its existing blocking drift finding; this ticket adds only the
      unrecorded case.
- [x] Tests cover an epoch (`1:0.2.8`), a Debian revision (`0.2.8-1ubuntu1`),
      and a tilde version.

## What was built

### Which versions are unorderable

The ticket names three cases, and only one of them is unorderable today.
Before comparing anything, the manager strips a dpkg version's epoch and Debian
revision (`manager_platform::dpkg::upstream_version`). So `1:0.2.8` and
`0.2.8-1ubuntu1` both become `0.2.8`, which `semver` parses, and reconciliation
adopts them as ticket 44 already does. A tilde stays in the upstream part:
`0.2.8~rc1` is not a semantic version, and neither is `0.2` or
`1:0.2.8~rc1-1ubuntu1`. Those are the packages this ticket shows. The tests
cover all five and assert which group each one is in, rather than calling the
epoch and revision cases unorderable.

### The finding

`HostComparison` in `crates/manager-core/src/lib.rs` has a new case,
`Unorderable`, for "nothing recorded, and dpkg holds a version `semver` cannot
parse". Reconciliation treats it exactly as it treated that case before, under
`Absent`: nothing is adopted, no record is created, and no drift finding is
raised. `Manager::unorderable_host_checks(state, probe)` walks the same
comparison and returns one doctor check per such package:
`DoctorCheckKind::UnorderableHostVersion`, status `Warning`, the component, and
the new `DoctorCheck::host_version` field holding the version exactly as dpkg
reports it, epoch and revision included. It reads the state and dpkg and
changes neither. A package that is recorded is not listed, because its
unorderable host version is already a blocking `VersionMismatch` drift finding,
and that is unchanged.

### Where it shows

`better-manager-cli doctor` adds these checks in real execution mode and prints
two more lines under each one: the version dpkg reports and that the component
is therefore not managed, then the way out, removing it with
`sudo apt remove <component>` and installing it from Better Manager. Under
`--execution mock` dpkg is not asked.

The window asks once, at the end of `reconcile_with_host`, and keeps the result.
The Health screen and the doctor results list it with the other checks, labelled
"Installed at a version Better Manager cannot compare" (安裝的版本無法比較),
with the same sentence under the component name. A demo window skips
reconciliation and never asks.

### Tests

Before the code existed, the new tests failed to compile (`manager-core`: 6
errors, `manager-cli`: 2, `manager-gui`: 3).

- `manager-core`, `tests/lifecycle.rs`, module `unorderable_host_versions`:
  the five versions above, each through `reconcile` and
  `unorderable_host_checks`, asserting the adopted version or the finding with
  dpkg's version; the state after both calls equals the state before for an unorderable
  package; a recorded package keeps its drift finding and is not listed again;
  an absent or orderable package produces no finding.
- `manager-cli`: the doctor lines for the finding carry the version, "not
  managed", and the `apt remove` line, and every other check keeps its single
  line.
- `manager-gui`: the sentence names dpkg's version and the way out in both
  locales and is absent for other checks; dpkg is asked in
  `reconcile_with_host` and not in `doctor_checks`, which runs on every frame;
  the doctor row draws the label and the sentence.

### Review fix: nothing is planned

The first version only reported the package. The planner reads the record and
not the host, so `Manager::plan(..., Install)` still produced a plan for it, and
the window offered Install. `Manager::plan_on_host` now plans as `plan_in_mode`
does and then checks every step against dpkg: a component the record does not
carry and dpkg holds at an unorderable version is refused with the existing
`ManagerError::HostVersionUnreadable`. `better-manager-cli run` and `restore`
plan through it in real execution, and the window's one planning path for a
component operation, `ManagerApp::plan_operation`, does too. A demo plans from
its own state as before.

The window offers no action the planner would refuse: a component card's main
button opens the component page instead of installing, and the page shows the
finding's sentence and drops the Install and Try again buttons. Tests:
`nothing_is_adopted_planned_or_changed_for_an_unorderable_package` now plans
Install and Update and asserts both are refused and the state unchanged;
`planning_on_the_host_plans_what_dpkg_does_not_block`;
`a_real_run_refuses_to_plan_a_package_dpkg_holds_at_an_unorderable_version` in
`manager-cli`; `the_window_neither_plans_nor_offers_an_unorderable_package` in
`manager-gui`. Each failed before the fix, to compile in `manager-core` and
`manager-cli` and on the missing planning path in `manager-gui`.

### Not done, and why

- `better-manager-cli plan` is a dry run in simulation mode, as before, so it
  still prints a plan for such a package. `run` refuses it.
- Stripping the epoch means the manager orders `1:0.2.8` below a catalog
  `0.3.0`, while dpkg orders it above. That was ticket 44's choice and it is
  unchanged here; an update planned from such a version would reach apt as a
  downgrade.
- The finding has not been seen on screen against a real package.
