# Ticket 202 — Prototype the gesture patch to `cosmic-comp` and measure its latency

**Spec:** better-os-session.md S6 ("Gestures": the contract and the budget)
**Blocked by:** none

## Why

The owner's requirement that every gesture follows the fingers is met in the
session only through one patch to `cosmic-comp` (C8). C8 ships it only once
the rest of the session works, but whether it is feasible, how large it is,
and what latency it adds are unknown (ADR 0038: "`cosmic-comp` with a gesture
patch" not run), and every surface that follows the fingers is designed
around the answer. So the prototype is built now and not shipped.

## What to build

- **The protocol XML** `betteros-gesture-unstable-v1` exactly as the spec's
  contract (claims for swipes and pinches, begin, update, pinch update, end,
  hold, release; the workspace driver), in `compositor/cosmic-comp/protocols/`
  under `GPL-3.0-or-later`.
- **The patch** against the `cosmic-comp` commit of the newest Epoch release,
  in `compositor/cosmic-comp/patches/` (`GPL-3.0-only`, ADR 0039's layout even
  before it is accepted; moved if the owner chooses otherwise): the new module
  under `src/wayland/protocols/` and its handler, the swipe and pinch arms of
  `src/input/mod.rs` routing a claimed kind and count to the claim, the
  workspace driver calling `update_workspace_delta` and `end_workspace_swipe`,
  the executable check and the security-context check, release on disconnect.
  Count its lines.
- **A test client** in Rust that claims four-finger swipes and pinches,
  drives the workspace switch from them, and draws a GPUI layer surface that
  follows `progress` (ticket 200's helper if it has landed, else a plain
  surface).
- **A replayed touchpad**: a libinput recording of swipes and pinches written
  for the test (or recorded by the owner on the laptop, which needs root and
  is the owner's to do), replayed in ticket 201's VM through `libinput replay`.
- **Measurements**, in the VM with software rendering and, if the owner
  agrees, once in a VM with the laptop's touchpad passed through: for each
  event, compositor input time to claim event delivered, delivered to the
  surface's commit, and the workspace animation's frame times while driven,
  p50 and p95 over at least 1,000 updates; the same for the released
  compositor's own four-finger switch as the baseline.

## Done when

- [ ] The patch applies to the newest Epoch tag and builds; its size is in
      Notes.
- [ ] Protocol tests in the patched tree: a claim routes only its kind and
      count, a second claim is refused, a client with another executable is
      refused, disconnect mid-gesture returns the compositor's behaviour with
      `cancelled`.
- [ ] The measurement table in Notes and in
      `engineering/performance/session-gestures.md`, with the hardware and the
      rendering stated, against the one-frame budget (spec, S5).
- [ ] If the budget cannot be met or the patch cannot be kept to input routing,
      the ticket says so and the owner is asked about C8 before ticket 223 or
      any finger-following surface work starts.
- [ ] No patched compositor installed anywhere but the VM; the VM deleted
      after.
