# Ticket 144 — Desktop stacks: files on the desktop grouped automatically

**Blocked by:** the desktop-icons decision in ticket 143 (who draws the
desktop: Better Files, COSMIC Files, or Zorin's extension)

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
