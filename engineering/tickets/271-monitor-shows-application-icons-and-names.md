# Ticket 271 — Better Monitor shows each application's icon and its own name

**Blocked by:** none

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks.

Ticket 23 named `app-catalog-core` as where an application's name and icon
come from. The Processes page shows neither an icon nor the desktop entry's
translated name.

## What to build

- `monitor-views`' grouping takes an application's display name and icon from
  `app-catalog-core`'s desktop-entry lookup (the application id ticket 174
  parses), and the Processes page draws the icon. No second desktop-entry
  reader.
- An application with no desktop entry keeps today's name and gets the
  generic picture, never a guessed icon.

## Done when

- Tests: an id with an entry, without one, a Flatpak, an entry whose icon is
  missing; zh-TW names. Seen in a sandbox with fixture entries.
