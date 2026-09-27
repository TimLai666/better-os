# Ticket 64 — A disk is released when the application writing to it goes away

**Epic:** Safe direct-removal storage (Issue #5) · **User Story:** a person
whose Better Files crashed mid-copy is not left with a disk that never becomes
ready to unplug · **Branch:** `ticket-64` · **Blocked by:** none ·
**Status:** ready

## What it delivers

A Better Files job registers a tracked operation with the storage service and
completes it when the job ends. If Better Files exits or crashes in between, the
completion is never sent and the service holds the device out of "ready to
unplug" until it restarts or the device is replugged. Separately, a job started
before Better Files' device link has its first device list is not registered
at all. After this ticket the service ties each tracked operation to the bus
name that started it and ends it when that name leaves the bus, and a job that
starts before the first device list is registered once the list arrives.

## Acceptance criteria

- [ ] An operation started over the bus records its sender's unique name. When
      that name leaves the bus, every operation it started is completed and
      the device's readiness is re-evaluated, with a flush as a normal
      completion would do.
- [ ] A completion for an operation the service already ended this way is
      accepted without error.
- [ ] The in-process engine, which has no bus, keeps its current behaviour.
- [ ] A Better Files job running when the first device list arrives is
      registered for the devices it writes to at that moment.
- [ ] Tests run on a private session bus: a client starts an operation and
      disconnects without completing, and the device becomes ready.
