---
version: alpha
name: Better OS
description: Every Better OS window in the visual language of WinUI on Windows 11, inside the desktop's own window frame, on a Linux desktop.
colors:
  primary: "#1a1a1a"
  secondary: "#5c5c5c"
  tertiary: "#005fb8"
  neutral: "#f3f3f3"
typography:
  h1:
    fontFamily: system-ui
    fontSize: 28px
    fontWeight: 600
    lineHeight: 36px
  h2:
    fontFamily: system-ui
    fontSize: 20px
    fontWeight: 600
    lineHeight: 28px
  body-md:
    fontFamily: system-ui
    fontSize: 14px
    fontWeight: 400
    lineHeight: 20px
  label-sm:
    fontFamily: system-ui
    fontSize: 12px
    fontWeight: 400
    lineHeight: 16px
rounded:
  sm: 4px
  md: 8px
spacing:
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 24px
components:
  button-primary:
    backgroundColor: "{colors.tertiary}"
    textColor: "#ffffff"
    rounded: "{rounded.sm}"
    height: 32px
  button-standard:
    backgroundColor: "#fbfbfb"
    textColor: "{colors.primary}"
    rounded: "{rounded.sm}"
    height: 32px
  card:
    backgroundColor: "#fbfbfb"
    rounded: "{rounded.sm}"
    padding: 16px
  dialog:
    backgroundColor: "#ffffff"
    rounded: "{rounded.md}"
    padding: 24px
---

## Overview

The owner decided on 2026-10-05 that every first-party window looks like a WinUI application on Windows 11. A person who knows Windows 11's Settings or File Explorer should find the same shapes, spacing, and type sizes here. Better Git is the first window built this way. The windows built before it keep their earlier look until each one is moved over.

This file is the baseline for all of them. The type ramp and the corner radii are taken from Microsoft's Windows app design guidance, read on 2026-11-11. The color values were checked on 2026-11-12 against WinUI's own theme resources in `microsoft/microsoft-ui-xaml` at commit `e38939f2680f0da968d7536a0b7863617ab5793e`: `controls/dev/CommonStyles/Common_themeresources_any.xaml` for fills, strokes, text, and status colors, and `controls/dev/Materials/Acrylic/AcrylicBrush_themeresources.xaml` for the flyout fill. Four values recalled earlier were wrong and are corrected below: dark secondary text, the dark dialog fill, the dark control stroke, and light secondary text.

The window frame is the exception. The owner decided on 2026-11-14 (ticket 108) that a window's corners, edges, shadow, title bar background, and minimize, maximize, and close buttons look like every other window on the desktop, the way Chromium and Electron windows such as Claude Desktop do; everything inside the title bar and the window stays Fluent. See [Window frame](#window-frame).

Every control is built once, in `better_ui::fluent` (ticket 80): window chrome, navigation view, command bar, buttons, text fields, check boxes, radio buttons, toggle switches, combo boxes, sliders, progress, info bars, dialogs, flyouts, menus, tooltips, lists, tables, settings cards, breadcrumbs, tab views, and selector bars. A window uses those rather than drawing its own. `better-fluent-gallery` shows each of them in every state, in both appearances.

## Colors

Two appearances, with the same roles in each. WinUI writes many of these as translucent colors, and the kit keeps them translucent, because a fill over a card reads differently from the same fill over the window. The hex value beside a translucent one is what it looks like over the window background; the front matter gives those composites for light.

| Role | WinUI resource | Light | Dark |
| --- | --- | --- | --- |
| Window background | `SolidBackgroundFillColorBase` | `#f3f3f3` | `#202020` |
| Card fill | `CardBackgroundFillColorDefault` | white at 70% (`#fbfbfb`) | white at 5% (`#2b2b2b`) |
| Control fill | `ControlFillColorDefault` | white at 70% (`#fbfbfb`) | white at 6% (`#2d2d2d`) |
| Dialog fill | `LayerFillColorAlt` over the base | `#ffffff` | `#2b2b2b` |
| Flyout, menu, and tooltip fill | `AcrylicInAppFillColorDefault` fallback | `#f9f9f9` | `#2c2c2c` |
| Primary text | `TextFillColorPrimary` | black at 89% (`#1a1a1a`) | `#ffffff` |
| Secondary text | `TextFillColorSecondary` | black at 62% (`#5c5c5c`) | white at 77% (`#cccccc`) |
| Disabled text | `TextFillColorDisabled` | black at 36% | white at 36% |
| Accent fill | `AccentFillColorDefault` | `#005fb8` | `#60cdff` |
| Text on accent | `TextOnAccentFillColorPrimary` | `#ffffff` | `#000000` |
| Hover and selection fill | `SubtleFillColorSecondary` | black at 4% | white at 6% |
| Control stroke | `ControlStrokeColorDefault` | black at 6% | white at 7% |
| Card stroke | `CardStrokeColorDefault` | black at 6% | black at 10% |
| Strong stroke (check box, field bottom edge) | `ControlStrongStrokeColorDefault` | black at 45% | white at 55% |
| Critical | `SystemFillColorCritical` | `#c42b1c` | `#ff99a4` |
| Success | `SystemFillColorSuccess` | `#0f7b0f` | `#6ccb5f` |
| Caution | `SystemFillColorCaution` | `#9d5d00` | `#fce100` |
| Critical, success, caution background | `SystemFillColor*Background` | `#fde7e9`, `#dff6dd`, `#fff4ce` | `#442726`, `#393d1b`, `#433519` |

The accent shades are not in the WinUI source: WinUI reads them from the operating system (`SystemAccentColorDark1` in light, `SystemAccentColorLight2` in dark). The values here are Windows 11's default blue, used when the desktop gives no accent. Since ticket 108 the accent follows the desktop's, as WinUI's follows Windows': when the GTK theme defines `accent_bg_color` and `accent_fg_color` (Adwaita's blue, or Zorin's per-theme colour), those become the accent fill and the text on it, in both appearances. Every other value is the resource named beside it; `crates/better-ui/src/fluent.rs` holds the full set and `crates/better-ui/src/fluent/tests.rs` asserts it row by row.

An info bar is drawn on its status background, never on the status color itself: the color goes only to the icon.

Accent is for the one primary action on a surface, the selected item's indicator, and a focused field's bottom edge. It is never a decoration. Status colors mark a state and are always paired with words or an icon, never used alone.

A window starts in the desktop's appearance and keeps following it while open; light and dark are stored choices, and a picker lists "Use system setting" first ([ADR 0019](/engineering/decisions/0019-appearance-follows-system.md), which replaced ADR 0004's dark default).

## Typography

The WinUI type ramp, size over line height, in pixels:

| Style | Size | Weight | Used for |
| --- | --- | --- | --- |
| Caption | 12/16 | 400 | Metadata, hints, timestamps |
| Body | 14/20 | 400 | Everything a person reads or clicks |
| Body Strong | 14/20 | 600 | A row's name, a field's label, a selected item |
| Subtitle | 20/28 | 600 | A section or pane title |
| Title | 28/36 | 600 | The page title, once per page |

Windows uses Segoe UI Variable, which cannot be shipped. Use the platform UI font GPUI supplies; on the supported desktops that resolves to Inter for Latin text and Noto Sans CJK TC for Chinese. Hierarchy comes from this ramp and from secondary text color. Nothing is larger than Title, and body text is never smaller than 14.

Code, diffs, paths, and hashes use the platform monospace font at 13/20.

## Layout

Spacing is on a 4px grid: 4 between an icon and its label, 8 between related controls, 12 inside a control row, 16 as a card's padding and between cards, 24 as a page's margin. Settings cards in one group sit 4px apart, as in Windows 11 Settings.

A window has a titlebar, a navigation pane or command bar, and one content area. The navigation pane is 320px wide when it holds a list a person works in (files, repositories) and 48px when it holds only icons. A pane that collapses goes to 48px behind a ☰ toggle, with each item naming itself in a tooltip; it never disappears. Rows in a list are 36px tall, or 48px when a row has a second line of caption text. Standard controls are 32px tall.

Content-driven widths, wrapping, and stacking at narrow sizes still apply. Descriptions wrap; only identifiers that are not needed to act may truncate.

## Elevation & Depth

Depth is carried by layers of fill, not by shadows on everything. The window background is the lowest layer. Cards and panes sit on it with the card fill and a 1px stroke. Only what floats above the page (dialogs, flyouts, menus, tooltips) has a shadow, and those use the dialog fill and the 8px radius.

Windows 11 puts a translucent material behind the window. A Wayland client cannot ask the compositor for that, so the window background is the solid color in the table.

## Shapes

Two radii, as on Windows 11. 4px for everything that sits in the page: buttons, fields, checkboxes, list row highlights, cards, progress bars. 8px for what floats above it: dialogs, flyouts, menus. Edges that meet another edge, such as a pane against the window edge, are not rounded. There are no pills.

A selected row in a list has a subtle fill across the row and a 3px accent bar, 16px tall and rounded, at its leading edge, never an outline. A text field has a 1px stroke whose bottom edge is the strong stroke at rest and a 2px accent line while focused; that line is the field's focus indicator. Every other control shows keyboard focus with WinUI's focus visual: a 2px ring in primary text color with a 1px inner ring in the opposite color, drawn 3px outside the control, and only when focus came from the keyboard.

## Icons

The owner decided on 2026-11-21 that icons inside a window are full-colour pictures, as in Windows 11's File Explorer and Settings, and no longer the single-colour line icons the toolkit brings with it. `design/design-demos/icon-directions.html` shows the old icons beside the chosen ones.

There are three kinds, told apart by what the icon stands for.

**An object icon** stands for a thing: a folder, a device, a file kind, a settings category, a component, a place in a navigation pane. It is a filled picture with no outline.

- Each one has its own outline and the colour the thing has in the world: a red-roofed house for Home, a photograph with a sun and hills for Pictures, a clapperboard for Videos. No shared tile, badge, or folder shape sits behind them.
- Light comes from the top left in every icon: the top of a shape is lighter than its bottom, and a shape may carry one soft gradient to show it. Nothing glows and nothing casts a shadow outside its own shape.
- An icon uses at most four colours and is drawn on a 24px grid so that it still reads at 16px and 20px, the two sizes rows use.
- The same picture is used in the light and the dark appearance, so every icon must stand out against both window backgrounds (`#f3f3f3` and `#202020`). A pale shape, such as a sheet of paper, gets a thin edge in a darker shade of its own colour.
- A disabled object icon is drawn at 36% opacity, the same as disabled text.

**An action icon** stands for something a button does: copy, rename, delete, refresh, new folder. It is a filled shape in two layers of one colour, and it never carries a colour of its own, because a row of buttons should not compete with the content. `design/design-demos/action-icon-options.html` shows the approved drawings under "三（修過）".

- The main layer draws the body of the thing in the full colour: the front sheet of copy, the pencil's shaft, the folder, the whole bin. The second layer draws only a minor part in a lighter shade of the same colour: the sheet behind, the pencil's tip, the folder's tab, the two slots on the bin. The body is never the lighter layer.
- The two layers do not overlap. Where one sits behind the other, only its visible part is drawn, with a gap between them. The lighter shade is a colour of its own in each appearance, not the full colour made transparent, which turns muddy on a dark background.
- A mark on top of a shape, such as the plus on the new-folder icon, is cut out of it.
- The colour depends on where the icon is. Alone, as in a toolbar button, it is the accent and drawn at 18px. Beside a text label, as in a menu row, it is the secondary text colour and drawn at 16px, so the words lead. Delete, and any other action that destroys something, is the critical colour in both places.

Four other treatments were tried on 2026-11-21 and the owner turned each down: a line icon with one coloured part reads as neither line nor colour; a detailed, shaded picture is too realistic for a button; a flat shape in its own bright colour per action looks childish in a row; and accent-coloured icons beside menu labels are too heavy.

**A plain symbol** is the third kind: arrows, chevrons, close, check, and the window buttons. These are marks, not pictures, and stay a 2px line with round ends in the primary text colour.

An icon in an info bar keeps its status colour as before. A tray icon stays a single-colour symbolic icon, because the desktop recolours those for its own panel.

Every icon is drawn once, in `better-ui`, and a window asks for it by name; a window does not draw its own. In Better Files, the folder and file pictures in the content area still come from the desktop's icon theme.

## Window frame

GNOME's compositor draws no frame for a Wayland window, so every application draws its own; GTK applications, Chromium, and Electron look like the desktop because GTK draws theirs from the current theme. Better OS does the same (ticket 108): `better_ui::system_frame` opens GTK 4 (and libadwaita when installed) at run time, asks it to draw the window frame with its shadow, the header bar, and the title buttons in each state into Cairo images, and the kit paints those images. It never imitates a theme by hand. Chromium's `ui/gtk/window_frame_provider_gtk.cc` and `nav_button_provider_gtk.cc` are the method.

- **Corners, edges, shadow, outline.** As GTK draws a window under the current theme, focused and in the background. All four corners are rounded with the theme's radius, measured from GTK's drawing (the top and the bottom each), as a GTK 4 or libadwaita window's are (ticket 141). The theme's `outline` (libadwaita's 1px inner line at 7% white) is drawn over the window's content as GTK draws it over its children. Maximized, fullscreen, or tiled on any edge, the corners are square and there is no frame or outline.
- **Content inside the corners.** Nothing a window draws crosses its rounded corners, and nothing is given up for it: content keeps the window's full width. GPUI clips only to rectangles, so the kit follows one rule. Every surface that reaches a corner paints its own fill into it with `window_chrome::corner_fill`, so its colour runs to the curve. What scrolls past a bottom corner is painted with `corner_clip(inset)`, which raises only its bottom by the curve's depth at the rows' own side inset (5px for rows 4px in at a 15px corner, nothing for a page padded 15px or more, the whole radius for rows that run the full width), and its scroll extent ends with the same depth (`corner_depth`) so the last row scrolls fully above the curve and stays clickable. A scrollbar that reaches the corner ends its track above the curve (`corner_scrollbar`). Tooltips, menus, and floating panels are moved inside the window's rounded shape (`inside_window`). `NavigationView`, dialogs, toasts, menus, and the staged-changes bar follow the rule already.
- **Resizing.** Every edge and corner, from GTK 4's 12px handle outside the window, with 24px corners. The input region follows the window's rounded corners grown by the handle, so clicks on the shadow beyond it go to whatever is behind; a press in a window corner outside the curve resizes from that corner.
- **Title bar.** As tall as GTK's header bar, with its background and bottom shade. The buttons are GTK's, in the order and on the side `org.gnome.desktop.wm.preferences button-layout` names, and change as soon as it does. Double, middle, and right clicks on empty space follow `action-double-click-titlebar`, `action-middle-click-titlebar`, and `action-right-click-titlebar`; the right click's default is the compositor's window menu. Dragging moves the window; dragging a maximized one restores it.
- **Inside the title bar.** Fluent: the back button, the app's icon and title in Body Strong, and any controls the app puts there, between the button groups.
- **Light and dark.** The frame is drawn in the window's own appearance, which follows the desktop's `color-scheme` when the person chose "system" (owner, 2026-11-14): a window someone set to light has a light frame on a dark desktop, as a libadwaita application that forces its scheme does.
- **Without GTK.** On a desktop where GTK 4 cannot be loaded, the kit draws Adwaita's frame itself (12px corners, a 47px header, round title buttons), never the old Windows caption buttons.

## Components

- **Buttons.** One accent button per surface, for the action the person came to do. Everything else is a standard button with the control fill and stroke. Text is a verb.
- **Command bar.** Icon-and-label buttons in a row across the top of a content area, without fills until hovered.
- **Lists.** Rows with a leading icon or checkbox, a Body Strong name, and Caption metadata. Hover and selection are fills, never borders.
- **Info bar.** A full-width strip under the command bar for a state the person has to know about: an icon in the status color, a Body Strong title, a Body message, and at most one action.
- **Dialogs.** A Subtitle title, Body text, and buttons at the bottom right with the primary one in accent.
- **Light-dismiss panels.** A flyout, menu, combo box list, or suggestion list closes on a press outside it, and a second press on the control that opened it only closes it; the next press opens it again.
- **Click targets.** The highlighted area is the click area: a row's hit area fills the row and meets the next one, and its inset fill is drawn inside it. A highlight shows only while the pointer is on that row.
- **Field reasons.** When a dialog's primary button is off, the reason is under the field it is about, in the critical color after an error glyph.

## Do's and Don'ts

- Do keep lifecycle wording user-facing: `Review changes`, `Install updates`, `Applying settings`, `Checking that everything works`, and `Restore previous version`.
- Do localize all UI copy at runtime for `zh-TW`, `en-US`, and `system` fallback.
- Do let every surface read its colors from the active theme so both appearances stay correct.
- Do open a window with `better_ui::window_chrome::window_options` and `better_ui::window_chrome::root`, and draw its title bar with `fluent::chrome::TitleBar`; Mutter gives a Wayland client no frame, and these give it the desktop's.
- Don't draw a window frame, a shadow, or window buttons in an app, and don't paint a background on the view's root: the frame paints the window background with the desktop's corners.
- Don't give an element that reaches a window corner a plain `.bg()`: draw its fill with `window_chrome::corner_fill`; give a list that scrolls past a bottom corner `corner_clip` with its rows' side inset and `corner_depth` as its last padding, and keep a page's own side padding away from the window's edge.
- Don't use fixed card or button widths that can clip translated content.
- Don't put a shadow, a gradient, or a radius larger than 4px on anything that sits in the page.
- Don't use the accent color for more than the primary action, the selection indicator, and focus.
- Don't hardcode a component's name, purpose, or icon by ID. Those come from the manifest, or from a shipped translation for a first-party component.
