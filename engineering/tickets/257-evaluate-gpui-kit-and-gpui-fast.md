# Ticket 257 — Evaluate moving to GPUI Kit 0.10 and GPUI Fast

**Blocked by:** none

## Why

Found on 2026-11-21 while sending a GPUI fix upstream. The toolkit Better OS
builds on has moved since the workspace pinned it:

- `longbridge/gpui-component`, pinned here by git revision, is now
  `longbridge/gpui-kit` (0.10, on crates.io), and takes GPUI from the
  `gpui-pre-*` crates on crates.io, weekly snapshots of Zed's GPUI that carry
  no behavioural patches.
- `longbridge/gpui-fast` is a maintained standalone GPUI with retained views,
  scroll layers and native window composition; GPUI Kit can build against it
  through its `gpui-fast` feature. Its README reports 73 to 88% less CPU per
  frame than GPUI on its own showcase.
- Better OS builds GPUI from Zed's repository by git revision, through the
  owner's fork since ADR 0043 for one Wayland fix. The same fix was offered
  to GPUI Fast as longbridge/gpui-fast#173 on 2026-11-21 (the owner's
  decision), because Zed does not take contributions opened by agents and
  zed-industries/zed#61368 already covers the defect there.

The owner's intent is to go back to an upstream once it carries the fix. With
GPUI Fast as that upstream, going back means building on GPUI Kit's current
stack, which is a move of every window.

## What to find out (no product code until the owner decides)

- What the move changes: every API difference between the pinned
  `gpui-component` revision and GPUI Kit 0.10 that `better-ui` and the windows
  use, and between the pinned GPUI revision and `gpui-pre` / `gpui-fast`; the
  size of the work per crate.
- What it buys, measured rather than quoted: build time and disk for a clean
  workspace build from crates.io against today's git checkout of Zed; idle
  and scrolling CPU of Better Files, Better Git and Better Monitor on GPUI
  Fast against today, with the existing benchmark harnesses.
- Whether GPUI Fast's window composition or anything else in it gives rounded
  clipping, which GPUI at the pinned revision lacks (ticket 141's recorded
  limit), and layer-shell surfaces as ticket 200 needs them.
- The licence picture: the two Zed crates with no licence field
  (`engineering/specs/copyleft-review.md`), what the crates.io snapshots
  declare, and what leaves the Corresponding Source obligation ADR 0043
  records.
- The risk: GPUI Fast calls itself experimental; how it tracks upstream, how
  often its API breaks, who maintains it, and what Better OS does if it
  stops.
- An ADR with the options (stay on Zed's GPUI through the fork; GPUI Kit with
  `gpui-pre`; GPUI Kit with GPUI Fast) and the work each needs, for the owner
  to decide.

## Done when

- The ADR is written with measurements from this machine, and the build
  tickets that follow the owner's choice exist.
