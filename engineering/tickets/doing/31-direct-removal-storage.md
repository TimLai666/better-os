# 31 — Safe direct removal for external storage

**Epic:** Safe direct-removal external storage (Issue #5)
**User Story:** A USB drive can be unplugged without pressing Eject first, and
Better OS only says so when it actually knows no write is pending.
**Blocked by:** none

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

- [x] Supported hot-pluggable external block-storage devices default to Direct
      Removal mode, including devices never seen before.
      `storage-service/tests/coordinator.rs::a_device_this_host_has_never_seen_defaults_to_direct_removal`
- [x] External block storage is distinguished from network mounts and internal
      disks. Classification is a positive rule over the UDisks2 `Drive`
      properties; network filesystems never appear as UDisks2 block devices at
      all. `storage-platform/src/model.rs` tests plus
      `internal_disks_and_devices_without_a_drive_are_never_admitted`.
- [x] Ready to unplug is never produced while a write or flush is known pending,
      proven by a test that holds a pending write.
      `storage-service/tests/coordinator.rs::ready_is_never_reported_while_a_write_is_pending`,
      and `ReadinessProof` cannot be constructed any other way.
- [x] Writing, Busy, Performance, and Unknown are distinct states carrying
      distinct promises. Six distinct kinds, exactly one of which permits direct
      removal, asserted in `storage-core/src/state.rs` and again on the wire in
      `storage-service/src/protocol.rs`.
- [x] The policy observes writes originating outside the Better Files process
      where the platform exposes enough information. Pending writeback and open
      writers come from the kernel, not from a file manager. The parsers are
      tested against fixture trees; an end-to-end run against a real external
      device mid-write has not been done — see the follow-ups.
- [x] Performance mode is opt-in, never enabled automatically, and requires
      Eject. There is no code path that sets it without a
      `PerformanceOptIn` covering every key in `PERFORMANCE_RISK_KEYS`.
- [x] Per-device mode preferences survive a reconnect, proven against a stable
      identity rather than a device path.
      `a_performance_override_survives_the_service_restarting` reconnects the
      same volume on a different kernel path.
- [x] Duplicate or missing device identifiers are handled without collapsing two
      devices into one preference record. Two devices sharing an identity are
      both marked ambiguous and neither inherits the preference; a device with
      no stable identifier can never hold one.
- [x] Flush after a file operation is filesystem-scoped, and no code path calls a
      global `sync` per small operation. The only flush is `syncfs(2)` on the
      mount; the fake records every path flushed and the test asserts exactly one
      per completed operation.
- [x] Connect and disconnect events are observed through events, with bounded
      idle overhead and no high-frequency polling. UDisks2 `InterfacesAdded`,
      `InterfacesRemoved`, and `PropertiesChanged`; no timer exists anywhere in
      the service, asserted by `an_idle_service_publishes_nothing_and_runs_no_timer`.
- [x] Typed device-state updates reach GUI clients and survive Better Files
      closing. `org.betteros.Storage1` on the session bus, exercised over a
      private `dbus-daemon` in `storage-service/tests/dbus_service.rs`.
- [x] Unsafe removal during a write produces an explicit warning and a diagnostic
      record, and never reports a clean completion.
      `unplugging_during_a_write_produces_a_warning_and_a_diagnostic_record`.
- [x] The component manifest declares services, D-Bus/UDisks2 integration,
      configuration paths, permissions, supported distributions, kernels,
      filesystems and transports, health checks, rollback behavior, and
      performance tests. Kernels, filesystems, and transports are the optional
      `hardware_support` group (contract: `engineering/contracts/component-manifest.md`),
      validated in `better-core` and tied to the code by
      `storage-service/tests/manifest.rs`. Released managers skip the group.
- [x] Uninstalling restores the original system behavior. `restore_defaults`
      returns every device to Direct Removal and is tested; nothing ever changed
      a system mount or cache setting (ADR 0027). The container check in
      `packaging/test-daemon-e2e.sh` installs the package, starts the service
      against a real udisksd and session bus, stops it, removes the package,
      and asserts no file, no process, and no state is left. **CI-only**: no
      Docker daemon in the worktree, so it has not been run locally.
- [ ] Real-device benchmarks recorded. The harness and record format exist
      (`better-storage-bench`, `engineering/performance/storage-performance.md`);
      the numbers need the owner's drives.

## Verification

Run and passing at the time of the commit, crate-scoped:

- `cargo fmt --all -- --check` — clean
- `cargo check -p storage-core -p storage-platform -p storage-service --all-targets` — clean
- `cargo test -p storage-core -p storage-platform -p storage-service --all-targets` —
  129 passing, 3 ignored (the live hardware checks)
- `cargo clippy -p storage-core -p storage-platform -p storage-service --all-targets -- -D warnings` — clean

Not run in this ticket: the full-workspace gates, which are the merge step's job.

State-machine coverage over recorded event sequences lives in
`storage-core/tests/event_sequences.rs`: connect → mount → write → flush → idle →
unplug, unplug during a write, disappearance before flush confirmation, flush
failure, filesystem error, service restart (idle and mid-copy), an unidentifiable
blocker, an unsupported filesystem, and duplicate identity.

Live checks against this machine, by hand:

- `cargo run -p storage-platform --bin better-storage-doctor` reported 15 block
  devices, 0 external hot-pluggable (nothing was plugged in), every NVMe
  partition correctly classified internal with a stable identity, writeback
  available only as the machine-wide figure, and the open-writer scan on `/`
  returning 73 writers with 365 processes it could not inspect. That is the
  unprivileged picture the safety document describes.
- `cargo test -p storage-platform --test live_smoke -- --ignored`: two of the
  three pass; `an_external_device_is_detected_and_identified_stably` fails by
  design with "plug one in and run this again".

Benchmarks that ran (synthetic, no hardware):

- state-machine throughput: 7.3M events/s, 136 ns per event, release build
- event-to-state-update latency through the service: p50 17 µs, p99 23 µs
- idle service: no timer, no published update, no state change

## Remaining

- **Only this:** run the real-device benchmark on the owner's hardware and
  paste the rows into `engineering/performance/storage-performance.md`. One
  command per device:
  `cargo run --release -p storage-platform --bin better-storage-bench -- --mount /media/$USER/LABEL --medium usb-flash --out engineering/performance/storage-runs.jsonl`
  across exFAT, NTFS, and ext4 on a flash drive, an external SSD, and an
  external HDD. Disconnect cleanup latency needs a physical unplug and is
  measured by hand from the service's state updates. Then move this ticket to
  `done/`.
- Confirm on the first CI run that the new better-storage container check
  passes; it has not run anywhere yet.

## Follow-ups

Packaging and the Better Files wiring were done later (tickets 36 and 35). The
readiness and privilege ADRs (0026, 0027), the `hardware_support` manifest
group, and the better-storage container check were done in the second pass
(see Notes). What stays open is the real-device benchmark under Remaining.

## Notes

2026-11-15, second pass on branch `ticket-31`:

- ADR 0026 (readiness rule, no numeric confidence score, the two thresholds)
  and ADR 0027 (the service stays unprivileged; UDisks2's polkit actions are
  the only consent path; no privileged helper) are written and linked from the
  safety signals spec and `architecture.md`.
- `hardware_support` added to the manifest schema without a version bump:
  the top level does not deny unknown keys, so released managers skip it, and
  a test fails if that ever changes. Entries are tokens, not a closed set, so a
  later value never makes a released manager refuse the catalog.
  `HOT_PLUGGABLE_TRANSPORTS` in `storage-platform` is now the one list both
  `classify` and the manifest test use.
- The container check needed `udisks2` in the e2e image (it is a Depends and
  the container has no network at test time). Risk to watch on the first CI
  run: udisksd started by hand with no udev database; if it refuses to start
  there, the check fails with its log printed.
- `better-storage-bench` is a measurement tool, not packaged. Its records
  carry no numbers until the owner runs it.

Carried over from the retired delivery status record.

Ticket 31 added the three storage crates on branch `ticket-31`, since merged.
`storage-core` is the whole decision: normalized device identity, the Direct
Removal and Performance policies, six distinct states, the evidence model, the
per-device preference model, and restore-default plans, with no D-Bus, UDisks2,
`/proc`, or GPUI anywhere in it. The invariant everything else leans on is that
`ReadyToUnplug` cannot be built without a `ReadinessProof`, a proof cannot be
built without positive evidence, and the proof type has no `Deserialize`, so no
client can hand the service a green light it never earned.

Identity combines every stable identifier the platform reported — WWN, serial,
partition UUID, filesystem UUID — and falls back to vendor, model, and the
`by-path` port chain, which is honestly labelled weak. A device known only by
`/dev/sdb1` is volatile and can never hold a preference. Two connected devices
that report the same identity are both marked ambiguous and neither inherits the
stored preference.

`storage-platform` holds UDisks2 over zbus with real interface definitions,
`syncfs` on a mount for the flush, `BLKFLSBUF` reported honestly as unsupported
without privilege, per-backing-device writeback where debugfs allows it and the
machine-wide `/proc/meminfo` figure as a heuristic fallback, and a
`/proc/<pid>/fd` writer scan that counts what it could not inspect rather than
implying it saw everything. Every one of those is a trait with a fake behind the
`test-support` feature.

`storage-service` owns the session-long state and publishes it over
`org.betteros.Storage1` on the **session** bus — unprivileged, no polkit action,
because everything it does is something the logged-in user could do directly.
`engineering/specs/storage-safety-signals.md` records which signals are authoritative, which
are heuristic, which are unavailable, and exactly what would need privilege;
issue #5 defers that boundary to an ADR and nothing here pre-empts it.

129 tests pass across the three crates, plus three `#[ignore]`d live checks. The
probe binary run against this machine found 15 block devices, no external
hot-pluggable device connected, writeback available only as the machine-wide
figure, and 365 processes the writer scan could not inspect on `/` — which is
the unprivileged picture the safety document describes rather than a limitation
discovered later. Gates were crate-scoped to avoid rebuilding GPUI in the
worktree; the workspace gate belongs on the main checkout after merge. Real
device throughput across exFAT, NTFS, and ext4 on flash, SSD, and spinning
external disks needs hardware and is recorded as a follow-up in the ticket, not
approximated by the synthetic benchmarks.
