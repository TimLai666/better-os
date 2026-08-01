# ADR 0007: Privileged Daemon IPC Protocol

## Status

Accepted for Better Manager on 2026-08-01. This is the explicit decision that
ADR 0005 deferred and that `AGENTS.md` requires before any real system
installation or rollback is implemented.

## Context

`manager-platform` declares a privileged executor interface with no working
implementation. Every shipped backend refuses to apply a package change, so
Better Manager can plan a transaction but cannot perform one.

`docs/security-and-rollback.md` states the privileged executor must be a
separately reviewed process or service that receives a typed plan, validates it
again, restricts allowed operations, logs each step, runs health checks, and
preserves enough state to restore the previous component. `docs/architecture.md`
names local APT as the execution path and requires the executor to be replaced
without changing the GUI or CLI contract. Neither document chose a transport.

## Decision

The privileged boundary is a **D-Bus system bus service authorized by polkit**.

- Bus name `org.betteros.Manager1`, object path `/org/betteros/Manager1`,
  interface `org.betteros.Manager1`.
- Implemented in Rust with `zbus`; polkit is queried through `zbus_polkit`.
- The service is D-Bus activated (`Type=dbus`), runs as root, and exits after an
  idle period. It is never enabled as a permanently running unit.
- One polkit action, `org.betteros.manager.apply-transaction`, gates every
  mutating method. Read-only status methods are ungated.
- The service ships in its own package, `better-manager-daemon`, separate from
  the `better-manager` GUI/CLI package.

### Wire contract

A new crate, `manager-ipc`, owns the wire types and is the single source of
truth for both sides. Plans and outcomes cross the bus as **serde JSON in a
string argument**, carrying an explicit `protocol_version`. Scalars the bus
itself routes on — transaction id, artifact filename, expected checksum, the
staged file descriptor — stay native D-Bus arguments.

The client streams each artifact to the daemon as a **unix file descriptor**
via `StageArtifact`; the daemon hashes while copying and keeps the file only on
a checksum match. The whole transaction is then handed over in a single
`ApplyTransaction` call. The daemon executes every step, logs it, health-checks
it, and rolls back on failure.

`WireAction` is a closed set: `Install`, `Update`, `Remove`, `Restore`. Enable,
Disable, and Verify do not cross the privileged boundary in protocol version 1.
Verify never needs root, and enabling or disabling a component has no approved
privileged meaning yet.

## Why D-Bus and polkit

It is what the target desktop already does. Zorin OS and Ubuntu run polkit and
the D-Bus system bus for every existing privileged desktop operation, and
PackageKit — the closest analogue to this daemon — is a D-Bus system service
gated by polkit actions. Choosing it means the authentication prompt, the
session/active/inactive distinction, the admin policy, and service activation
are all provided and already audited, rather than reimplemented.

`zbus` keeps the whole stack Rust, which `docs/language-policy.md` requires for
daemons.

## Why not a custom Unix socket

A socket with `SO_PEERCRED` would have the smallest dependency surface and the
easiest audit. It was rejected because authorization, not transport, is the hard
part: proving a caller is an authenticated administrator means either
reimplementing an authentication prompt or degrading to "member of a privileged
group", which is a weaker and less reviewable policy than a polkit action an
administrator can inspect and override. Socket activation would also not give
the desktop an authentication dialog.

## Why not varlink

Varlink has a schema and a simpler protocol, but its Rust ecosystem is thinner
and it has no established polkit integration path. The authorization argument
above then applies again.

## Why not a one-shot pkexec helper

Spawning a short-lived privileged helper per transaction avoids a daemon
entirely, which is attractive. It was rejected because every operation would
prompt for a password with no way to keep an authorization across a
multi-component transaction, progress reporting over stdout is weaker than bus
signals, and a crashed helper leaves no queryable transaction state. The daemon
keeps a journal that survives a client restart; a helper cannot.

## Why JSON in a string argument

The plan and outcome schemas are deep — nested optional values, closed enums,
per-step reports, execution logs, rollback records — and will keep changing.
Native D-Bus types would force a hand-maintained variant encoding in parallel
with the Rust types, which is exactly where mismatches hide. A shared crate with
`#[serde(deny_unknown_fields)]`, an explicit protocol version, and a hard size
cap checked before parsing gives both sides one definition and a strict reject
path. The cost is that the payload is opaque to D-Bus introspection, which the
`ProtocolVersion` property and this document mitigate.

## Why the client downloads and the daemon re-verifies

The unprivileged client fetches artifacts from GitHub Releases and passes an
open file descriptor to the daemon, which re-hashes the bytes it receives.

Putting HTTPS, TLS, and redirect handling inside the root process would enlarge
the privileged attack surface for no gain: the daemon has to treat the client as
untrusted and hash the artifact itself either way, so downloading in the user
session costs nothing in integrity while removing a network stack from root.
Passing a descriptor instead of a path also removes the shared-directory
time-of-check/time-of-use problem entirely, and proxy and network configuration
stay in the session that owns them.

## Daemon-side revalidation

The daemon trusts nothing from the client and does not read manifests at all.
Before touching any state it checks payload size, protocol version, and strict
field shape; that every component name matches a hard `better-*` whitelist
independent of any catalog; that the plan's target release and architecture
match what the daemon reads itself from `/etc/os-release` and `dpkg`; that
artifact filenames carry no path separators and match the release-asset naming
contract; and that the resolved path stays inside its own cache directory. Before
each install it re-hashes the cached file and cross-checks the `.deb` control
fields, so a package cannot claim a name it does not have. It then compares
`dpkg` state against the plan's expected prior version and refuses on drift.

A refusal at this stage happens before any mutation and therefore writes no
rollback record, which is what `docs/manager-ux-logic.md` requires.

## Systemd hardening

The unit sets `ProtectHome`, `PrivateTmp`, `ProtectKernelModules`,
`ProtectKernelTunables`, `ProtectKernelLogs`, `ProtectClock`,
`ProtectControlGroups`, `RestrictRealtime`, `RestrictNamespaces`,
`LockPersonality`, `SystemCallArchitectures=native`, and `DevicePolicy=closed`.

It deliberately does **not** set:

- `NoNewPrivileges` — it is inherited by children, and dpkg maintainer scripts
  of dependency packages may legitimately execute setuid helpers. PackageKit
  omits it for the same reason.
- `ProtectSystem` — dpkg must write `/usr` and `/etc`.
- `PrivateNetwork` — `apt-get install ./local.deb` may fetch dependencies from
  the Ubuntu archives.
- `MemoryDenyWriteExecute` — interpreters invoked by packaging tooling may need
  writable executable mappings.

## Consequences

- `manager-core` stays free of D-Bus. The client half of the protocol lives in
  `manager-platform` behind a `dbus-client` feature, so the planner and state
  machine keep their current shape.
- The mock execution path remains, and remains the default for tests and demos.
  Replacing the executor does not change the lifecycle stages the GUI and CLI
  already drive.
- A privileged daemon now exists to review. Its package, unit, busconfig, and
  polkit policy are review artifacts in their own right.
- The daemon is the first component in the project to ship Debian maintainer
  scripts.

## Accepted residual risk

APT runs dpkg maintainer scripts as root, and those scripts are arbitrary code
from dependency packages. No sandbox around the daemon changes that; it is
inherent to installing Debian packages. The mitigations are the `better-*`
component whitelist, the checksum the authorizing administrator approved, the
control-field cross-check, and the polkit administrator authentication itself.

If the daemon dies mid-transaction, the journal entry stays in an executing
state and is marked as requiring manual recovery on the next start. The daemon
never silently resumes an interrupted APT run.

## Deferred

Package signing and its signature format, the public APT repository, release
channels, running a component's own `--version` as a health probe under dropped
privileges, and a `dpkg --configure -a` repair action remain undecided. Protocol
version 1 also leaves Enable, Disable, and Verify outside the privileged
boundary.
