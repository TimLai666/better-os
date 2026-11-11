# Ticket 75 — 重新檢查 checks the component instead of failing

**Epic:** Better Manager (Issue #8) · **User Story:** a person whose component
is marked as failed can press 重新檢查 and get a real answer · **Branch:**
`ticket-75` · **Blocked by:** none · **Status:** implemented, not merged

## What the machine reported

A Zorin 18 machine running Better Manager 0.2.7 had Better Awake 0.2.7
installed and healthy under dpkg, and recorded as failed: an Update All that
included it was cut off when the machine shut down. Every press of 重新檢查
failed at the install stage with `daemon.plan_rejected` / `ipc.error.empty_plan`,
and the window explained it as "the system service refused the plan as not
matching this machine".

## The defects

1. **A plan with no privileged step always fails.** Verify, Enable, and Disable
   do not cross the privileged boundary, so their wire plan has no steps.
   `RealDriver::apply` meant to complete such a plan without the daemon, but it
   calls `wire_plan`, which validates first, and `WirePlan::validate` refuses an
   empty plan. The completion branch after it is unreachable. In real mode,
   重新檢查, 啟用, and 停用 can never succeed.
2. **重新檢查 checks nothing.** With no daemon outcome, the real driver reports
   the health stage as completed, and `finish` marks the component healthy
   under the evidence key `mock_health_check_passed`. Fixing defect 1 alone
   would turn a failure into an unverified pass.
3. **The refusal is described wrongly.** Every `daemon.plan_rejected` is shown
   as a machine mismatch, whatever the detail says.

## The rule a verify uses

The daemon's health check is the only rule for "installed and healthy": dpkg
reports the package installed, and every path `dpkg-query -L` lists exists,
except directories, conffiles, and paths dpkg's configuration excludes. A
verify must use the same rule, read unprivileged. It moves out of
`manager-daemon` into `better-core::package_health`, which both the daemon and
`manager-platform` call. Nothing about the rule changes.

## Acceptance criteria

- [ ] A plan whose steps are all Verify, Enable, or Disable never reaches the
      executor and never fails wire validation.
- [ ] A plan that mixes privileged and unprivileged steps still sends only the
      privileged ones.
- [ ] A Verify step runs the shared health rule against the real dpkg database
      through `manager-platform`: healthy → the component is healthy and its
      failure cleared; a missing file → the stage fails naming the file; a
      query that cannot run → the stage fails as undetermined. Nothing is
      reported healthy without that check.
- [ ] The daemon's health check gives the same answers it gave before the move,
      and its existing tests pass unchanged apart from import paths.
- [ ] The evidence key for a passed verify no longer starts with `mock_`, and the
      window still shows it as passed.
- [ ] `daemon.plan_rejected` is described as a machine mismatch only when its
      detail is a release or architecture mismatch; any other detail gets a
      generic refusal with the detail shown.
- [ ] Regression tests reproduce the field report: a verify-only plan through
      the real driver with a fake executor and a fake package probe.

## What was built

- `DpkgProbe::installed_files` reads `dpkg-query -L` and the conffile list,
  and marks the paths that dpkg's own configuration or apt's `DPkg::Options`
  exclude from unpacking. It runs no shell, sets `LC_ALL=C`, and refuses an
  unsafe package name before any command runs.
- `verify_package` runs `better_core::package_health::check_installed` against
  that probe. The daemon's rule is unchanged; the move was already done.
- `RealDriver::wire_plan` returns `Ok(None)` for a plan with no privileged step,
  so a verify, enable, or disable plan never reaches the executor or the
  validator. A mixed plan still sends only its privileged steps.
- `RealDriver::check_health` runs `verify_package` for each Verify step. A
  healthy package passes, a failed check stops the stage with `health.failed`
  and the missing file named, and a check that cannot run stops it with
  `health.undetermined`. A daemon outcome that is not Completed still wins.
- `finish` records a passed verify as `health_check_passed`. The window reads
  that key and still reads the old `mock_health_check_passed`, because state
  saved before this change still carries it.
- `plan_rejection_is_host_mismatch` calls a `daemon.error.plan_rejected` a
  machine mismatch only when its detail starts with a release or architecture
  mismatch. Any other refusal falls through to the generic daemon label.

Why: the field report showed a healthy package recorded as failed, because a
verify-only plan was sent to the daemon as an empty plan and was refused. The
health stage also reported success without checking anything.

Not verified:

- No Better Awake install on a real host was checked through the window or the
  CLI. The dpkg queries were exercised only against the real `dpkg` package,
  which the verify tests use.
- The `apt-config` path filter was not tested against a host that sets a real
  `path-exclude`.
- Nobody has looked at a failed or an undetermined verify in the window; its
  two labels are covered by the copy tests only.
- Nobody has pressed 重新檢查 on the machine that reported the defect.

## What the review changed

An independent review of the branch found two defects in it, both fixed here.

- **A real machine mismatch was no longer called one.** The daemon's refusal is
  stored as a key and a detail, and the window passed the label only the key,
  so the check for "plan targets release" never matched. The check now takes
  the key and the detail the way a failure record holds them, and a test builds
  the record rather than a hand-joined string.
- **Disable then enable cleared a failure unchecked.** Finishing a transaction
  marks its components healthy and clears their failure. Once Enable and
  Disable stopped failing on the empty plan, they finished without a check, so a
  failed component could be made healthy by toggling it. The health stage now
  runs the shared rule for every step the daemon never saw.

The review also found the file-list reading written twice, in the daemon's
driver and in `manager-platform`. Both now call
`package_health::installed_files_from` and `path_filter_from`, and each keeps
only the running of the commands.
