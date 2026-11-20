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
- Review fixes: `MenuSegment` now owns the divider, the cell's height, and
  its menu, and gives focus back to itself when the menu closes (Escape or a
  choice), as `SplitButton` and `DropDownButton` do; Better Git draws it as
  the last child of its sync cell with nothing around it, so the corner
  test's harness and the app draw the same thing, and the test measures the
  segment against the cell (same top, bottom, and trailing edge, starting at
  the divider). New tests: Enter, Space, Alt+Down, and F4 each open the menu
  once on its first item without invoking the primary part, and Escape gives
  focus back (`escape_gives_focus_back_to_a_toolbar_arrow_segment`).
- Seen again with the keyboard, English:
  `en-US-4-keyboard-enter-on-focused-segment-opens-menu-on-first-item.png`
  and `en-US-5-keyboard-escape-closes-menu-focus-ring-back-on-segment.png`.

### Checks that ran

Run in the worktree through the build lock wrapper, on 2026-11-21:

- `cargo fmt --all --check`: exit 0.
- `cargo +stable clippy -p better-ui -p monitor-views -p monitor-gui -p git-gui
  -p manager-gui -p app-chooser-gui -p fluent-gallery -p awake-gui
  -p launcher-gui -p touchpad-gui --all-targets -- -D warnings` (rustc
  1.97.1): exit 0.
- `cargo test -p better-ui -p monitor-views -p monitor-gui -p git-gui`:
  exit 0, 534 passed, 0 failed.
- Not run: clippy and tests of `files-gui`, a dependent of `better-ui`. It
  does not build on this machine: `glib-sys` finds no `glib-2.0` through
  pkg-config (GStreamer, through `files-preview`). Nothing in `files-gui`
  changed.
- `escape_gives_focus_back_to_a_toolbar_arrow_segment` was also run with the
  focus hand-back removed, and failed, so it guards the fix.
