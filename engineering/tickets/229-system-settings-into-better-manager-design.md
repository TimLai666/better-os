# Ticket 229 — Display, network, Bluetooth, sound, users, and region settings in Better Manager: design

**Spec:** better-os-session.md, Not doing (C9)
**Blocked by:** none for the design; building waits on the owner's answers
**Blocked:** the owner's answers to M1 to M9 at the top of `design/system-settings-spec.md`; the design, mockups, ADR 0042 (proposed) and build tickets 231 to 252 are in the repository (2026-11-21)

## Why

C9: in the session COSMIC Settings serves system settings at first, and
moving each page into Better Manager's control center (ADR 0017) is later
work with its own tickets. Without a ticket that later work has no owner.

## What to deliver

- For each of COSMIC Settings' pages the session uses (displays, network,
  Bluetooth, sound, users, region and language, date and time, keyboard,
  power, accessibility, and whatever ticket 201's inventory adds): what the
  page does in COSMIC Settings 1.11 (read from its source and seen in the VM),
  what it writes (cosmic-config, NetworkManager, BlueZ, AccountsService,
  PipeWire, `cosmic-randr`), and whether the same page must also serve GNOME
  (where GNOME Settings serves it today).
- A design in the form of `design/control-center-spec.md` for the pages the
  owner wants first, with every state and copy in both languages and mockups.
- The order, the decisions for the owner with recommendations, and the build
  tickets cut after the owner answers.

## Done when

- The inventory, the design, and the owner's answers are in the repository,
  and the build tickets exist.

## Notes

Delivered on 2026-11-21:

- The inventory, the design, and the order: `design/system-settings-spec.md`
  (sections 1, 2, 15), with every state and both languages for the wave 1
  pages (sections 6 to 12) and mockups in `design/system-settings/`, rendered
  to `/home/tim/Developer/better-os-evidence/229/` and looked at.
- The decisions for the owner, M1 to M9, with recommendations and costs, at
  the top of that file; ADR 0042 (proposed) for M6.
- The build tickets 231 to 252, each blocked on the owner's decisions it
  depends on (M1 for all of them).

Not done, and why: the owner has not answered M1 to M9 yet, so "the owner's
answers are in the repository" is carried by those blockers rather than met
here; the orchestrator's brief for this ticket closes it once the design,
mockups, and tickets are in (as ticket 99 closed with Better Input's D1 to D9
open). COSMIC Settings was read from its 1.11.0 source only; nothing was seen
in a VM, because no VM image is on the machine (ticket 201 boots one). Each
build ticket's VM check is where the session's behaviour is first seen.
