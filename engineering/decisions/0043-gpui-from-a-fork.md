# ADR 0043: Better OS builds GPUI from its own fork of Zed

## Status

Accepted by the owner on 2026-11-21 (ticket 200), for exactly what is
written under "Decision". A change the fork would carry that does not meet
the rule there needs its own record.

## Context

Every Better OS window is GPUI, taken from Zed's repository at a pinned
commit (`ae394f3` when this was written; ADR 0020 chose the repository over
crates.io, and this record changes only which repository serves Zed's
crates; `gpui-component` still comes from its own), and the session's surfaces open
GPUI windows as layer-shell surfaces (ADR 0038, ticket 200). Ticket 200 found
that at that commit a layer surface that fills an axis (the protocol's size
0: the top bar, the taskbar, the desktop) is disconnected by a
`wp_viewport.set_destination` protocol error whenever the compositor sends
its fractional scale before the surface's first configure, which
`cosmic-comp` does on every output above 100 %. The fix is a few lines in
`gpui_linux`'s `wayland/window.rs`. Zed's `main` still has the defect on
2026-11-21; an upstream pull request with a wider fix, zed-industries/zed#61368,
has been open since 2026-07-20, and a narrower one, #62432, was closed by its
author unmerged.

Three ways were weighed in ticket 200: carry the fix in a fork (a), wait for
upstream and then move the pin (b), or avoid size 0 in Better OS's own code by
learning each output's logical size over a second Wayland connection (c).
(b) leaves the session's bars unbuildable on HiDPI for an unknown time and a
pin move drags in every upstream change since `ae394f3` against the pinned
`gpui-component`; (c) duplicates what the compositor already does and breaks
on every mode change.

## Decision

1. **Where GPUI comes from.** The fork is `https://github.com/TimLai666/zed`,
   the owner's GitHub fork of `zed-industries/zed`. Its branch
   `better-os/gpui-<upstream short revision>` is the pinned upstream revision
   plus Better OS's commits; a tag `better-os/gpui-<upstream short revision>-<n>`
   marks every head the workspace has built, and is never moved or deleted.
   The workspace keeps its dependencies written against
   `https://github.com/zed-industries/zed` and redirects every crate from that
   source with one `[patch."https://github.com/zed-industries/zed"]` section
   naming the fork and the exact revision. A patch section, not a changed URL,
   because `gpui-component` depends on Zed's repository itself: a changed URL
   in the workspace would leave its copy on upstream and build two GPUIs.
2. **What the fork may carry.** Only fixes that are also offered upstream:
   each one is its own commit on top of the pinned revision, the same change
   is offered to Zed as a pull request (or is already an open upstream pull
   request, named in the commit), and its commit message says which. Nothing
   else: no feature, no refactoring, no Better OS-only behaviour. A larger
   change, for example clipping a window to rounded corners for ticket 141's
   recorded limit, is not covered by this record and needs its own decision.
3. **The first commit**, from ticket 200: `gpui_linux` sends
   `wp_viewport.set_destination(-1, -1)` (unset) rather than a zero
   dimension while a layer surface waits for its compositor-chosen size, with
   a unit test, and the `NOTICE`-style line Apache-2.0 section 4(b) asks of a
   modified file (`gpui_linux` is Apache-2.0).
4. **Moving the pin.** A GPUI update picks a new upstream revision, drops every
   fork commit upstream now contains (merged, or fixed differently), rebases
   the rest onto the new revision as a new branch
   `better-os/gpui-<new short revision>`, tags it, and moves the patch
   section's revision in the same Better OS change that updates the lockfile.
   The old branch and its tags stay, because published packages were built
   from them (Corresponding Source, below). When the fork carries nothing, the
   patch section is removed and the workspace builds upstream again.
5. **Who pushes.** The owner's GitHub account owns the fork; the owner and
   agents working for the owner with the owner's credentials push to it, only
   to `better-os/*` branches and tags, never force-pushing a branch or a tag
   any published build used. Never a local path dependency: a build must be
   reproducible from the repository and the fork alone.

## Consequences

- `Cargo.lock` names the fork as the source of all 22 crates the workspace
  takes from Zed. CI and `packaging/build-deb.sh` fetch it like any git
  dependency; nothing else changes for them.
- **Corresponding Source.** The windows are conveyed under GPL-3.0-or-later
  (`engineering/specs/copyleft-review.md`), so their Corresponding Source now
  includes the modified `gpui_linux`. It is offered where the rest of the
  dependency source is: on a third party's server, the fork, at the exact
  revision `Cargo.lock` and `THIRD-PARTY-LICENSES.md` name. GPL-3.0 section
  6(d) keeps Better OS responsible for its staying available; the tags in
  Decision 1 are how. Apache-2.0 section 4(b) asks that the modified file say
  it was changed; the fork's commit adds that line to `window.rs`, which is
  also why the fork commit and the upstream pull request differ by that line.
- The fork's single commit is a few lines; dropping it is a no-op once
  upstream has the fix. The cost is the discipline above, not code.
- Zed's contribution rules (read 2026-11-21) require the contributor to sign
  Zed's CLA, refuse contributions from autonomous agents, and ask that pull
  request descriptions be the contributor's own words. The upstream offer is
  therefore the owner's to make, by hand; ticket 200's notes say what was
  prepared.
