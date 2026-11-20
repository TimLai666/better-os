# Ticket 229 — Display, network, Bluetooth, sound, users, and region settings in Better Manager: design

**Spec:** better-os-session.md, Not doing (C9)
**Blocked by:** none for the design; building waits on the owner's answers

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
