# Security and Rollback Boundary

## Current guarantees

- CLI and GUI never change the system themselves. They plan, fetch, and ask.
- Neither invokes `sudo`, `apt`, or an arbitrary manifest command. Reading
  installed versions with `dpkg-query` is unprivileged and read-only.
- A plan states whether it is a simulation or a real transaction, and a real
  one must name the artifact of every step that installs something.
- Only `manager-daemon` changes the system, and only for a caller polkit
  authorized for `org.betteros.manager.apply-transaction`, or, for the one
  repair of an interrupted dpkg run, for
  `org.betteros.manager.repair-package-system` (ADR 0014). The repair runs a
  fixed `dpkg --configure -a` and takes nothing from the caller.
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

## What the health check requires in the person's session

After the daemon's own health check, `manager-core` checks what lives in the
person's session for every step that leaves a component enabled. A failed
check fails the operation, and an install, update, or restore that fails it is
rolled back, so each rule here says only what Better OS controls.

- **User units.** Every unit the component's package installed under
  `/usr/lib/systemd/user` (from dpkg's file list) is enabled and running. A
  unit that exited, even with status 0, is not running, and the startup pass
  and Verify (Check again) start it like a stopped one (ticket 131). The units
  that still exit 0 are the Awake tray's and the Monitor tray's Quit and
  `better-window-attention` when another instance owns its name; none of that
  is the component being off, which only disabling it in Better Manager or
  removing it means. A second window-attention instance exits while the first
  runs, so the unit stays active and nothing restarts in a loop. A unit that
  systemd records as killed by its stop signal is stopped, not finished.
- **GNOME Shell extensions.** Every extension the component's package
  installed under `/usr/share/gnome-shell/extensions`, and every extension its
  installed companion packages installed there (ADR 0018), is in the person's
  `enabled-extensions`. One missing after Better Manager wrote it fails the
  check.
- **GNOME's switch that turns off all extensions** (`disable-user-extensions`)
  is the person's own decision about the whole desktop, which Better OS never
  changes. For every component it is a warning in Activity
  (`shell_extension.all_turned_off`), never a failed check: the extensions
  stay in the list and run when the switch goes off. Failing would roll back
  Better Manager's own update, or any component's, over a setting the update
  cannot change (ticket 131). The component's own page says what the switch
  costs it (Better Touchpad: no gesture reaches the desktop).

## Release security

The manager verifies artifact checksums before installation. Package signing is
deferred and must be decided before a future signed distribution channel. The
project is licensed under GPL-3.0-or-later. Every Debian package carries the
root license and the generated third-party license inventory under
`/usr/share/doc/<package>/`.
