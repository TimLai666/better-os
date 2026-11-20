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

## Notes

- Cause: the arrow was a 32px `Button` centred in a cell as tall as the
  toolbar, so only that square, not the whole segment after the divider,
  took a press. The fix is `better_ui::fluent::button::MenuSegment`: the
  segment itself is the target, fills its cell's height, draws hover,
  pressed, and open over all of it, and keeps the keyboard (Enter, Space,
  Alt+Down, F4). Better Git's sync cell uses it.
- Other split buttons: the kit's `SplitButton` (Better Manager's Keep awake,
  the gallery) already gave its chevron part the full height and its 35px
  width; a test now holds that. Better Files has no split button; its
  command bar's drop-down commands are whole buttons. No Files change.
- Tests: `better-ui` `fluent::interaction_tests::a_press_in_the_corner_of_a_toolbar_arrow_segment_opens_its_menu`
  and `a_press_in_the_corner_of_a_split_buttons_chevron_opens_its_menu`,
  real presses through GPUI's test window at the segment's corners.
- Seen in a headless sway (wayhand driver inside a capped scope with its own
  runtime directory, home, and session bus) with real pointer presses on a
  scratch repository, zh-TW and English, in
  `/home/tim/Developer/better-os-evidence/130/`: a press on the bottom-right
  and top-left corners of the segment opens the menu, a second corner press
  closes it, and the hover fill covers the segment.
