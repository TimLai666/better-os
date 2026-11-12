# Ticket 80 — The Fluent control kit every window is built from

**Blocked by:** none · **Status:** ready

## Why

The owner wants every Better OS window in Fluent Design (WinUI 3 on
Windows 11) and finds all of them unusable as they are. Every window is built
on gpui-component, whose controls look like a web component library, not like
Windows 11. Better Git restyled a few of them by hand and still shows the
wrong focus ring. Eight windows are about to be rebuilt in parallel; if each
one restyles controls on its own, they will not match and the work is done
eight times. This ticket builds the controls once, in `better-ui`, so every
window rebuild uses the same ones.

## What to build

In `crates/better-ui`, a `fluent` module grown from today's token file into a
control kit. Each control follows Microsoft's WinUI 3 guidance for its size,
spacing, states (rest, hover, pressed, focused, disabled), and keyboard
behaviour, in light and dark, reading `DESIGN.md`'s tokens:

- Theme: one call that sets gpui-component's theme to the Fluent palette,
  radii, and fonts for light and dark, so any gpui-component control still in
  use looks as close as it can.
- Window chrome: the titlebar with the app icon and title, and the window
  buttons, matching Windows 11.
- NavigationView: the left pane with items, section headers, a selection bar,
  an optional footer item (Settings), and a compact 48px mode.
- CommandBar and its buttons (icon, icon with label, overflow `…`).
- Buttons: accent, standard, subtle; icon button; toggle button; split button.
- Text box with the 2px accent bottom edge when focused, password box, search
  box, number box.
- Check box with the indeterminate state, radio buttons, toggle switch.
- Combo box (drop-down), slider, progress bar, progress ring.
- Info bar (informational, success, warning, error), content dialog, flyout,
  menu flyout (context menu) with separators, keyboard shortcuts, and
  submenus, tooltip.
- List rows with selection fill and bar; a details table with sortable column
  headers.
- Settings card and expander, as in Windows 11 Settings, because most windows
  are settings pages.
- Breadcrumb bar, tab view.
- `better-fluent-gallery`: a small window, like the WinUI 3 Gallery, that
  shows every control in every state, in light and dark. It is how this kit is
  looked at and how its screenshots are taken.

## Rules

- Every control is keyboard operable and has a visible focus indicator.
- Text never smaller than 12px; Body 14px; the type ramp from `DESIGN.md`.
- No control truncates zh-TW text that fits in English.
- The DESIGN.md colors are recalled values; check each against the WinUI 3
  source (`microsoft/microsoft-ui-xaml`, the theme resource dictionaries) and
  correct DESIGN.md where it differs, citing the file.

## Acceptance

- [ ] The gallery opens and shows every control above, in light and dark,
      and every state; screenshots of each page are attached to the ticket's
      What was built.
- [ ] Each control is compared with the matching WinUI 3 control at the same
      scale, and the differences that remain are listed.
- [ ] Unit tests for every piece of logic that is not drawing (state
      machines, keyboard handling, sorting).
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass. No new external dependency.
