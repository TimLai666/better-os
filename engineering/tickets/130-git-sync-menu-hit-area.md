# Ticket 130 — Better Git: the sync button's whole arrow segment opens the menu

**Blocked by:** none

## Why

The owner reported on 2026-11-16 that only the arrow glyph beside Better
Git's fetch/pull/push button opens its menu; clicking elsewhere in the arrow's
segment does nothing.

## What to build

- The whole dropdown segment of the split button (its full height and width,
  including padding, up to the divider) is the click target, with the hover
  and pressed state drawn over the whole segment, as GitHub Desktop and
  WinUI's SplitButton do.
- Check every other split button in the suite (Better Manager, Better Files)
  and fix the same pattern at its source in `better-ui` if it is shared.

## Done when

- A test clicks the segment's corner (not the glyph) and the menu opens.
- Seen in a sandbox with real mouse clicks.
