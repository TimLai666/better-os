# Ticket 148 — Better OS's own visual style in every window

**Blocked by:** 147 (the icons the style is drawn around)

## Why

On 2026-11-21 the owner asked for one overall style designed from the new
colour icons and from Luma (simplyluma.com), whose look the owner likes.
Shown `design/design-demos/style-direction.html` (Better Files, a control
center page, and a dialog, menu, and toast, light and dark), the owner said
everything but the action icons was very good; those were settled separately
and are in DESIGN.md's Icons section. This replaces the decision of 2026-10-05
that every window looks like WinUI on Windows 11.

Luma gave the approach, not the material: none of its colours, icons, or
artwork is used.

## The style, as approved in the demo

- **A sheet on a base.** The window is a warm grey base (`#f1efeb` light,
  `#1e1d1b` dark). The navigation pane sits directly on it. The content area
  is one sheet (`#ffffff`, `#282725`) with a 12px radius, a 1px line, and no
  shadow, 8px in from the window's right and bottom edges.
- **Controls float over the content.** A command bar is not a strip across
  the top: it is rounded groups (12px radius, raised fill, a soft shadow)
  lying on the sheet, the path on the left and the actions on the right.
  A running file operation is another such group at the bottom of the sheet.
- **Selection is a raised row.** A selected row in a navigation pane is the
  raised fill with a small shadow and Body Strong text, in place of the subtle
  fill with a 3px accent bar. A selected tile in the content area is the
  accent's soft tint.
- **A row with a second line is one block.** A device with its capacity bar
  and free space is a single row whose hover and selection cover all of it
  (owner, 2026-11-21).
- **Three radii.** 8px for controls and rows, 12px for the sheet, cards,
  menus, and floating groups, 16px for dialogs. Count chips, switches, and
  slider thumbs are fully round.
- **Warm neutrals.** Text `#1c1b19` / `#f3f1ec`, secondary `#66635d` /
  `#b6b2aa`; lines and hover fills are the text colour at 9% and 5%.
- **Each window's own tint.** The top 150px of the sheet fades from a 13% mix
  of the application's colour into the sheet fill (blue for Files, violet for
  the control center).
- **Accent follows the desktop**, as since ticket 108; `#3a6ff0` (light) and
  `#86a8ff` (dark) when the desktop gives none.
- **Shadows only on what floats**: floating groups, the selected row, menus,
  toasts, dialogs. Light comes from above in all of them.
- Type sizes, the 4px spacing grid, and row and control heights are unchanged.

## Taken as shown unless the owner says otherwise

The owner was told these three would follow the demo and did not object; none
was confirmed in words.

1. The title bar's background is the window's base, drawn by Better OS, so it
   runs into the navigation pane. The minimize, maximize, and close buttons
   stay the desktop's own (ticket 108), in its order and on its side.
2. The per-window tint stays.
3. The new style replaces WinUI everywhere, not window by window as an option.

## What to build

1. **DESIGN.md rewritten** to this style: front matter, Overview, Colors,
   Layout, Elevation, Shapes, Components, and the do and don't list. The Icons
   and Window frame sections are kept and brought in line. Every value the
   demo uses is written there, and the demo then follows DESIGN.md.
2. **The kit.** Every control in `better_ui::fluent` takes the new shapes,
   fills, and shadows: window chrome, navigation view, command bar (as
   floating groups), buttons, text fields, check boxes, radio buttons, toggle
   switches, combo boxes, sliders, progress, info bars, dialogs, flyouts,
   menus, tooltips, lists, tables, settings cards, breadcrumbs, tab views,
   selector bars, toasts. `fluent/tests.rs` asserts the palette row by row and
   is updated with it. Whether the module keeps the name `fluent` is decided
   here and recorded.
3. **Every window laid out as sheet on base**: Files, Git, Monitor, Manager
   and the control center pages, Launcher (which has no title bar), Awake,
   Touchpad, the app chooser, and the gallery.
4. Ticket 142's corner radius and this ticket's radii are one setting; 142's
   taskbar and compositor parts are untouched by this ticket.

## Done when

- The gallery shows every control in every state in both appearances, and
  screens of each window, light and dark, have the owner's go-ahead.
- No window paints the old palette: a test fails on any WinUI colour constant
  left in use.
- Keyboard focus, contrast (4.5:1 for text, 3:1 for control edges and icons),
  and the reduced-motion setting are checked on the new controls.
