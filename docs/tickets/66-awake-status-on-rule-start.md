# Ticket 66 — Better Awake's menu shows a session a rule started on its own

**Epic:** Better Awake (Issue #13) · **User Story:** a person whose rule just
started keeping the machine awake sees it in the tray menu without waiting ·
**Branch:** `ticket-66` · **Blocked by:** none · **Status:** implemented, not merged

## What it delivers

Ticket 59 made the service push a status after a tick that ends a session. A
tick that only starts a rule's session still pushes nothing, so the tray menu
lags until the next unrelated change. After this ticket any tick that changes
the set of sessions or the refused rules pushes one `StatusChanged`, and a tick
that changes nothing pushes nothing.

## Acceptance criteria

- [x] A tick that starts a rule session pushes exactly one `StatusChanged`.
      *`a_rule_that_starts_on_a_tick_pushes_one_status` in
      `crates/awake-service/tests/signals.rs`.*
- [x] A tick that changes nothing pushes nothing.
      *The second tick in the same test, with the rule's session still held,
      and in `a_rule_the_battery_holds_back_on_a_tick_pushes_one_status`.*
- [x] A tick that both ends and starts sessions pushes the `SessionEnded`
      events first and one status after them.
      *`a_tick_that_ends_and_starts_sessions_pushes_the_ends_first_and_one_status_after`.*
- [x] Tests run on a private session bus and write only temporary state.

## What was built

### A tick says whether it changed anything

`AwakeEngine::tick` now returns a `TickOutcome`: the sessions it ended, as
before, and a `changed` flag. The flag is true when the tick changed the set of
sessions the service holds or the set of rules it refused. The engine reads
both sets before the tick and again after it, and compares them. Session ids
are never reused, so a session that ends and a new one that starts on the same
tick still count as a change. A tick that ended a session is always a change.

Refused rules are compared by rule id, not by reason. A rule held back by a
flat battery carries the reading in its reason, and the status only shows how
many rules are refused, so a reading that drops from 9% to 8% is not a change
the menu can show.

`service::tick_and_announce` pushes each ended session as `SessionEnded`, then
one `StatusChanged` when the tick changed anything. A tick that changed nothing
pushes nothing. It still returns the ended sessions, so `main.rs` did not
change and still prints a low-battery stop to stderr.

A request answered while a tick runs can make that tick read as a change. That
costs one extra status push, which carries the whole state and is harmless.

### Tests

- Engine: a rule that takes hold on a tick is a change and ends nothing, and
  the tick after it is `TickOutcome::default()`. A rule that the battery holds
  back on a tick is a change, and the next tick is not. A tick that ends a
  session is a change. The existing tick tests now read `.ended`.
- `crates/awake-service/tests/signals.rs`, on a private `dbus-daemon`: a rule
  that takes hold pushes one status listing its session and nothing else, and
  the next tick pushes nothing. A rule the battery holds back pushes one status
  with one refused rule, and the next tick pushes nothing. A manual session
  that expires on the same tick a rule takes hold pushes its `SessionEnded`,
  then one status listing only the rule's session, then nothing.

Before the change, the first two bus tests failed with no status arriving. The
third already passed, because a tick that ended something already pushed one
status. It stays as the guard on the ordering.

The engines in these tests are built with `AwakeEngine::start_in` over a
temporary directory. The real `~/.local/state/better-awake` directory had the
same modification time, 1790489366, and the same single file before and after
both crates' test suites ran.

### Not done, and why

- A tick can change what the status shows in other ways: a lost inhibitor
  raising attention, or the rules being suppressed while no session is held.
  Neither pushes a status yet. The ticket asked for sessions and refused
  rules, and the other fields change on their own schedules.

### Gates

Run from this worktree with its own target directory:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -p awake-service --all-targets --all-features --locked -- -D warnings`:
  clean on stable and on 1.98.0.
- `cargo test -p awake-service --locked`: 58 unit, 1 hermetic, 10 manifest,
  8 private-bus. `cargo test -p awake-tray --locked`: 63 unit, 19 private-bus.
- `Cargo.lock` did not change.
