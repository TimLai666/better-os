# Ticket 107 — Every D-Bus service answers its first call

**Blocked by:** none · **Status:** merged, not released

## Why

While fixing ticket 102, the worker traced a hang in the Awake tray's tests
to zbus 5.18: adding interfaces to an already-built connection with
`connection.object_server().at(path, iface)` starts the object server
lazily, in a spawned task that subscribes to incoming method calls. A call
that arrives before that task subscribes is dropped with no reply. A zbus
TRACE of a hung run showed the message received and broadcast with no
`dispatch_call` for it (`zbus-5.18.0/src/connection/mod.rs` around line
889). Serving the objects through `zbus::connection::Builder::serve_at`
before `build()`, and requesting the well-known name only after the objects
are served, took the tray's tests from 5 hangs in 30 runs to none in 40.

The same late `object_server().at(...)` pattern is still used, as of
2026-11-14, in:

- `crates/monitor-service/src/main.rs` (around line 47, and ticket 97's
  `start_on_bus` in `crates/monitor-service/src/lib.rs`)
- `crates/manager-daemon/src/main.rs` (around line 54)
- `crates/launcher-platform/src/bus.rs` (around line 171)
- `crates/manager-gui/src/deep_link.rs` (around lines 200 and 221)

A first call lost this way is a D-Bus activation that never answers (the
caller waits for its timeout), a window that shows "service not answering"
once after login, or a flaky test.

## What to build

- Every Better OS process that exports objects on D-Bus serves them through
  `Builder::serve_at` (or otherwise guarantees the object server is running
  before the well-known name is requested), and requests the name last.
  Search the whole workspace for `object_server().at(` and fix every
  occurrence that runs after `build()`, not only the four above.
- Where a process must add an object later (after start), it does so on a
  connection whose object server is already running, and the reason is
  stated in a comment.
- A regression test per service: start it on a private `dbus-daemon`,
  call a method the instant the name appears, repeated many times (for
  example 200 starts), and require every call to be answered.

## What was built

Every place a Better OS process exports objects on D-Bus now has its object
server reading method calls before its well-known name is requested, and the
name is requested last.

- `awake-service` — `service::serve` (new) serves `AwakeDbusService` and asks
  for the name from the builder; `main.rs` calls it. Ticket 102 had already
  moved the code to the builder; it is now one function a test can run.
- `awake-tray` — `sni::serve` (new) serves the item and the menu from the
  builder and attaches the controller; `main.rs` calls it. The tray's names
  are only claimed (activation answers them through the bus), so nothing else
  changed.
- `storage-service` — `service::serve` (new) holds what `main.rs` already did
  in the right order: serve from the builder, start forwarding updates, watch
  departures, then ask for the name.
- `manager-daemon` — `service::serve` (new) serves `ManagerService` and asks
  for `org.betteros.Manager1` from the builder. polkit is asked on a second
  system-bus connection, because the service (which holds the authorizer) has
  to exist before the served connection does; polkit identifies the caller
  from the call's sender, so the asking connection does not matter.
- `monitor-tray` — `serve` (new, in `lib.rs`) serves the item and the menu
  from the builder; `main.rs` then requests `org.betteros.MonitorTray1`.
- `launcher-platform` — `SessionBusRegistry::connect` serves
  `org.freedesktop.Application` from the builder. The inbox is handed out only
  once the name is acquired, as before. New `connect_forward_only` builds a
  registry that serves nothing and refuses to take the name, for a process
  that only forwards; `touchpad-gesture-service` uses it. `touchpad-gui` also
  only forwards and still calls `connect` (it is outside this ticket's
  files); it serves an interface nobody calls, on a unique name nobody is
  told.
- `files-gui` — `filemanager1::serve_on` (new) holds what `serve` already did
  in the right order (merged from main with ticket 93): serve
  `org.freedesktop.FileManager1` from the builder, then ask for the name.
- `monitor-service` — `start_on_bus` keeps its signature (a test and `main`
  pass a connection that is already built) and now first waits for the
  object server: it starts it and pings its own unique name
  (`org.freedesktop.DBus.Peer.Ping`, which only a reading server answers),
  with attempts doubling from 5 ms to 250 ms and a 10 s limit. The store-wait
  path still takes the name before the interface exists, because that is what
  tells a window to let go of the store; the late `object_server().at` there
  is on a server that is already reading, so a call in between gets
  `UnknownObject` at once instead of no reply. The reason is in the comment.
- `manager-gui` — `deep_link::bus::claim` keeps its late `object_server().at`
  (the `ShowPage` channel is made there and `connect` hands out plain
  connections a second window only forwards on); it now waits for the server
  the same way before it requests `io.betteros.Manager`. The reason is in the
  comment.

Late `object_server().at(` left in place: `crates/manager-gui/src/deep_link.rs`
(`claim`, stated above), `crates/monitor-service/src/lib.rs` (store-wait path,
stated above), and the test-only fakes in `crates/manager-gui/src/tests.rs`
and `crates/monitor-tray/tests/tray.rs`, which this ticket did not change.

### Regression tests

`tests/first_call.rs` in `awake-service`, `awake-tray`, `files-gui`,
`launcher-platform`, `manager-daemon`, `monitor-service`, `monitor-tray` and
`storage-service`, and
`src/first_call_tests.rs` in `manager-gui`. Each starts its own
`dbus-daemon` and runs the real start-up function 200 times:

- The services, `files-gui`, `monitor-tray` and `launcher-platform` use a
  private bus
  whose configuration and `.service` files are the test's own: each start
  has a name whose `Exec` only touches a marker file and sleeps a second.
  A client calls the name, the test waits for the marker (the bus is now
  holding the call), starts the service, and the bus delivers the call the
  moment the name is taken. A call counts as lost only if it is not answered
  within 30 seconds of the name being taken, so a slow start under load is
  not reported as a lost call.
- `awake-tray` has no name to activate; a client calls the item and the menu
  on its unique name the moment `serve` returns.
- `manager-gui` calls `ShowPage` the moment `NameOwnerChanged` says the name
  has an owner; a claim that never happens fails the test after 30 seconds
  instead of hanging it.

On one run each test answers 200 of 200. Under load (16 copies of the test at
once) the fixed code answered every call in 16 of 16 runs for the first
eight (`files-gui` was added after the merge with main); with
the serve step temporarily put back to the late `object_server().at` pattern,
every one of the eight lost calls: `awake-tray` 1283 calls across 16 runs,
`monitor-tray` 33, `launcher-platform` 8, `awake-service` 7, `monitor-service`
3 to 8 in every run, `manager-daemon` 6, `storage-service` 4, `manager-gui` 4.
A single unloaded run of the late pattern usually passes, so the tests guard
against the race rather than reproducing it on every run.

## Acceptance

- [x] No `object_server().at(` after `build()` remains without a stated
      reason; list each place changed.
- [x] The regression tests pass 200 out of 200 starts for each service.
- [x] Tests use their own `dbus-daemon` only.
- [x] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
