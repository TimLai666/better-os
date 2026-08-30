# 31 — Safe direct removal for external storage

**Epic:** Safe direct-removal external storage (Issue #5)
**User Story:** A USB drive can be unplugged without pressing Eject first, and
Better OS only says so when it actually knows no write is pending.
**Blocked by:** none
**Status:** todo

## Goal

Make every supported hot-pluggable external block device default to Direct
Removal mode, and build the system-level layer that knows the truth about
pending writes — because other applications write to the same device, so this
cannot live inside Better Files.

## What it delivers

- `storage-core`: the normalized external-device identity model, the Direct
  Removal and Performance policies, the state machine, the evidence and
  confidence model, the per-device preference model, and rollback and
  restore-default plans. No D-Bus, UDisks2, GPUI, or shell-command detail.
- Device identity built from the most stable available combination — filesystem
  UUID, partition UUID, device serial, model and vendor identifiers, transport
  and topology metadata — never from a transient path like `/dev/sdb` alone, and
  handling duplicate or missing identifiers safely.
- The five states, distinct in the type system and in what they promise: Ready
  to unplug, Writing, Busy, Performance mode, and Unknown or unsupported. Ready
  to unplug is only produced with enough evidence to support it.
- Direct Removal as the default policy for every newly detected supported
  device, including one never seen before. Performance mode exists only as an
  explicit per-device override that is never enabled automatically, requires
  Eject, and states its risk and performance trade-off before activation.
- `storage-platform`: detection of hot-pluggable external block devices,
  distinguishing them from network mounts and internal disks; UDisks2
  integration; observation of mount, unmount, disconnect, and media-change events
  through events rather than high-frequency polling; filesystem-scoped flush
  requests and their verification; pending-writeback and open-use inspection
  where practical; supported mount or cache-policy changes; and honest reporting
  when a condition is unsupported or unverifiable.
- `storage-service`: device-state coordination outside Better Files, receiving
  file-operation completion notifications from Better Files and any future Better
  Copy, publishing typed device-state updates to GUI clients, surviving Better
  Files closing, and never requiring a root GUI.
- Flush behavior: the narrowest applicable operation — the affected filesystem or
  device — batched where correctness permits, never a global `sync` after every
  small operation, and never blocking a UI thread while waiting for persistence.
- Mount-on-open behavior preserved: a device is shown before mounting, mounts on
  open, stays mounted when the user leaves, and Eject remains available as an
  explicit action.
- Failure handling for: unplug during active writes, unplug while being viewed,
  disappearance before flush confirmation, filesystem errors, flush command
  failure, UDisks2 or service restart, duplicate identities, unsupported
  filesystem or transport, and unidentifiable blocking processes. Unsafe removal
  produces an explicit warning and a diagnostic record, and recommends a
  filesystem check where appropriate.
- A written record of which safety signals are authoritative, which are
  heuristic, and which are unavailable, plus the distinction between "no known
  pending writes" and absolute physical persistence.
- Component manifest, health checks, rollback documentation, tests, and
  benchmarks. Removing the component restores the original system mount and cache
  behavior and removes the service integration cleanly.

## Out of scope

- Better Files sidebar presentation, device rows, navigation cleanup, and the
  Eject control's UI (ticket 35).
- Network mounts, cloud storage, optical media, and internal non-hot-pluggable
  disks.
- Guaranteeing safety during active writes, or marketing the feature that way.
- Modifying filesystem or kernel source, replacing UDisks2, and automatic
  filesystem repair.

## Deferred decisions

Issue #5 requires an ADR with measured options rather than a silent choice for:
the exact mount options per filesystem, the exact readiness algorithm and
confidence threshold, whether hardware write-cache settings should ever be
changed, the exact privileged service boundary and IPC protocol, how aggressively
flushes are batched, which filesystems or USB bridges are excluded from a Ready
to unplug claim, and whether mode settings live in Better Files, Better Manager,
or a future Better Settings.

## Acceptance criteria

- [ ] Supported hot-pluggable external block-storage devices default to Direct
      Removal mode, including devices never seen before.
- [ ] External block storage is distinguished from network mounts and internal
      disks.
- [ ] Ready to unplug is never produced while a write or flush is known pending,
      proven by a test that holds a pending write.
- [ ] Writing, Busy, Performance, and Unknown are distinct states carrying
      distinct promises.
- [ ] The policy observes writes originating outside the Better Files process
      where the platform exposes enough information.
- [ ] Performance mode is opt-in, never enabled automatically, and requires
      Eject.
- [ ] Per-device mode preferences survive a reconnect, proven against a stable
      identity rather than a device path.
- [ ] Duplicate or missing device identifiers are handled without collapsing two
      devices into one preference record.
- [ ] Flush after a file operation is filesystem-scoped, and no code path calls a
      global `sync` per small operation.
- [ ] Connect and disconnect events are observed through events, with bounded
      idle overhead and no high-frequency polling.
- [ ] Typed device-state updates reach GUI clients and survive Better Files
      closing.
- [ ] Unsafe removal during a write produces an explicit warning and a diagnostic
      record, and never reports a clean completion.
- [ ] The component manifest declares services, D-Bus/UDisks2 integration,
      configuration paths, permissions, supported distributions, kernels,
      filesystems and transports, health checks, rollback behavior, and
      performance tests.
- [ ] Uninstalling restores the original system behavior.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- A state-machine test suite over recorded UDisks2 and kernel event sequences,
  covering every failure case listed above
- Benchmarks across one large sequential file, many small files, and a
  metadata-heavy tree, recording application-visible copy completion time, time
  until data is actually flushed, total time until Ready to unplug, throughput
  impact versus default system behavior, CPU overhead, service idle overhead, and
  disconnect cleanup latency
- Manifest validation through `better-core`, plus an uninstall smoke asserting
  the original mount and cache behavior is restored
