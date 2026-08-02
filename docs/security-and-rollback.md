# Security and Rollback Boundary

## Current guarantees

- CLI and GUI never change the system themselves. They plan, fetch, and ask.
- Neither invokes `sudo`, `apt`, or an arbitrary manifest command. Reading
  installed versions with `dpkg-query` is unprivileged and read-only.
- A plan states whether it is a simulation or a real transaction, and a real
  one must name the artifact of every step that installs something.
- Only `manager-daemon` changes the system, and only for a caller polkit
  authorized for `org.betteros.manager.apply-transaction`.
- Monitor exports redact inventory values marked sensitive.

## The privileged boundary

`manager-daemon` is a separately reviewed root process, reached over the D-Bus
system bus and packaged on its own as `better-manager-daemon`. ADR 0007 records
the protocol decision and its rejected alternatives.

It receives a typed plan and validates it again from scratch rather than
trusting the client: payload shape and limits, a hard `better-*` component
whitelist independent of any catalog, its own reading of the release and
architecture, artifact names confined to its cache, a re-hash of the cached
bytes immediately before installing, a cross-check of the `.deb` control fields,
and a comparison of dpkg state against the plan's expected prior version. It
logs every command it runs, health-checks what it applied, and keeps a rollback
record per component.

After an install, update, or restore is applied, it atomically records the
actual installed version and the artifact filename and checksum under
`/var/lib/better-os/installed/<component>.json`. Rollback accepts that artifact
only when its recorded version matches the version dpkg reports before the
transaction. A missing or mismatched record is manual recovery; the artifact
being applied is never a fallback for an older version.

The rollback record is written immediately before the first APT call that
touches a component, and never earlier. A plan refused during revalidation
therefore leaves no restore point behind, because nothing changed. After a
failure the daemon reports how far it got — restored, partially restored, or
needing a person — and never claims a host was put back when it was not.

If the daemon dies mid-transaction its journal entry stays in an executing
state and is reported as needing manual recovery on the next start. An
interrupted APT run is never silently resumed.

APT executes dpkg maintainer scripts as root, and those are arbitrary code from
dependency packages. That is inherent to installing Debian packages and is an
accepted residual risk; see ADR 0007.

## Release security

The manager verifies artifact checksums before installation. Package signing is
deferred and must be decided before a future signed distribution channel. The
project is licensed under GPL-3.0-or-later. Every Debian package carries the
root license and the generated third-party license inventory under
`/usr/share/doc/<package>/`.
