# Ticket 115 — Better Monitor records slowdowns by itself

**Blocked by:** none · **Status:** open

## Why

The owner decided on 2026-11-14: the slowdown record ("變慢記錄") should be
written automatically, and the person should not be able to add one by hand.
A manual "剛剛變慢了" (Just slowed down) button can be pressed by accident,
and a person presses it after the fact, so the moment it marks is late; the
service sees the slowdown as it happens.

Today an incident exists only when someone presses the button in the window
(`crates/monitor-gui/src/render.rs` ~248, `app.rs` `mark_incident`) or runs
`better-monitor mark` (`crates/monitor-cli/src/lib.rs` ~343), through the
`MarkIncident` request (`crates/monitor-ipc/src/lib.rs` ~115). The service
already samples what a detector needs, including pressure stall information
(`crates/monitor-service/src/inventory.rs`, `crates/monitor-store/src/sample.rs`,
`incident.rs`), and the window already words the state as "Your computer is
slowing down: programs are waiting for …" (`monitor-gui/src/i18n.rs` ~1150).

## What to build

- The service detects a slowdown itself and records an incident with the
  samples around it, with no window open: sustained pressure (CPU, memory or
  I/O "some"/"full" stall above a threshold for several seconds), memory
  nearly exhausted with swap activity, an OOM kill, or thermal throttling,
  using signals the service already reads or can read without privileges.
  Thresholds have hysteresis: one incident per slowdown, extended while it
  lasts, with a quiet period before a new one; a brief spike is not one.
- Each incident says in plain words what it was (which resource programs were
  waiting for, how long, how bad) and which processes were using the most of
  that resource at the time.
- The manual button and its intro copy are removed from the window, the tray
  and any menu; the Events page says the records are made automatically and
  what counts as a slowdown. `better-monitor mark` is removed too, and the
  `MarkIncident` request is no longer accepted from clients (an older window
  that sends it gets a clear refusal, not a crash). Records already made by
  hand are kept and shown, labelled as marked by hand.
- Storage stays bounded: the existing retention applies, plus a cap on
  incidents per day so a machine that is always under pressure does not fill
  the store.
- The detector costs nothing measurable: it runs on the samples the service
  already takes.
- English and Traditional Chinese (Taiwan usage).

## Acceptance

- [ ] Unit tests on synthetic sample streams: sustained pressure makes one
      incident; a spike makes none; a long slowdown is one incident, not many;
      two slowdowns separated by quiet make two; the daily cap holds; each
      trigger kind is named correctly.
- [ ] An integration test with the service on a private bus and fake
      readings: an incident appears without any client asking; `MarkIncident`
      is refused.
- [ ] Screenshots of the Events page with automatic records and with none, in
      both languages, from a headless sandbox with a fixture store.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
