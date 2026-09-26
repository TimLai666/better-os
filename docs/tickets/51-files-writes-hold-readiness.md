# Ticket 51 — Better Files' own writes to an external disk hold its readiness

**Epic:** Safe direct-removal storage (Issue #5) · **User Story:** a person
copying to a USB disk with Better Files is not told the disk is safe to remove
while the copy is still running · **Branch:** `ticket-50-52` ·
**Blocked by:** none · **Status:** implemented, not merged

## What it delivers

`StorageClient::notify_operation_started` and `notify_operation_completed`
exist and are tested against a running service, and nothing calls them. A Better
Files copy to an external device therefore reaches the storage service only
through platform signals, and readiness can be claimed before our own write has
finished. After this ticket, a Better Files job that writes to a mounted
external device is registered with the storage service as a tracked operation
from start to finish.

## Acceptance criteria

- [ ] A destination path is mapped to the device whose mount point is its
      longest prefix. Paths on no tracked device map to nothing and send
      nothing.
- [ ] The mapping resolves `..` and symbolic links in the destination before
      matching, and a prefix match respects path components (`/media/a`
      does not match `/media/ab`).
- [ ] A job that writes to a tracked device sends started before its first
      write and completed when it ends, whether it succeeded, failed, or was
      cancelled. A move from one device to another notifies both.
- [ ] With no storage service, the in-process engine receives the same
      notifications, so its readiness answer is the same.
- [ ] A notification that fails is logged and never fails the job.
- [ ] The mapping lives outside GPUI code and is unit-tested. The job-to-
      notification wiring is tested with a fake device link.
- [ ] `files-operations` gains no dependency on any storage crate.

## What was built

A Better Files job that writes to a mounted external device is now registered
with the storage layer from before its first write until it ends.

- **`files-operations` gained a generic observer, not a storage dependency.**
  `JobObserver` has `starting` and `finished`; `JobEngine::with_observer` takes
  one. The engine calls `starting` on the worker thread after the job is marked
  running and before it plans or writes, and `finished` after the last write
  (and after a rollback) but before the terminal state is set, so nobody can
  see a finished job the observer has not heard about. A job cancelled while
  queued never started and produces neither call; a retried job produces a new
  pair. The ticket pointed at `JobEvent::Started`; that event is delivered to a
  channel after the worker has moved on, so it cannot promise "before the first
  write" and was not used.
- **The mapping lives in `crates/files-gui/src/tracking.rs`, with no GPUI.**
  `written_paths` names what each operation changes: a copy its destination, a
  move its destination and every source, a duplicate or rename its sources, a
  trash its sources and the trash, a restore the trash and the path the
  `.trashinfo` records, a permanent delete its targets, a checksum nothing.
  `resolve_path` canonicalizes the deepest existing ancestor and applies the
  rest lexically, and `device_for` picks the longest whole-component mount
  point. It all runs on the job's worker thread, never the render thread.
- **`StorageTracker` is the engine's observer.** It is process-wide beside the
  one engine (`files_gui::shared_tracker`), and the window attaches its link to
  it. It remembers which devices each job was announced to and releases exactly
  those, including one whose started notice failed, since the service may have
  recorded it anyway. The operation is named `better-files:<pid>:job-<n>`.
- **The link carries it to either backend.** `DeviceLink` gained
  `mounted_devices`, `operation_started`, and `operation_completed`.
  `StorageLink` answers `operation_started` only once the service or the
  in-process coordinator has it, and gives up after two seconds, logging to
  standard error and letting the job proceed. Completion does not wait: the
  service flushes before it answers, so the service call runs as its own task
  and the link keeps serving the window; `StorageClient` became `Clone` for
  that. The link now wakes on a request instead of sleeping out its two-second
  poll, which also makes a mount click prompt.

Verified by tests: the observer's ordering in
`crates/files-operations/tests/observer.rs` (4 tests, including that the
destination file does not exist when `starting` is called); the mapping in
`tracking.rs` (5 tests); the wiring through a real engine and a fake link in
`integration_tests.rs` (6 tests: copy, link and `..` destinations, no device,
move between two devices, failed and cancelled jobs, a failing notice); and the
in-process path in `devicelink.rs`, where a started notice takes a fake-backed
coordinator from Ready to Writing and completion takes it back.

Not done, deliberately or because it is outside this ticket:

- If Better Files exits or crashes while a job is running, nothing sends the
  completion, and the service keeps the device out of Ready until it restarts or
  the device reconnects. The service would have to tie an operation to its
  sender's bus name to clear it; that is a storage-service change.
- With the in-process engine, the completion's flush runs on the link thread,
  so the sidebar's device states pause while a large write is flushed.
- A job started before the link has its first inventory — in the first moments
  after the window opens — is registered nowhere.

Not verified: a copy to a real USB device against a running service. There is
no external device on this machine, and the service's own side of
`NotifyOperationStarted` was already tested over a private bus in ticket 35.
