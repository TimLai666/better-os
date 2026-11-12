# Ticket 68 — Better Manager can repair a package transaction a crash interrupted

**Epic:** Better Manager (Issue #8) · **User Story:** a person whose machine lost
power in the middle of an install can finish it from Better Manager instead of
a terminal · **Branch:** `ticket-68-69` · **Blocked by:** none · **Status:** released in v0.2.9

## What it delivers

A transaction interrupted by a crash or power loss leaves dpkg with half-
configured packages, and every later apt operation refuses until someone runs
`dpkg --configure -a` as root. The project owner decided on 2026-09-27 that the
privileged daemon offers that repair. It is a new privileged operation, so it
changes the protocol ADR 0007 decided and needs ADR 0014 first.

## Acceptance criteria

- [x] ADR 0014 records the operation: what it runs (`dpkg --configure -a` and
      nothing that takes an argument from the client), the polkit action and
      its default authorization, how it is detected that a repair is needed,
      and what the daemon refuses.
- [x] The daemon detects an interrupted state from dpkg's own record rather
      than from an error string, and reports it to clients.
- [x] The repair is a method with no free-text argument, authorized by its own
      polkit action, serialized with every other transaction, and its outcome
      and log tail are reported like an install's.
- [x] `better-manager-cli repair` and a control on the screen that shows the
      interrupted state both reach it through `manager-core`; neither runs
      anything privileged itself.
- [x] Tests use the daemon's fake APT/dpkg driver; the new polkit action ships
      in the daemon package and `verify-deb.sh` checks it.

## What was built

### The decision

[ADR 0014](../decisions/0014-dpkg-interrupted-repair.md) records it. The daemon
runs exactly `dpkg --force-confdef --force-confold --configure -a`, with the
same cleared environment as the APT driver. The two `--force-conf*` flags are
the ones every install already passes through apt, so a changed configuration
file cannot stop the run at a prompt nobody can answer. The method takes no
argument.

The new polkit action is `org.betteros.manager.repair-package-system`, with
`auth_admin` for active, inactive, and any sessions. That is stricter than
`apply-transaction`, which keeps an active session's authorization for a while,
because a repair configures every package dpkg left unfinished, not only
`better-*` ones.

### How the daemon knows

`crates/manager-daemon/src/dpkg_state.rs` reads dpkg's own record. A package
whose `Status` field ends in `half-installed`, `unpacked`, `half-configured`,
`triggers-awaited`, or `triggers-pending` was left mid-operation. A file with an
all-digit name in `/var/lib/dpkg/updates/` is a journal entry dpkg has not
folded back, which is the test apt makes before it refuses to run. Either one
means a repair is needed. No error string is read.

The result is a new wire document, `PackageSystemState` in `manager-ipc`: the
journal flag, up to 64 unfinished packages with their status, and a count of
the rest. `RepairOutcome` carries the verdict (`repaired`, `still_interrupted`,
or `failed` with a key), the log entry of the command with its exit code and
output tails, and dpkg's state as read after the run. Both follow the plan's
rules: unknown fields refused, closed enums, a size limit before parsing,
package names checked against dpkg's syntax. The protocol version stays 1,
because no existing document changed.

### The D-Bus surface

Two methods join `org.betteros.Manager1`:

- `GetPackageSystemState`, ungated like `GetStatus`.
- `RepairPackageSystem`, authorized by the new action only. It shares the one
  running flag with `ApplyTransaction`, so a repair never overlaps a
  transaction or another repair. It runs nothing and returns an error when
  dpkg's record shows nothing unfinished (`daemon.error.nothing_to_repair`) or
  cannot be read (`daemon.error.host_unreadable`). Once dpkg has run, the result
  is an outcome, not an error: a held dpkg lock is `daemon.error.apt_busy`, any
  other non-zero exit is `daemon.error.repair_failed`, and a clean exit that
  leaves a half-installed package is `still_interrupted`.

`ApplyTransaction` now reads the same record before it touches anything and
refuses an interrupted host with `daemon.error.dpkg_interrupted`, before any
rollback record is written. Before this, apt failed, the daemon tried to roll
back, and the rollback failed for the same reason.

The policy file in `packaging/daemon/` declares the action, and
`packaging/verify-deb.sh` fails a daemon package whose policy does not. A unit
test in `authorize.rs` reads the same policy file and checks that every action
the daemon asks polkit for is declared and that the repair action has no
`auth_admin_keep`.

### Clients

`manager-core` has a new `repair` module. `package_system_state` asks the
service, `request_repair` asks for the repair and words the answer as a
`RepairResult`, and `record_repair` writes the result into the activity log.
No component record changes; the next reconciliation reads dpkg as it always
does. The platform side is a new trait, `PackageSystemRepair`, implemented by
the D-Bus client and refused by `UnapprovedPrivilegedExecutor`.

`better-manager-cli repair` prints dpkg's state and, when something is
unfinished, asks for the repair and prints the verdict, the command, the last
20 lines of each output stream, and anything still unfinished.
`repair --check` only reports. `--execution mock` refuses the command with
`manager.error.repair_needs_real_execution`, because a simulated repair would
report a change to a machine nothing changed.

The window asks the service each time the Health screen or the doctor results
are opened, and after a transaction fails. It never asks at launch, so opening
the window does not start a root process, and a demo window never asks. When
dpkg is interrupted, the Health screen draws a card above the checks: each
unfinished package in dpkg's words, what it blocks, that the repair asks for
an administrator's password, and one Repair button. The button is disabled while
a repair or a transaction runs. The request runs off the UI thread, because
the password prompt holds it. The result is recorded against the state the
window has when it returns, and the window then reconciles with dpkg again. A
refused transaction's failure card now says the package system was interrupted
and points at the Health screen.

### Tests

Before the code existed, every new test failed to compile on the missing types
and methods (`manager-ipc`: 7 errors, `manager-daemon`: 28, `manager-platform`:
2, `manager-core`: 1, `manager-cli`: 4, `manager-gui`: 11).

- `manager-ipc`: round trips for both new documents, and refusals for an
  oversized document, an unknown field, a status dpkg never leaves mid-run,
  another protocol version, a bad package name, too many packages, a bad name
  inside a repair's remaining state, and a plan step with the action `repair`.
- `manager-daemon`, `dpkg_state.rs`: every unfinished status is read and
  packages at rest are not, multi-line fields are skipped, the cap and count
  work, a journal entry counts and dpkg's `tmp.i` does not, a missing
  `updates/` is no journal, and a missing status file is an error.
- `manager-daemon`, `executor.rs`, against `FakeAptDriver`: a transaction is
  refused before APT runs and with no restore point while dpkg is interrupted;
  a repair runs the fixed argv and reports `repaired`; it runs nothing on a
  clean or unreadable record; it reports `still_interrupted` with what remains;
  a failed run keeps its log; a held lock is `apt_busy`.
- `manager-daemon`, `service.rs`: a repair is refused as busy while the
  transaction flag is held, and runs once it is released.
- `manager-daemon`, private session bus: the state is readable by a caller
  polkit would refuse; a caller granted only `apply-transaction` cannot repair;
  a caller granted the repair action gets the outcome and log; a clean host is
  refused with nothing run; `ApplyTransaction` on an interrupted host returns a
  failed outcome under the client's id with no step index and no recovery.
- `manager-platform`: the unapproved executor refuses both new calls; an
  unknown-method error from an older daemon is a refusal.
- `manager-core`, `tests/repair.rs`: each result is recorded with its activity
  kind and key, the log is passed through, a cancelled password prompt is
  `daemon.polkit_denied`, nothing-to-repair is information, an absent service
  is `daemon.unavailable`.
- `manager-cli`: the command is listed, documents `--check` and the dpkg
  command, takes no positional argument, and refuses under `--execution mock`;
  the report lines name each package in dpkg's words and print only the tail.
- `manager-gui`: the notice is absent for a clean host and names packages and
  the omitted count in both locales; every repair result has its own sentence;
  the copy exists in both locales; the evidence keys are worded; the Health
  screen draws the card and repairs through `manager-core`, and no window
  source runs dpkg or reads its files.

### Review fixes

- `dpkg_state::read` reads the status file as bytes and decodes it lossily. A
  real status file can hold a maintainer name or description that is not UTF-8,
  and reading it as text made every transaction and repair on that machine fail
  with `host_unreadable`. Test:
  `a_status_file_with_bytes_that_are_not_utf8_is_still_read`.
- `MAX_REPAIR_BYTES` is 1 MiB instead of 512 KiB, and an outcome may carry one
  log entry. The old limit was smaller than two full output tails of control
  characters once JSON escaped them. Tests:
  `the_largest_repair_outcome_the_daemon_can_produce_is_accepted`,
  `a_repair_outcome_with_more_than_one_log_entry_is_refused`.
- `RepairStatus` refuses unknown fields. It is adjacently tagged now
  (`{"state": "failed", "detail": {"error_key": ...}}`), because serde ignores
  extra fields beside the tag of an internally tagged unit variant. Test:
  `an_unknown_field_inside_a_repair_status_is_refused`.
- The daemon used to exit 120 seconds after it started, whatever it was doing.
  A fake `dpkg --configure -a` that takes four times a shortened idle period
  showed the timer ending mid-repair. `service::wait_until_idle` now waits while
  a transaction or repair holds the running flag and starts the idle period again
  once it is released; `main.rs` uses it. Tests:
  `the_idle_timer_waits_for_a_repair_that_outlasts_it`,
  `the_idle_timer_ends_after_the_idle_period_when_nothing_runs`.
- A failed repair now tells the person they can run `sudo dpkg --configure -a`
  from a terminal, in `better-manager-cli repair` and on the Health card, in
  both languages.

Each fix's test failed before the fix: the non-UTF-8 test panicked on
`HostUnreadable`, the three `manager-ipc` tests failed on the old limit and the
ignored field, the idle test saw the timer finish first, and the CLI and window
tests found no terminal command.

### Not done, and why

- Nothing here was run against a real dpkg. The daemon's real reader and the
  real `dpkg --configure -a` call are not exercised by any test, and the
  container end-to-end check does not interrupt an install. Doing that safely
  needs a container, not this host.
- The Health card has not been seen on screen. A demo window never asks the
  service, and a real window needs a daemon on the system bus.
- A transaction left in the daemon's journal as executing by the crash is not
  touched by a repair; the daemon still prints its startup message about it.
- `half-installed` is reported and not fixed, as ADR 0014 says.
- The repair inherits the daemon unit's hardening (`ProtectKernelModules`,
  `ProtectKernelTunables`, `ProtectHome`, `DevicePolicy=closed`). A kernel or
  initramfs upgrade, the most common interrupted run, calls `depmod`,
  `update-initramfs`, and `grub-probe`, and those may fail inside the sandbox.
  Not verified. To verify: in a virtual machine, leave a `linux-image` package
  unpacked, run `dpkg --configure -a` under `systemd-run --wait --pipe` with the
  unit's properties passed by `-p`, and compare with an unsandboxed run. The
  unit is unchanged, because its hardening is ADR 0007's decision.
- `AptRun::is_lock_contention` recognizes "Could not get lock" and the two
  frontend-lock messages. What dpkg prints when it cannot take its status
  database lock has not been checked against a real dpkg, so a repair blocked
  that way may be reported as `repair_failed` rather than `apt_busy`. No string
  was added from memory.
- `StageArtifact` does not hold the running flag, so the idle timer does not
  wait for a slow artifact copy; it does wait for the transaction that follows.
- `packaging/e2e/50-better-os-e2e.rules` grants the e2e user only
  `apply-transaction`, so the container check cannot exercise the repair yet.
