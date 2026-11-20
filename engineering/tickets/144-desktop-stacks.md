# Ticket 144 — Desktop stacks: files on the desktop grouped automatically

**Blocked by:** 214 (Better Files draws the desktop in the Better OS session).
Before 2026-11-21: the desktop-icons decision of ticket 143, which the owner
answered as C6 of [ADR 0038](/engineering/decisions/0038-cosmic-as-the-desktop.md)
(Better Files); on GNOME also a desktop row in ADR 0036, which waits.

## Under ADR 0038's option D (2026-11-21)

The owner answered C6 on 2026-11-21: Better Files draws the desktop. So this
ticket's blocker is met on the session side: in the Better OS session the
desktop is ticket 214's layer surface, and stacks build on it. On GNOME the desktop stays Zorin's extension until a desktop row is added
to ADR 0036, which waits with ticket 151 (C6: built only if the owner keeps
building GNOME-only surfaces), so stacks ship in the session first.

## Why

The owner asked on 2026-11-16 for macOS-style Stacks on the desktop: files
on the desktop gathered automatically into piles by kind, so a busy desktop
stays tidy without moving anything.

## What to build

- A "Use stacks" switch for the desktop. On: items in `~/Desktop` are grouped
  into stacks — by kind (Images, Documents, PDFs, Spreadsheets, Screenshots,
  Videos, Archives, Folders, Others), or by date added / modified, chosen by
  the person. Nothing moves on disk; a stack is only a view.
- Clicking a stack fans it open in place; scrolling over a closed stack
  previews its items; dragging a file out of a stack or a new file onto the
  desktop files it into its stack immediately.
- Stacks follow the same kind rules Better Files already uses for icons and
  grouping (one place decides a file's kind), and sort and arrange like the
  rest of the desktop.
- Works with keyboard navigation and screen readers; off restores the
  person's own icon positions exactly.

## Done when

- Tests for grouping (each kind, date buckets, new and removed files, rename
  across kinds) and for positions restored after turning stacks off.
- Screens with stacks closed and open, light and dark.
