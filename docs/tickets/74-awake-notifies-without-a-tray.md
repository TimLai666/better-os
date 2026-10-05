# Ticket 74 — Better Awake tells the person about a low-battery stop even with no tray

**Epic:** Better Awake (Issue #13) · **User Story:** a person who does not run
the tray still learns that low battery ended their session · **Branch:**
`ticket-74` · **Blocked by:** 66 (same service code, merged) · **Status:** implemented, not merged

## What it delivers

The tray raises the low-battery notification, so with no tray running a stop is
only in History and on stderr. The owner decided on 2026-09-27 that the service
raises it itself when no tray is there to do it, and exactly one notification
is shown either way.

## Acceptance criteria

- [x] The service knows whether a tray is running (for example by the tray
      owning a well-known bus name) without polling.
      *The tray owns `org.betteros.AwakeTray1`; the service asks the bus
      `NameHasOwner` once, when it has a stop to report.*
- [x] With a tray, the tray notifies and the service does not; without one, the
      service does; never both, never neither.
      *`with_a_tray_running_only_the_tray_raises_the_low_battery_notification`
      and `without_a_tray_the_service_raises_the_low_battery_notification` in
      `crates/awake-tray/tests/session_bus.rs`. Two short races remain; see
      below.*
- [x] A notification failure is logged and changes nothing else.
      *`no_notification_service_changes_nothing_but_the_notification` and
      `a_notification_service_that_never_answers_does_not_hold_up_the_tick` in
      `crates/awake-service/tests/signals.rs`.*
- [x] Tests use a private session bus with a fake notification service and
      write only temporary state.

## What was built

### The tray says it is there by owning a name

A tray that will raise the notification owns `org.betteros.AwakeTray1` on the
session bus. The name follows the `org.betteros.<Name>1` pattern the other
services use, and nothing else in the repository used it. The bus drops the
name when the tray's connection closes, so a tray that quits or crashes hands
the notification back without having to say so.

The tray takes the name only after it is subscribed to the service's events,
and on the same connection those events arrive on. From the moment the name is
owned the service stops notifying, so a tray that owned it before it could hear
a stop would let that stop pass with no notification. The tray binary used to
follow the service on a second connection of its own. It now uses the one it
already had, so the name and the subscription live and die together.

`awake_tray::notify::claim_notifications` takes the name. It does not take it
when the tray has no notifier, because a tray that cannot notify must leave the
service to do it. It asks without replacing and without queueing, so a second
tray started while one is running is refused the name and does not notify;
two trays notifying would be two notifications for one stop. The first tray
keeps the name until it goes. The second does not take over then, and the
service notifies instead.

### The service asks when it has something to say

When the service ends a session for low battery, it asks the bus once whether
anyone owns the tray's name, through `NameHasOwner`. Nothing is polled and no
state about the tray is kept between stops. If nobody owns the name, the service
raises the notification itself. If the bus cannot answer, the service counts
that as no tray. If a tray was there after all, that is two notifications for
one stop, which is the better way to be wrong.

The bus is asked before `SessionEnded` goes out. A tray that takes its name
between the question and the signal is already subscribed, so it hears the stop
and notifies too. That race ends in two notifications, not none.

This covers both places the service finds a stop: a tick, and a rule edit
answered by the D-Bus handler. The handler case matters because zbus polls a
handler on its own executor, where `tokio::spawn` and tokio's timer are not
available. `LowBatteryNotifier` captures a `tokio::runtime::Handle` when the
service starts and raises the notification on a task spawned through it, so
the handler never waits for the notification service. A notification service
that never answers is given up on after five seconds without holding up the
reply or the tick. A failure is written to stderr. Everything else already
happened by then: the session has ended, been recorded in History, and been
announced on the bus.

### One wording for both

The wording, the locale rule, the icon name, and the tray's bus name moved to
a new `awake_ipc::notification` module, which both processes already depend on.
It does not touch a bus. The tray's `Locale` is now that type, re-exported from
`labels`. The tray reads its own labels through a `LocaleLabels` trait, because
the type now lives in another crate. The tray's application name in its menu is
the same string the notification is shown under.

The `Notify` proxy itself could not move. `awake-ipc` does not depend on zbus,
and adding that dependency would have changed `Cargo.lock`. The service has its
own copy of the proxy declaration, about twenty lines that describe the
freedesktop interface. Neither crate depends on the other, and `Cargo.lock` did
not change.

The service reads its locale with the same rule as the tray, from `LC_ALL`,
`LC_MESSAGES`, or `LANG` in its own environment.

The manifest's `session-bus-name` permission now names the tray's bus name as
well as the service's. A manifest test checks that it does.

### Tests

- `crates/awake-service/tests/signals.rs`, on a private `dbus-daemon` with a
  fake notification service under `org.betteros.NotificationsTest`:
  - With no tray, a tick's stop is notified once by the service, in the
    expected wording, and `SessionEnded` and the status still go out in order.
  - With a connection owning the tray's name, the service raises nothing.
  - A tray that quit no longer stops the service from notifying.
  - A stop found while answering a rule edit is notified, and the request is
    still answered.
  - With no notification service at all, the tick still ends the session and
    pushes `SessionEnded` and the status.
  - A notification service that never answers does not hold up the tick.
- `crates/awake-tray/tests/session_bus.rs`, with the real service and a tray
  loop built the way `main.rs` builds it:
  - With a tray, exactly one notification arrives and the tray sent it.
  - Without a tray, exactly one arrives and the service sent it, under the
    tray's application name.
  - A second tray is refused the name.
  - A tray with no notifier does not take it.
- `crates/awake-service/tests/manifest.rs`: the tray's bus name is named in the
  manifest.
- The locale and wording tests moved from the tray to `awake-ipc` with the
  code they test.

Before the change, the bus tests did not compile, because the tray's name, the
service's notifier, and `claim_notifications` did not exist. The manifest test
failed. To check that the two exactly-once tests can fail, the service's tray
check was changed by hand to always answer "no tray". Then
`with_a_tray_on_the_bus_the_service_leaves_the_notification_to_it` and
`with_a_tray_running_only_the_tray_raises_the_low_battery_notification` both
failed, and passed again once the check was restored.

Every engine is built with `AwakeEngine::start_in` over a temporary directory.
The real `~/.local/state/better-awake` directory had the same modification
time, 1790489366, and the same single file before and after the test suites
ran.

### Not done, and why

- Two races remain, each about as wide as one round trip to the bus. A stop
  found while a tray is taking its name can be notified twice. A stop found
  while a tray is quitting can go un-notified: the service saw the name, and
  the tray stopped listening before the signal reached it.
- The notification raised by the service has not been seen on a real desktop.
  The service runs as a systemd user unit, so its locale is the user manager's
  environment, which may not be the desktop session's. A person whose desktop
  is in zh-TW but whose user manager has no `LANG` would get the English
  wording from the service and the zh-TW wording from the tray.
- The AGENTS.md entry saying that a stop with no tray running is only in
  History and on stderr is stale now. This branch does not edit AGENTS.md, so
  removing the entry is left to the merge.

### Gates

Run from this worktree with its own target directory:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -p <crate> --all-targets --all-features --locked -- -D warnings`
  for `awake-ipc`, `awake-service`, and `awake-tray`: clean on stable and on
  1.98.0.
- `cargo test -p awake-ipc --locked`: 37 unit. `cargo test -p awake-service
  --locked`: 58 unit, 1 hermetic, 11 manifest, 14 private-bus.
  `cargo test -p awake-tray --locked`: 59 unit, 23 private-bus.
  `cargo test -p manager-core --locked`, which embeds the manifest: 29 unit,
  50 lifecycle, 1 ignored as before. `cargo check -p awake-gui --all-targets`:
  clean.
- `Cargo.lock` did not change.
