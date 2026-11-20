# Ticket 204 — The privileged service adds System76's pinned source and installs and purges the session's closure

**Spec:** better-os-session.md S1, S3 ("Packages and where they come from")
**Blocked by:** 201

## Why

C2 allows Better Manager to add System76's APT source, its key, and a pin
limited to the session's closure (ADR 0038). `manager-daemon` today installs
only local artifacts it verified by checksum; the closure comes from an
archive APT verifies by its signature. That is a new kind of privileged step,
so it is built and tested on its own, with nothing taken from a manifest.

## What to build

- **A closed table in `manager-daemon`** with one entry: the source URL
  `https://apt.pop-os.org/release`, suite `noble`, component `main`, the
  fingerprint `63C4 6DF0 140D 7389 6142 9F4E 204D D8AE C33A 7AFF`, the
  keyring's bytes shipped in the `better-manager-daemon` package
  (`/usr/share/better-os/keyrings/`) and checked against the fingerprint
  before use, and the 14 package names of the spec. Nothing in a plan or a
  manifest can name a source, a key, or a package outside it.
- **Methods** on the daemon's interface (ADR 0007's protocol; names and wire
  types in `manager-ipc`), each with its own polkit action
  (`org.betteros.manager.desktop-session`, `auth_admin_keep`):
  - add the source (deb822 file under `/etc/apt/sources.list.d/`, keyring
    under `/etc/apt/keyrings/`, pin under `/etc/apt/preferences.d/`: the
    origin at −1, the 14 names at 500), `apt-get update` limited to it, and the
    `gdm3` preseed of `shared/default-x-display-manager`;
  - install the closure and the session package (the session package as a
    verified local artifact, as today) in one transaction, with the existing
    health check and rollback;
  - purge the closure, the session package, and the Fcitx 5 packages the
    install recorded as added, then remove the source, keyring, and pin;
  - the deferred purge of the spec's "Removing from inside the session": a
    transient unit that waits until logind reports no session of the Better OS
    session for any user, then purges (ADR 0028's shape).
- **Refusals**: a pin or source file present but different from the table's
  (reported, never overwritten); a closure that `apt-get -s` would resolve
  with anything from System76's origin outside the 14 names (refused before
  anything is installed); a removal while another user's Better OS session is
  open (refused with that user's name).
- **Tests** with the APT driver faked, a pin tested against a captured copy
  of System76's `noble` `Packages` and a host `apt-cache policy` (the evidence
  of ticket 143), and the container end-to-end check extended to add the
  source from a local mirror fixture, install, and purge (no network at test
  time, AGENTS.md).

## Done when

- [ ] Unit tests for the table's integrity (fingerprint matches the shipped
      keyring), each file's exact contents, every refusal above, and that no
      IPC argument can carry a name or URL.
- [ ] The container check installs a two-package fixture closure from the
      local mirror under the real pin, shows `apt-get -s dist-upgrade` taking
      nothing else from it, and purges back to the starting `dpkg -l`.
- [ ] In ticket 201's VM, the daemon adds the real source and installs and
      purges the real closure, leaving `/etc/apt` byte-for-byte as before.
- [ ] `engineering/specs/security-and-rollback.md` gains the new steps and
      their undo.
- [ ] fmt, clippy on stable, the crates' tests, and the container check pass.
