# Ticket 75 — 重新檢查 checks the component instead of failing

**Epic:** Better Manager (Issue #8) · **User Story:** a person whose component
is marked as failed can press 重新檢查 and get a real answer · **Branch:**
`ticket-75` · **Blocked by:** none · **Status:** ready

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
