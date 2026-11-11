# Ticket 64 — A disk is released when the application writing to it goes away

**Epic:** Safe direct-removal storage (Issue #5) · **User Story:** a person
whose Better Files crashed mid-copy is not left with a disk that never becomes
ready to unplug · **Branch:** `ticket-64` · **Blocked by:** none ·
**Status:** implemented, not merged

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

## What was built

The storage service now ends a client's tracked operations when that client
leaves the bus, and Better Files registers a job that started before its link
had a device list.

- **The coordinator knows who started what.**
  `StorageCoordinator::operation_started_by` records the operation under the
  caller's unique bus name, and `client_departed` completes every operation
  that name still holds through the ordinary `operation_completed`, so each
  one gets the same filesystem flush and signal refresh a real completion
  would. `operation_completed` removes the operation from whichever client
  held it, so a client that completed its work has nothing left to end, and a
  completion that arrives after the departure is an ordinary, harmless
  completion. `operation_started`, which the in-process engine calls, records
  no client and is unchanged.
- **The bus side.** `NotifyOperationStarted` reads the sender from the message
  header. After recording the operation it asks the bus `NameHasOwner` for that
  sender and ends its operations at once if the answer is no. This covers a
  client that sent the notice and died before the call reached the
  coordinator's lock, whose departure would otherwise already have been
  handled. A bus that cannot answer is treated as the client still being
  there, which never claims readiness early.
- **`service::watch_departures`** subscribes to `NameOwnerChanged` for names
  losing their owner (`arg2=''`), and returns the task that calls
  `client_departed` for each unique name that leaves. The subscription is in
  place when it returns. `main.rs` serves the object, starts this task, and only
  then takes `org.betteros.Storage1`, so no client can reach the service before
  it is listening for departures. The task runs on the runtime `main` spawns
  it on; no handler spawns anything.
- **`DeviceLink::mounted_devices` returns `Option`.** `None` means the link has
  no device list yet, which is now different from a list with nothing mounted.
  `StorageLink` fills it on its first inventory even when that inventory is
  empty (`publish_inventory`). `NoDeviceLink` answers `Some` of an empty list.
- **An early job is registered when the list arrives.** When
  `StorageTracker::starting` finds no list, the job goes ahead without waiting
  and a thread of its own checks every 50 ms until the list arrives, the link
  reports `Unavailable`, or the job finishes. If the job is still running, it
  is announced for the devices it writes to at that moment. The thread holds
  the job's registration lock while it sends the started notices, and
  `finished` takes the same lock, so a completion is never queued before its
  start.
- **A completion goes through the link that announced the job.** Before this,
  `finished` used whichever link was attached last. With two windows open,
  that sent the completion over the second window's connection, or to a
  different backend altogether. Each registration now keeps its link, which
  also keeps that link's connection open until the job is released, so closing
  the first window no longer drops a connection the service would read as the
  job's end.

Verified by tests. On the service side, over a private `dbus-daemon`:
`a_client_that_disconnects_mid_write_does_not_hold_the_device_forever` in
`crates/storage-service/tests/dbus_service.rs`. A client mounts, starts an
operation, and closes its connection without completing it. The device goes
from Writing back to Ready to unplug, and a later completion for the same
operation from a new connection is answered without error. Without the
coordinator change the device stayed Writing for the whole five seconds. In
`crates/storage-service/tests/coordinator.rs`, three tests cover two clients
where only the departed one's operations end, one flush per ended operation,
a late completion, a client that completed before leaving, and an in-process
operation that no departure touches. On the Better Files side, in
`crates/files-gui/src/integration_tests.rs`, a job parked on a conflict before
the first list is registered once the list arrives and released when it is
cancelled. A job that finishes before the list arrives sends nothing. A job
announced through one link is released through that link after a second link
is attached. `devicelink.rs` checks that an empty first inventory still counts
as the first one.

Not done, deliberately or because it is outside this ticket:

- The `NameHasOwner` check has no deterministic test. A test that sends the
  notice without waiting for the reply and closes at once passed with the
  check disabled as well, so it did not show that the check is needed and was
  not kept.
- An operation started over a peer-to-peer connection, which has no sender,
  is recorded without a client and is never ended by a departure. The service
  only serves on the session bus, so this does not happen today.
- The early-job thread polls. It exists only while a job that started before
  the first list is still running, and stops when the list arrives. A link
  that stays in Connecting keeps it polling until the job ends.
- With the in-process engine, the completion's flush still runs on the link
  thread. Ticket 51 already records this.

Not verified: a real Better Files crash mid-copy to a real USB device against
a running service. This machine has no external device. The departure path
was exercised only through the private-bus test above.
