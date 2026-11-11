---
version: alpha
name: Better OS
description: Every Better OS window in the visual language of WinUI on Windows 11, on a Linux desktop.
colors:
  primary: "#1a1a1a"
  secondary: "#5f5f5f"
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

This file is the baseline for all of them. The type ramp and the corner radii are taken from Microsoft's Windows app design guidance, read on 2026-11-11. The color values are WinUI's default theme resources as recalled, and should be checked against the WinUI source before the first release that ships them.

## Colors

Two appearances, with the same roles in each. Light values are in the front matter.

| Role | Light | Dark |
| --- | --- | --- |
| Window background | `#f3f3f3` | `#202020` |
| Card and control fill | `#fbfbfb` | `#2b2b2b` |
| Dialog and flyout fill | `#ffffff` | `#2c2c2c` |
| Primary text | `#1a1a1a` | `#ffffff` |
| Secondary text | `#5f5f5f` | `#c5c5c5` |
| Accent fill | `#005fb8` | `#60cdff` |
| Text on accent | `#ffffff` | `#000000` |
| Control and card stroke | black at 6% | white at 8% |
| Critical | `#c42b1c` | `#ff99a4` |
| Success | `#0f7b0f` | `#6ccb5f` |
| Caution | `#9d5d00` | `#fce100` |

Accent is for the one primary action on a surface, the selected item's indicator, and a focused field's bottom edge. It is never a decoration. Status colors mark a state and are always paired with words or an icon, never used alone.

Which appearance a window starts in is unchanged: dark by default, with light and system-follow as stored choices ([ADR 0004](docs/decisions/0004-dark-first-themeable-appearance.md)).

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

Spacing is on a 4px grid: 4 between an icon and its label, 8 between related controls, 12 inside a control row, 16 as a card's padding and between cards, 24 as a page's margin.

A window has a titlebar, a navigation pane or command bar, and one content area. The navigation pane is 320px wide when it holds a list a person works in (files, repositories) and 48px when it holds only icons. Rows in a list are 36px tall, or 48px when a row has a second line of caption text. Standard controls are 32px tall.

Content-driven widths, wrapping, and stacking at narrow sizes still apply. Descriptions wrap; only identifiers that are not needed to act may truncate.

## Elevation & Depth

Depth is carried by layers of fill, not by shadows on everything. The window background is the lowest layer. Cards and panes sit on it with the card fill and a 1px stroke. Only what floats above the page (dialogs, flyouts, menus, tooltips) has a shadow, and those use the dialog fill and the 8px radius.

Windows 11 puts a translucent material behind the window. A Wayland client cannot ask the compositor for that, so the window background is the solid color in the table.

## Shapes

Two radii, as on Windows 11. 4px for everything that sits in the page: buttons, fields, checkboxes, list row highlights, cards, progress bars. 8px for what floats above it: dialogs, flyouts, menus. Edges that meet another edge, such as a pane against the window edge, are not rounded. There are no pills.

A selected row in a list has a subtle fill across the row and a 3px accent bar, 16px tall and rounded, at its leading edge. A text field has a 1px stroke with a 2px accent bottom edge while focused.

## Components

- **Buttons.** One accent button per surface, for the action the person came to do. Everything else is a standard button with the control fill and stroke. Text is a verb.
- **Command bar.** Icon-and-label buttons in a row across the top of a content area, without fills until hovered.
- **Lists.** Rows with a leading icon or checkbox, a Body Strong name, and Caption metadata. Hover and selection are fills, never borders.
- **Info bar.** A full-width strip under the command bar for a state the person has to know about: an icon in the status color, a Body Strong title, a Body message, and at most one action.
- **Dialogs.** A Subtitle title, Body text, and buttons at the bottom right with the primary one in accent.

## Do's and Don'ts

- Do keep lifecycle wording user-facing: `Review changes`, `Install updates`, `Applying settings`, `Checking that everything works`, and `Restore previous version`.
- Do localize all UI copy at runtime for `zh-TW`, `en-US`, and `system` fallback.
- Do let every surface read its colors from the active theme so both appearances stay correct.
- Do draw the titlebar through `better_ui::window_chrome`; Mutter gives a Wayland client none.
- Don't use fixed card or button widths that can clip translated content.
- Don't put a shadow, a gradient, or a radius larger than 4px on anything that sits in the page.
- Don't use the accent color for more than the primary action, the selection indicator, and focus.
- Don't hardcode a component's name, purpose, or icon by ID. Those come from the manifest, or from a shipped translation for a first-party component.
