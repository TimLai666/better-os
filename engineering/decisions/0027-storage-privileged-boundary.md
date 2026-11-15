# ADR 0027: Better Storage stays unprivileged

## Status

Accepted on 2026-11-15 (ticket 31). Issue #5 deferred "the exact privileged
service boundary and IPC protocol" to a record; this is it.

## Context

Three signals would make the readiness rule (ADR 0026) stronger and all three
need root: per-device writeback in `/sys/kernel/debug/bdi/<dev>/stats`
(debugfs is root-only), a device cache flush (`BLKFLSBUF` needs the block
device open for writing), and an open-writer scan that sees every process.
Changing mount options or a drive's write-cache setting would need root too.
Better OS already has one privileged service, `better-manager-daemon`
(ADR 0007), reached over the system bus and authorized by polkit.

## Decision

- `better-storage-service` runs as the logged-in person, as a systemd user
  unit, and owns `org.betteros.Storage1` on the **session** bus. It has no
  polkit action of its own. Everything it does — watch UDisks2, mount and
  unmount through UDisks2, `syncfs(2)` a mount, read `/proc` — the person could
  do directly.
- Operations that need the system's consent go through **UDisks2's own polkit
  actions** (mount, unmount, power off, format, restore an image). Better
  Storage does not add a second authority over the same devices.
- **No privileged storage helper is built.** The three signals above are
  reported as `NeedsPrivilege` or Unsupported, and the readiness rule is
  designed to be honest without them (a proof says it was not fully
  corroborated).
- No mount option and no hardware write-cache setting is ever changed. That
  is also why uninstalling has nothing to restore beyond the person's own
  preference file.
- If a privileged signal is ever added, it is a narrow read-only method on a
  system-bus service with its own polkit action, taking a device the
  service resolves itself (never a path a client supplies), returning only a
  byte count or a flush result. That is a new record, not an extension of
  `org.betteros.Manager1`, whose actions are package transactions.

## Options rejected

- **Add the reads to `better-manager-daemon`.** It is D-Bus activated, exits
  when idle, and is reviewed as a package executor; a debugfs reader polled per
  device change would keep it resident and widen what its polkit actions mean.
- **A root storage service.** One more root process on every desktop for a
  signal that improves a claim from "no known pending writes" to "no known
  pending writes, also per device". The claim the UI makes does not change.
- **Ask the person to run the service as root.** AGENTS.md forbids a root GUI
  path, and a root session service would own the person's preferences.

## Consequences

- The writer scan is partial on every normal desktop and the proof says so.
- Per-device writeback is almost always unavailable; machine-wide writeback is
  shown as a heuristic only.
- A security review is not needed for this component today because it crosses
  no privilege boundary of its own; adding the helper above would need one.
