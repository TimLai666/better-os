# Ticket 59 — Better Awake tells the person when low battery ends a session

**Epic:** Better Awake (Issue #13) · **User Story:** a person whose keep-awake
session was stopped by low battery learns it from a desktop notification ·
**Branch:** `ticket-59` · **Blocked by:** none · **Status:** implemented, not merged

## What it delivers

A low-battery stop writes history and prints to stderr. The service emits no
signal for a stop made by its own tick, and the tray drops `SessionEnded`. After
this ticket the service emits `SessionEnded` with its cause for every session it
ends on its own, and the tray raises one desktop notification through
`org.freedesktop.Notifications` for a low-battery stop.

## Acceptance criteria

- [x] Every session the service ends without a client request emits
      `SessionEnded` with its cause, once.
      *Asserted in `awake-service`'s engine tests and over a private session
      bus in `crates/awake-service/tests/signals.rs`.*
- [x] The tray raises one notification per low-battery stop, in zh-TW or
      English following the tray's existing label selection, naming the
      battery threshold that was crossed.
- [x] No notification daemon, or a failed call, is logged and does not stop
      the tray.
- [x] Tests use a private session bus or a fake notifier. No test raises a
      notification on the developer's desktop.
- [x] The AGENTS.md follow-up about the unwired notification can be removed.
      *It is stale now. This branch does not edit AGENTS.md, so removing the
      entry is left to the merge.*

## What was built

### The service says when it ends a session

The engine now reports every session it ends, as an `EndedSession`: the
session id, the `EndCause`, and the battery stop threshold the session carried.
The threshold is read before the state machine runs the command, because once
it reports a session ended it no longer holds it. Each report comes out of one
state transition, which is what makes "once" true without the service keeping
a list of ids it has already announced. The `announced` list in `main.rs` is
gone for that reason.

`AwakeEngine::tick` and `AwakeEngine::shutdown` return what they ended. The
service pushes each one as `SessionEnded` and then pushes a `StatusChanged`.
Before this change a tick pushed nothing, so a session that expired or hit its
battery threshold stayed in every open tray menu until the next request. A
tick that ended nothing still pushes nothing. `service::tick_and_announce` and
`service::shutdown_and_announce` are the functions `main.rs` calls, so the
private-bus test drives the same path the binary runs.

A request path can end sessions too. Ending a session, pausing the rules, and
editing a rule so it no longer matches are what the client asked for, and that
client gets its answer and the `StatusChanged` as before. A low-battery stop is
the exception. A rule edit re-reads every provider, so it can find a flat
battery and stop a session nobody asked about. `AwakeEngine::handle_reporting`
returns those stops and the D-Bus handler announces them.

`SessionEnded` travels on the existing `StatusChanged` D-Bus signal, which
already carries whole event documents, and arrives before the status that no
longer lists the session. It gained two optional fields,
`battery_stop_percent` (the threshold) and `battery_percent` (the reading that
crossed it). Both are omitted when empty and default when absent, so a document
without them still reads. The protocol version did not change.

### A rule on a flat battery no longer stops every tick

Writing the "once" test turned up a loop. A rule that still matched after
protection stopped its session was given a new session on the next tick, and
protection stopped that one too: with a charger plugged in and the battery at
9%, three ticks wrote three history entries. Each would have become a
notification. `a_rule_stopped_by_a_flat_battery_is_not_restarted_and_stopped_again_every_tick`
failed with 3 stops before the fix.

A rule with no session whose threshold is above the current reading is now not
started. It is counted as a refused rule with the key
`awake.battery.below_stop_threshold:<percent>`, so the Automatic Rules page
shows it among the rules that match and could not be given a session. A rule
that already holds a session is left to protection, which runs after the rules
as before. That keeps a threshold raised above the reading by an edit a battery
stop rather than a `trigger_cleared` end.

### The tray raises the notification

`awake-tray::notify` holds a hand-written proxy for `Notify` on
`org.freedesktop.Notifications`, on the session connection the tray already
has. No notification crate was added and `Cargo.lock` did not change.
`notify-rust` was not used: it declares Rust 1.89 against the workspace's 1.85,
and it is in the lockfile only because `gpui_linux` depends on it.

The notification is worded from the tray's `Labels`, so it follows the same
`Locale::from_environment` choice as the menu. The summary names the
threshold ("Stopped keeping awake: battery below 20%", "電量低於 20%，已停止保持清醒")
and the body names the reading. The icon is the standard `battery-caution`
name, because Better Awake still ships no artwork.

The tray's signal loop now calls `notify::handle_event`. It returns a status
for the menu and raises the notification for a low-battery stop. Nothing that
goes wrong there reaches the loop. A missing notification service, a refusal,
or a call that gets no answer within five seconds is written to stderr, and the
next event is handled as usual. The timeout exists because the call is made
from the loop that keeps the menu following the service. The service still
prints the stop to stderr, so a stop is visible when no tray is running.

### Tests

- Engine: expiry, a low-battery stop, and a rule that stops matching are each
  reported once from a tick, with the threshold. A client's own end is not
  reported. A battery stop found while answering a rule edit is. A raised
  threshold ends the rule's session as a battery stop. Shutdown reports what it
  ended.
- `crates/awake-service/tests/signals.rs`, on a private `dbus-daemon`: the
  order `SessionEnded` then `StatusChanged` from a tick, silence from a tick
  that ended nothing, an expiry, a battery stop found during a request, a plain
  end request that pushes no `SessionEnded`, and shutdown.
- `crates/awake-tray/tests/session_bus.rs`, against a fake notification service
  on a private bus under the test-only name `org.betteros.NotificationsTest`:
  one notification per stop with the `susssasa{sv}i` signature the
  specification defines, both locales, no notification for any other cause, no
  service at all, a refusal, a service that never answers, and a tray with no
  notifier.

Nothing here can reach the developer's notification daemon. The bus belongs to
the test, the fake answers to a name no real daemon uses, and the default name
is never called in a test.

### Tests no longer touch the real state directory

The tray's bus tests built their service with `AwakeEngine::start`, and two
engine unit tests restarted the service the same way. `start` keeps rules and
history under `$XDG_STATE_HOME/better-awake`, or `~/.local/state/better-awake`
when that is unset, and reads this machine's `/proc` and `/sys`. Every tray bus
run added entries to the developer's real History and ran against the rules
they had written.

`AwakeEngine::start_in(backend, directory, clock)` now puts the state, rules,
and history files in one directory, under the names the service uses, and
points the providers at a `/proc` and `/sys` tree inside it. An empty directory
is a machine with no battery, no charger, and no rules, so the tray tests also
stop depending on the battery of the machine they run on. It exists only under
`cfg(test)` and the `test-support` feature, which the shipped binary never
enables. The tray bus tests and `signals.rs` use it, and the two restart tests
reopen the fixture's own files. No test sets `XDG_STATE_HOME`: tests run in
parallel threads of one process, so an environment variable set by one test
would be seen by the others.

`crates/awake-service/tests/hermetic.rs` reads the test code of `awake-service`
and `awake-tray` and fails on `AwakeEngine::start(`, `from_default_path(`, or
`Roots::system(`. Before the fix it named `engine.rs` and `session_bus.rs`.
`the_test_service_keeps_its_history_in_its_own_directory` checks that a session
the tray test ran is recorded in the test's directory. The real history file's
modification time was 1790409060 before and after both crates' test suites
ran.

### Not done, and why

- The notification has not been seen on a real desktop. Every test uses the
  fake service, so how GNOME draws it, and whether it groups it under Better
  Awake, is unverified. No `desktop-entry` hint is sent.
- A tick that only *starts* a rule's session still pushes no `StatusChanged`,
  so a menu can miss a rule taking hold until the next request. This ticket
  covered sessions ending; starting is the same kind of gap.
- The AGENTS.md follow-up is not removed on this branch.
- The history entries the old tray tests wrote into the real
  `~/.local/state/better-awake/awake-history.json` before this branch fixed
  them are still in that file. Nothing here deletes a person's History, test
  entries included; they carry the tests' fixed clock, 1700000000, and the
  reason "Started from the tray".

### Gates

Run from this worktree with a shared `CARGO_TARGET_DIR`, crate-scoped:

- `cargo fmt --all -- --check`: clean.
- `cargo check -p awake-ipc -p awake-service -p awake-tray --all-targets`:
  clean. `cargo check -p awake-gui`: clean.
- `cargo clippy -p awake-ipc -p awake-service -p awake-tray --all-targets -- -D warnings`:
  clean.
- `cargo test -p awake-ipc`: 33 passed. `cargo test -p awake-service`: 55 unit,
  1 hermetic, 10 manifest, 5 private-bus. `cargo test -p awake-tray`: 63 unit,
  19 private-bus. The bus tests ran against a real `dbus-daemon` rather than
  skipping.
