# Ticket 257 — Upgrade GPUI Kit to its current release, on the GPUI that suits Better OS

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

## Owner's decision (2026-11-21)

GPUI Kit is upgraded to its latest release. This is an upgrade of the
component library Better OS already uses (it was called `gpui-component` when
the workspace pinned it), not a change of toolkit. Which GPUI it sits on is
chosen from the measurements below: `gpui-pre` (Zed's GPUI, unchanged, from
crates.io) or GPUI Fast (longbridge's GPUI with retained views).

The owner's preference, stated the same day, is GPUI Fast. So GPUI Fast is
the choice unless the measurements below find something that rules it out (a
window that cannot be made to work on it, a defect with no fix in reach); in
that case the finding goes back to the owner rather than being worked
around. A component library always needs one of the two underneath it:
GPUI Kit draws nothing by itself.

One fact weighs on that choice: Better OS's layer-shell surfaces need the
viewport fix of ADR 0043. `gpui-pre` carries no behavioural patches, so it
has the defect until Zed fixes it; GPUI Fast has it once
longbridge/gpui-fast#173 is merged. Whichever is chosen must have the fix, or
the fork of ADR 0043 stays.

## What to build

- First the measurements and facts in the next section, written into an ADR
  that records GPUI Fast as the owner's choice with what was measured, or
  takes a finding that rules it out back to the owner.
- Then the upgrade itself, in one branch after the open pull requests have
  landed: `better-ui` and every window on GPUI Kit's current release, the
  workspace's GPUI from the chosen source, ADR 0043's fork dropped if the
  chosen source has the fix, the licence review and inventory redone, the
  packaging and CI caches adjusted, and every window looked at in light and
  dark, zh-TW and English, against screenshots taken before the upgrade.

## What to find out first

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
- An ADR with the two GPUI choices under the upgraded GPUI Kit (`gpui-pre`;
  GPUI Fast) and the work each needs, with a recommendation.

## Done when

- The ADR is written with measurements from this machine.
- Every window builds and runs on the upgraded GPUI Kit, with the workspace
  gate and CI green, and before-and-after screenshots that show no window
  lost anything.

## Notes

**First part done (2026-11-21): the facts and the ADR.**
[ADR 0045](/engineering/decisions/0045-gpui-kit-on-gpui-fast.md) records
the owner's decision with what was measured on this machine. Nothing found
rules GPUI Fast out. Evidence and logs:
`/home/tim/Developer/better-os-evidence/257/`.

- The API distance was sized on the branch
  `scratch/257-gpui-kit-0.10-on-gpui-fast` (one commit, not for merging):
  GPUI Kit 0.10 on GPUI Fast 0.1.17 from crates.io, the Zed patch section
  gone. Every crate checks (`--workspace --all-targets`) except `git-gui`,
  whose multi-line fields must become `TextareaState`. The edits to get
  there are about twenty lines, listed in the ADR.
- ADR 0043's viewport defect is in GPUI Fast 0.1.17 and was reproduced on
  sway at scale 2; longbridge/gpui-fast#173's diff on the published
  `gpui-fast-linux`, given as `[patch.crates-io]`, fixes it.
- GPUI Fast clips to rounded corners (`overflow_hidden` with `rounded`),
  paint only; removed outputs leave the display list in GPUI Fast and
  `gpui-pre` alike.

**What the upgrade part must do** (the ADR's plan, in order): screenshots of
every window before (light, dark, zh-TW, English); the workspace manifest on
the latest GPUI Kit with `gpui-fast`, exact versions, the Zed dependency and
patch section removed, #173 as a `[patch.crates-io]` of `gpui-fast-linux`
from the owner's fork unless a GPUI Fast release has it (the owner's call,
ADR 0045's open questions), lockfile and `docs/third-party-licenses.md` in
the same commit; `better-ui`, then the windows smallest first and `git-gui`'s
text areas, each audited for render state read outside entities (retained
views) and compared with `GPUI_VIEW_RETENTION=0` where anything looks stale;
`session-layer-facts.sh` on `cosmic-comp` at 150 % and 200 % and the
`outputs` case on sway; the copyleft review rewritten (no GPL crate from Zed
is left; Decision 4 settled; Corresponding Source for crates.io sources and
the patch); CI caches and `packaging/build-deb.sh` for all four targets;
the workspace gate, CI green, both benchmark suites against ADR 0045's
numbers, screenshots after compared one by one; ADR 0020, ADR 0043,
`architecture.md` and AGENTS.md's toolkit follow-ups brought up to date
(including the HTTP client note, which ADR 0045 found is a lockfile
over-approximation: no HTTP client is linked into a Linux window today
either).
