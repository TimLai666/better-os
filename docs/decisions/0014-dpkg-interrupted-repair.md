# ADR 0014: The privileged daemon repairs an interrupted dpkg run

## Status

Accepted by the project owner on 2026-09-27. It adds one privileged operation to
the protocol ADR 0007 decided, and closes the `dpkg --configure -a` item that
ADR 0007 left under Deferred.

## Context

When a machine loses power or crashes while dpkg is unpacking or configuring
packages, dpkg records how far it got and stops. Every later `apt-get` run then
refuses with "dpkg was interrupted, you must manually run 'sudo dpkg --configure
-a'". Better Manager installs through `apt-get`, so from that point every
install, update, remove, and restore fails, and the only way out was a root
terminal. The person Better Manager is written for does not have one open.

ADR 0007 keeps every host change inside `better-manager-daemon`, behind polkit,
with a closed set of operations. A repair is a host change, so it needs its own
decision rather than a new `WireAction`.

## Decision

The daemon offers exactly one repair: it runs `dpkg --configure -a`.

### What it runs

The argument vector is fixed in the daemon:

```
dpkg --force-confdef --force-confold --configure -a
```

The two `--force-conf*` flags are the ones every install already passes through
apt's `Dpkg::Options`. They keep a changed configuration file from stopping the
run at a prompt nobody can answer, because the daemon has no terminal. The
environment is cleared the same way the APT driver clears it: `PATH`,
`DEBIAN_FRONTEND=noninteractive`, `APT_LISTCHANGES_FRONTEND=none`, `LC_ALL=C`.

Nothing in the command comes from the client. The D-Bus method takes no
argument at all, so there is no package name, path, or option for a caller to
supply and no string for the daemon to validate.

### The D-Bus surface

Two methods join the `org.betteros.Manager1` interface:

| Method | Arguments | Returns | Authorization |
| --- | --- | --- | --- |
| `GetPackageSystemState` | none | `PackageSystemState` as JSON | none, like `GetStatus` |
| `RepairPackageSystem` | none | `RepairOutcome` as JSON | `org.betteros.manager.repair-package-system` |

Both documents live in `manager-ipc` beside the plan and the outcome, with the
same rules: `deny_unknown_fields`, closed enums, a size limit checked before
parsing, and `protocol_version` 1. The existing documents do not change, so the
protocol version stays 1. A client that calls `RepairPackageSystem` on a daemon
older than this decision gets D-Bus's unknown-method error, and reports it as a
daemon refusal rather than as a repair that ran.

`RepairOutcome` is reported the way a transaction step is: a status and the log
entry of the command it ran, with its argument vector, exit code, and the tail
of its output, capped at the same 64 KiB per stream. An outcome carries at most
one log entry, because the repair runs one command. Both documents are limited
to 1 MiB, which covers the worst outcome the daemon can write: two full tails of
control characters, which JSON escapes to six bytes each, plus the longest
remaining state. Its status is one of
`repaired`, `still_interrupted`, or `failed` with a stable error key. It also
carries dpkg's state as the daemon read it after the run.

### The polkit action

`org.betteros.manager.repair-package-system`, shipped in the daemon package in
the same policy file as `apply-transaction`:

| Session | Default |
| --- | --- |
| active | `auth_admin` |
| inactive | `auth_admin` |
| any | `auth_admin` |

This is stricter than `apply-transaction`, which keeps an active session's
authorization for polkit's keep window. The difference is scope. A transaction
only ever touches `better-*` packages the daemon verified itself. `dpkg
--configure -a` configures every package dpkg left unfinished, whoever
installed it, and runs their maintainer scripts as root. That is worth asking
for every time, and a repair is rare enough that asking costs little.

### How the daemon knows a repair is needed

It reads dpkg's own record and never an error message:

- `/var/lib/dpkg/status`: a package whose `Status` field ends in
  `half-installed`, `unpacked`, `half-configured`, `triggers-awaited`, or
  `triggers-pending` was left mid-operation. A package at rest is `installed`,
  `config-files`, or `not-installed`.
- `/var/lib/dpkg/updates/`: a file whose name is all digits is an entry dpkg
  wrote to its journal and has not folded back into the status file. This is
  the same test apt makes before it prints the "dpkg was interrupted" message.

Either one means the package system needs a repair. `PackageSystemState` reports
both: whether the journal has entries, and each unfinished package with its
status, up to 64 packages plus a count of the ones left out.

The daemon reads these files itself. The client could read them too, since they
are world-readable, but then two programs would each decide what "interrupted"
means, and the daemon has to decide it anyway before it runs anything.

### What the daemon refuses

`RepairPackageSystem` runs nothing when:

- polkit does not authorize the caller for the repair action. Authorization for
  `apply-transaction` does not count. The error is `daemon.error.unauthorized`.
- a transaction or another repair is running in this daemon. Both share the one
  running flag `ApplyTransaction` already uses, so a repair never overlaps a
  transaction. The error is `daemon.error.busy`.
- dpkg's record shows nothing unfinished. The error is
  `daemon.error.nothing_to_repair`. Running dpkg on a machine that needs nothing
  would only be a root process with nothing to do.
- dpkg's record cannot be read. The error is `daemon.error.host_unreadable`.

When the run itself fails, the outcome says so instead of the call failing: a
dpkg lock held by another package manager is `daemon.error.apt_busy`, and any
other non-zero exit is `daemon.error.repair_failed`, with the log either way. A
run that exits 0 and leaves packages unfinished is `still_interrupted`, with
the packages that remain.

`ApplyTransaction` also reads the same record before it touches anything. If
dpkg is interrupted, the plan is refused with `daemon.error.dpkg_interrupted`
before any rollback record is written, instead of letting `apt-get` fail and
then attempting a rollback that fails for the same reason.

### Clients

`better-manager-cli repair` and the Health screen of the window both reach the
daemon through `manager-core`, which asks for the state, asks for the repair,
and writes the result into the activity log. Neither runs dpkg or reads its
files. `better-manager-cli repair --check` only asks for the state. A demo
window never asks, and `--execution mock` refuses the command, because a
simulated repair would report a change to a machine nothing changed.

## Why not a new `WireAction`

A plan step names one `better-*` component and the daemon checks every step
against that whitelist. A repair names no component and acts on packages the
daemon did not install. Putting it in the plan would mean either a step with no
component, which every other check has to learn to skip, or a component name
that means "everything", which is the kind of exception the whitelist exists to
prevent. A separate method with no argument keeps the plan's rules unchanged.

## Why not run `apt-get -f install` as well

`apt-get -f install` resolves dependencies, which can install or remove
packages the person never asked about, from the network. `dpkg --configure -a`
only finishes what dpkg already started, from files already on disk. Anything
the repair leaves unfinished is reported as `still_interrupted`, and the person
decides what to do next.

## Consequences

- The daemon's D-Bus surface grows by two methods and its policy file by one
  action. `packaging/verify-deb.sh` fails a daemon package whose policy does not
  declare the new action.
- The daemon no longer exits on a fixed timer from startup. It exits once 120
  seconds have passed with no transaction or repair running, so a repair or an
  install that takes longer than that still reaches its caller.
- A repair is not written to the transaction journal. dpkg's own record is the
  durable state here: a repair that is itself interrupted leaves dpkg
  interrupted, and the next `GetPackageSystemState` reports exactly that.
- The daemon's journal can still hold a transaction in the executing state from
  the crash that caused all this. The startup message about it is unchanged, and
  the repair does not rewrite it.

## Accepted residual risk

`dpkg --configure -a` runs the maintainer scripts of every unfinished package,
including packages Better OS did not install. That is the same code a person
would run from a terminal to recover, and the `auth_admin` default puts an
administrator's password in front of it every time.

The check cannot tell a run that was interrupted from one that is still in
progress under another frontend, such as unattended-upgrades in the middle of
a run. Both leave the same record. In that case a repair fails on dpkg's lock
and reports `apt_busy`, and a transaction is refused as `dpkg_interrupted`
until the other run finishes. Neither changes anything.

The repair runs inside the daemon's systemd unit and inherits its hardening,
which ADR 0007 decided and this decision does not change: `ProtectKernelModules`,
`ProtectKernelTunables`, `ProtectHome`, and `DevicePolicy=closed`, among others.
The most common interrupted run is a kernel or initramfs upgrade, and its
maintainer scripts call `depmod`, `update-initramfs`, and `grub-probe`. With
`/usr/lib/modules` made inaccessible and block devices closed, those can fail
inside the sandbox when they would succeed from a terminal. This has not been
verified. The way to verify it is a virtual machine, never a host: leave a
`linux-image` package unpacked and unconfigured, then run
`systemd-run --wait --pipe` with the unit's hardening properties set by `-p` on
`dpkg --configure -a`, and compare it with the same command run outside the
sandbox. Until that is done, a failed repair tells the person they can run
`sudo dpkg --configure -a` from a terminal instead, in the command line and in
the window, in both languages.

`half-installed` is reported but `dpkg --configure -a` does not fix it; that
package needs to be reinstalled or removed, which this decision does not offer.
The repair reports it as `still_interrupted`.
