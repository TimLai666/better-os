# Ticket 80 — The Fluent control kit every window is built from

**Blocked by:** none · **Status:** merged, not released

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

## What was built

- **The kit.** `crates/better-ui/src/fluent.rs` keeps the tokens and gains
  what every control shares: the visual states (`Interaction`), a gallery
  override (`ShownState`) so any state can be drawn without a pointer, WinUI's
  keyboard focus visual (2px outer and 1px inner ring, 3px outside the
  control, shown only after keyboard input), the elevation border's darker
  edge, one `Key` table every control's keyboard rules are written against,
  and `step_index`, the arrow/Home/End rule lists, menus, and panes share.
  Sixteen modules under `fluent/` hold the controls: `theme` (one
  `set_appearance` call that repaints gpui-component in the Fluent palette,
  4px and 8px radii, and 14px type, plus `Appearance` with ADR 0004's
  dark/light/system load, save, and follow-the-desktop), `chrome`
  (titlebar), `navigation`, `command_bar`, `button` (accent, standard,
  subtle, icon, toggle, split), `text_box` (text, password, search, number),
  `toggle` (check box with indeterminate, radio buttons, toggle switch),
  `selection` (combo box, slider), `progress` (bar and ring), `info_bar`
  (info bar, and the shared error pattern: one sentence plus expandable,
  copyable details), `dialog` (content dialog, flyout, tooltip), `menu` (menu
  flyout and context menu with separators, shortcuts, one level of
  submenus), `list` (list rows, details table with sortable headers),
  `settings_card` (card and expander), `breadcrumb`, and `tabs` (tab view and
  selector bar). A shared `words` module gives on/off, states, actions, and
  byte, percent, and duration units in English and zh-TW.
- **Every number comes from WinUI.** Colors, sizes, padding, and per-state
  brushes were read from `microsoft/microsoft-ui-xaml` at
  `e38939f2680f0da968d7536a0b7863617ab5793e` (each module names its
  `*_themeresources.xaml`), and keyboard rules from its C++ (`ButtonBaseKeyProcess.h`,
  `ToggleButton_Partial.cpp`, `ToggleSwitch_Partial.cpp`, `NumberBox.cpp`,
  `RadioButtons.cpp`). The palette is now WinUI's own resources, translucent
  where WinUI's are, and `DESIGN.md` was corrected where the recalled values
  differed: dark secondary text (white at 77%, not `#c5c5c5`), the dark
  dialog fill (`#2b2b2b`), the dark control stroke (white at 7%), and light
  secondary text (`#5c5c5c` over the window). Better Git keeps building; it
  sees the corrected values through the same field names.
- **The gallery** is `better-fluent-gallery` in a new crate,
  `crates/fluent-gallery`, rather than a binary in `better-ui`, so the library
  every window links does not gain `gpui-platform` and
  `gpui-component-assets` (and through them an HTTP client) for a development
  tool. It is `publish = false`, not in `packaging/build-deb.sh`'s package
  list, and not in any manifest. Each page draws every control in Rest,
  Pointer over, Pressed, Focused, and Disabled side by side, then a live copy;
  the command bar switches light and dark and English and zh-TW at runtime.
  `--light`, `--dark`, `--zh-TW`, `--page`, `--compact`, and `--dialog` open
  it in a given state.
- **Owner-audit items folded in.** RadioButtons and SelectorBar always show
  the selected choice; the combo box marks its choice with a fill and an
  accent pill. The info bar tints with the status background, keeps text
  primary, and has a close button. `ErrorReport`/`ErrorBar` are the shared
  error pattern. The navigation pane collapses to 48px behind ☰ with
  tooltips and never disappears. List and table selection is a fill and a 3px
  bar. A test fails if any control sets a text size off the ramp.
  `better_ui::AppIcon` and `icons::IconSearch` resolve a desktop entry's
  `Icon=` through the icon theme directories and fall back to the letter;
  `application_tile_with_icon` and `application_list_row_with_icon` draw it,
  and the old glyph-only functions are unchanged wrappers, so no window had
  to change.

### Compared with WinUI 3

The comparison is against WinUI's resource values, templates, and C++, at
the same pixel scale; there was no Windows machine to put a rendered WinUI
control beside each one. What still differs:

- **Type and icons.** Inter and Noto Sans CJK TC instead of Segoe UI
  Variable, so text sits on a 20px line where Segoe uses 19 (menu items are
  fixed at WinUI's 36px rather than derived). Glyphs are gpui-component's
  Lucide icons, not Segoe Fluent Icons; the info bar's "i" and "!" are text.
- **Materials and motion.** No Mica or Acrylic (a Wayland client cannot ask
  for them): flyouts use the acrylic fallback colors. The elevation border is
  a 1px darker edge, not a gradient. No WinUI transitions: no knob slide, no
  selection-indicator slide, no flyout or dialog open animation. The
  indeterminate bar is one moving segment (WinUI animates two), and the ring
  is a polyline arc whose motion approximates WinUI's, not its Lottie
  animation.
- **Placement.** The combo box drops below its field; WinUI opens the list
  over the field, aligned to the selected item. Tooltips appear where GPUI
  puts them, at the pointer, not above the control. A click on an open split
  button's chevron or combo field closes the menu on mouse-down-outside and
  then reopens it, where WinUI closes it.
- **Scope cut.** NavigationView has the left mode only (no top or minimal
  mode, no hierarchical items, no back button, no auto-suggest box).
  CommandBar has labels on the right only, and its overflow takes a count
  (`max_primary`, with `fit_count` for a window that measures) rather than
  measuring itself. Menus have no toggle or radio items, no access keys, and
  one submenu level. The tab view has no drag, no scroll buttons, and no
  tear-out. The details table is not virtualized and has no column resizing.
  The breadcrumb folds by item count, not measured width. The slider has no
  tick marks and no value tooltip. The text box is single-line and its text
  editing, caret, and context menu are gpui-component's. No high-contrast
  theme.
- **Not from WinUI itself.** WinUI 3 has no settings card and no data grid.
  The settings card and expander follow the Windows Community Toolkit
  controls Windows 11 Settings uses, from their documented sizes rather than
  their source, which was not fetched; the details table follows File
  Explorer and the Toolkit's DataGrid. The selector bar's pill width (16px)
  was read from its animation, not a resource.
- **Accent.** WinUI takes the accent from the system; the kit uses Windows
  11's default blue in both appearances.

### Screenshots

Taken in a headless sway session (wayhand sandbox, 1920×1080), with `HOME`
and the XDG directories pointed at a scratch directory:
`/tmp/claude-1000/-home-tim-Developer-better-os/3b559518-c3b2-4104-bc77-d1c2e845c29a/scratchpad/fluent-shots/`

- Dark: `dark-01-buttons.png`, `dark-02-text.png`, `dark-03-selection.png`,
  `dark-04-status.png`, `dark-05-dialogs-menus-context-menu-open.png`,
  `dark-06-dialogs-flyout-menu-tooltip.png`,
  `dark-07-content-dialog-open.png`, `dark-08-lists-tables.png`,
  `dark-09-lists-app-icons.png`, `dark-10-navigation-view.png`,
  `dark-11-commandbar-breadcrumb-tabs-selector.png`,
  `dark-12-settings-cards.png`, `dark-13-window-chrome-type.png`,
  `dark-14-zh-TW-compact-nav-tooltip-buttons.png`, `dark-15-zh-TW-settings.png`.
- Light: `light-01-buttons.png`, `light-02-text.png`,
  `light-03-selection.png`, `light-04-status.png`,
  `light-05-dialogs-flyout-menu-tooltip.png`, `light-06-flyout-open.png`,
  `light-07-lists-tables-app-icons.png`, `light-08-table-sorted-by-size.png`,
  `light-09-navigation-view.png`,
  `light-10-nav-keyboard-selected-trash-then-compact.png`,
  `light-11-commandbar-breadcrumb-tabs-selector.png`,
  `light-12-settings-cards.png`, `light-13-window-chrome-type.png`,
  `light-14-combo-open-live-keyboard.png`, `light-15-slider-keyboard-51.png`.

They live in a session scratch directory; copy them somewhere durable before
it is cleaned.

### Verified, and not

- Exercised by hand in the sandbox: Tab order skips the gallery's frozen
  samples; Space toggles a check box (indeterminate went to unchecked) and a
  toggle switch, Enter does not toggle the switch; arrows move a radio group;
  the split button's chevron opens its menu; a context menu moves with the
  arrows, skips the disabled item, opens its submenu with Right, and invokes
  with Enter; the content dialog traps Tab and closes on Escape; the flyout
  closes on Escape; the number box steps with Up and Page Up; the combo box
  opens, moves, and commits from the keyboard; the slider steps with Right and
  Page Up; the navigation pane moves with Down, skips headers and disabled
  items, selects with Enter, and collapses with ☰; a table header sorts by
  size with units understood; light, dark, and zh-TW switch at runtime with no
  truncated text.
- Unit tests (`crates/better-ui/src/fluent/tests.rs`, the icon tests in
  `lib.rs`): palette rows against the XAML, the type ramp, every keyboard
  rule, the check box toggle order, the number and slider models, the menu,
  combo, tab, list, radio, selector, and navigation key handling, sorting,
  breadcrumb folding, progress math, dialog buttons, the error report chain,
  appearance storage, shared words and units, and icon resolution.
- Checks: `cargo fmt --all -- --check`, `cargo +1.99.0 clippy` and stable
  `cargo clippy --workspace --all-targets --all-features --locked -- -D
  warnings`, `cargo +1.95 check --workspace --all-targets --locked`, `cargo
  test --workspace --locked --no-fail-fast` (3238 passed, 0 failed), and
  `packaging/generate-third-party-notices.sh --check` all pass. The only new
  package in `Cargo.lock` is the workspace's own `fluent-gallery`.
- Not verified: a GNOME/Mutter desktop (only headless sway), dragging a slider
  past its own bounds, touch input, a screen reader, right-to-left text, and
  any window other than the gallery. No first-party window uses the kit yet;
  each moves over in its own ticket.
