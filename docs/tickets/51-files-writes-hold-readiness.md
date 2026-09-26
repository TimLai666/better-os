# Ticket 51 — Better Files' own writes to an external disk hold its readiness

**Epic:** Safe direct-removal storage (Issue #5) · **User Story:** a person
copying to a USB disk with Better Files is not told the disk is safe to remove
while the copy is still running · **Branch:** `ticket-50-52` ·
**Blocked by:** none · **Status:** ready

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
