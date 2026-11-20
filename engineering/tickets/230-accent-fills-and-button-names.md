# Ticket 230 — Accent-coloured marks that a pale accent hides, and buttons with no accessible name

**Blocked by:** none

## Why

Ticket 147's reviews found two faults outside that ticket's scope (icons),
and left them for a ticket of their own.

1. **A pale desktop accent hides what is still drawn in the raw accent.**
   Since ticket 147, icons and accent text keep their contrast whatever the
   desktop's accent is (`fluent::icon_accent_for`, `accent_text_for`). The
   selection indicator of a navigation pane and of a list row, badges on an
   accent fill, and the letter tiles in lists still use the accent as the
   theme gives it. With a theme made for a dark desktop shown on a light
   window (`ZorinBlue-Dark` gives `#bde6fb`, about 1.2:1 on `#f3f3f3`) the
   selection bar and a "Default" badge are barely visible
   (`/tmp/claude-1000/t147-screens/round2/13-awake-quick-sessions-light.png`
   when it was taken); white text on Zorin's light accent `#15a6f0` is 2.7:1.
2. **An icon-only button has no accessible name.** `gpui-component`'s
   `Button` takes its accessible name from `label` only, so a button with an
   icon and a tooltip and no label announces nothing. Better Awake's Quick
   Sessions start button is one; ticket 147 gave it a tooltip and could do no
   more. Neither `gpui-component`'s sidebar nor the kit's `NavigationView`
   sets a role or a name on a navigation item, and whether GPUI's
   accessibility layer infers one from the text was not checked.

## What to build

- One rule for a mark drawn in the accent (indicator, badge fill, letter
  tile): which contrast it must keep against what it sits on (3:1, as for an
  icon) and for the text on it (4.5:1), stepped as ticket 147's roles are,
  with the fill of a primary button left as the desktop gives it unless the
  owner decides otherwise. DESIGN.md says it first.
- Every control in `better_ui::fluent` that draws an icon-only button or a
  navigation item gives it an accessible name and role, from its tooltip or
  label; where `gpui-component`'s `Button` cannot carry one, the kit's own
  button is used or the kit wraps it. Find every icon-only button in the
  windows by a source scan, as ticket 147's `window_icons.rs` does for icons.

## Done when

- The accent table test of ticket 147 covers indicators, badges, and tiles in
  both appearances, and fails on today's code for the pale accent.
- A test fails when a window draws an icon-only button with no accessible
  name; a screen reader pass in a headless session (Orca's log, or AT-SPI
  read directly) names the navigation items and the icon-only buttons of one
  window.
