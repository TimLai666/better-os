# Ticket 107 — Every D-Bus service answers its first call

**Blocked by:** none · **Status:** ready

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

## Acceptance

- [ ] No `object_server().at(` after `build()` remains without a stated
      reason; list each place changed.
- [ ] The regression tests pass 200 out of 200 starts for each service.
- [ ] Tests use their own `dbus-daemon` only.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
