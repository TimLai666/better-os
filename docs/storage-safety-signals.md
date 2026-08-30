# Storage safety signals: authoritative, heuristic, unavailable

Issue #5 requires this document. Better OS tells a user a device is ready to
unplug, and that claim is only worth anything if it is clear what it rests on.

## The claim being made

"Ready to unplug" means **no known pending writes**: every signal this host
could read reported nothing outstanding for that volume, at the moment it was
checked.

It does not mean **absolute physical persistence**. Those are different claims,
and the second one is not provable from user space on arbitrary hardware:

- a device-internal volatile write cache may still hold data after the kernel
  and the filesystem are done with it
- a USB bridge may report a flush as complete before the media has it
- a drive may report flush support it does not implement correctly
- power loss during a metadata update can damage a filesystem that every signal
  said was idle

Nothing in Better OS should be worded as if unplugging during a write were safe.

## Signals, and what each one is worth

| Signal | Source | Status | What it proves |
| --- | --- | --- | --- |
| Filesystem flush | `syncfs(2)` on a descriptor for the mount | **Authoritative** for the filesystem | Everything the kernel held for that filesystem was written out and the call returned success. This is what a readiness claim is built on. |
| Tracked file operations | Better Files and future Better Copy, over `org.betteros.Storage1` | **Authoritative** for this system's own writes | An operation that started and has not reported completion is outstanding. The service owns this and it is never unknown. |
| Per-device writeback | `/sys/kernel/debug/bdi/<major:minor>/stats` | **Authoritative** when readable, usually **unavailable** | Bytes the kernel still owes this specific device. debugfs is root-only on a normal desktop, so an unprivileged session almost never gets it. |
| Machine-wide writeback | `/proc/meminfo` `Dirty` + `Writeback` | **Heuristic**, and only corroborating | Says something about the machine, nothing about one device. It is recorded and shown, and it never grants or blocks a readiness claim on its own. |
| Open writers | `/proc/<pid>/fd` plus `/proc/<pid>/fdinfo` | **Authoritative for the processes the scan can see**, otherwise partial | A process holding a file on the mount open for writing. Another user's processes cannot be inspected from a session process, so the result carries a coverage value and a count of what it could not read. |
| Device cache flush | `BLKFLSBUF` ioctl on the block device | **Unavailable** unprivileged | Needs the block device open for writing. Reported as unsupported, never as a flush that happened. |
| Device presence, mount state, media change | UDisks2 `InterfacesAdded`, `InterfacesRemoved`, `PropertiesChanged` | **Authoritative** | What is connected and what is mounted. Event-driven; nothing polls. |
| Hot-pluggable and external | UDisks2 `Drive.Removable`, `Drive.MediaRemovable`, `Drive.ConnectionBus`, `Block.HintSystem` | **Authoritative** as a positive rule | A device is treated as external only when a drive object says it is removable on a hot-pluggable bus. Absence of evidence classifies it as internal or unsupported, never as external. |

## The readiness rule as implemented

A mounted volume is reported ready only when all of this holds:

1. no tracked file operation is in flight
2. the open-writer scan produced an observation, and it found no writers
3. a filesystem flush was verified, and no write has been observed since
4. per-device writeback, where readable, is zero
5. the verified flush is not older than the policy's `max_proof_age`
   (5 minutes by default)

An unmounted volume is ready without a flush: there is no filesystem left owing
the device anything, and rule 1 still applies.

Anything else degrades. Bytes pending or an operation in flight is **Writing**.
A process holding the volume is **Busy**. A missing, unreadable, or unsupported
signal is **Unknown** — never a softer version of ready.

### The two thresholds that are policy, not fact

Both are `EvidencePolicy` fields, both are here rather than buried in a
comparison, and both are candidates for the ADR issue #5 asks for:

- **`require_complete_writer_scan`, default false.** A session process cannot
  read another user's `/proc` entries, so on a normal desktop the scan is almost
  always partial. Requiring completeness would leave every device permanently
  unknown for a reason unrelated to the device. The gap is recorded instead:
  a proof built over a partial scan reports `fully_corroborated == false`.
- **`max_proof_age`, default 5 minutes.** Observation is event-driven, so a
  write invalidates a proof immediately. This bound exists for missed events,
  not for expected ones.

## What would need privilege

The service is unprivileged, and every one of the following is a thing it
deliberately does not do:

- **Per-device writeback accounting.** debugfs is root-only. A privileged helper
  reading `/sys/kernel/debug/bdi/<major:minor>/stats` would turn the machine-wide
  heuristic into an authoritative per-device signal.
- **Device cache flush.** `BLKFLSBUF` needs the block device open for writing.
- **A complete open-writer scan.** Seeing every process's descriptors needs
  more than session privilege.
- **Changing mount options or hardware write-cache settings.** Not done at all.
  Issue #5 defers whether it should ever happen, and no code path here changes a
  system mount or cache setting, which is also why uninstalling restores the
  original behavior with nothing to undo.

Adding any of these means a privileged service and a polkit action, which is the
boundary issue #5 explicitly defers to an ADR.

## What is out of scope entirely

Network mounts, cloud storage, optical media, and internal non-hot-pluggable
disks. Network filesystems never appear as UDisks2 block devices, so they cannot
reach this model at all; internal disks are classified and excluded.
