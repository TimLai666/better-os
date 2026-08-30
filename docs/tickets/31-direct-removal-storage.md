# 31 — Safe direct removal for external storage

**Epic:** Safe direct-removal external storage (Issue #5)
**User Story:** A USB drive can be unplugged without pressing Eject first, and
Better OS only says so when it actually knows no write is pending.
**Blocked by:** none
**Status:** done for the first implementation (issue #5 scope). The GUI half is
ticket 35; hardware benchmarking and two deferred decisions are follow-ups
below.

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
- [ ] The component manifest declares services, D-Bus/UDisks2 integration,
      configuration paths, permissions, supported distributions, kernels,
      filesystems and transports, health checks, rollback behavior, and
      performance tests. **Partly.** `components/manifests/better-storage.yaml`
      declares everything the schema 2 manifest can express and passes
      `better-core` validation. Supported kernels, filesystems, and transports
      have no manifest fields; adding them is a schema change and is listed as a
      follow-up rather than smuggled into unvalidated keys.
- [ ] Uninstalling restores the original system behavior. **Partly.**
      `restore_defaults` returns every device to Direct Removal, is idempotent,
      and is tested; nothing here ever changed a system mount or cache setting,
      so there is nothing else to undo. The packaging-level uninstall smoke does
      not exist yet, because the component is not packaged yet.

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

## Follow-ups

- Benchmark real devices: one large sequential file, many small files, and a
  metadata-heavy tree, across exFAT, NTFS, and ext4 on a USB flash drive, an
  external SSD, and an external HDD. Record application-visible copy completion
  time, time until data is actually flushed, total time to Ready to unplug,
  throughput against default system behavior, CPU overhead, and disconnect
  cleanup latency. This needs hardware and has not been done; it is not
  something the synthetic benchmarks stand in for.
- Write the ADR issue #5 asks for on the readiness algorithm and confidence
  threshold. The two values that need it are `EvidencePolicy`'s
  `require_complete_writer_scan` (default false) and `max_proof_age` (default
  five minutes), both documented in `docs/storage-safety-signals.md`.
- Write the ADR on the privileged boundary. Per-device writeback accounting, a
  device cache flush, and a complete open-writer scan all need privilege this
  service deliberately does not take.
- Decide whether the manifest schema should gain supported-kernel,
  filesystem, and transport fields, which the acceptance criteria ask a manifest
  to declare and schema 2 cannot express.
- Package the component and add the uninstall smoke test.
- Wire Better Files to `org.betteros.Storage1`: the sidebar, the device row, the
  Eject control, and the navigation cleanup after a disconnect. Ticket 35.
