# Ticket 82 — Better Launcher and the app chooser, rebuilt in Fluent and complete

**Blocked by:** 80 · **Status:** ready

## Why

The audit found no real application icons anywhere (letter tiles), desktop
file names under every app, a search for "cal" returning 22 results including
Foot and Chrome, uneven tiles, no pinned or frequent apps, and a hard-coded
dark theme. The app chooser opens a full-size window to pick one app, shows
the same letter tiles and file names, uses jargon chips, jumps its layout on
selection, and is hard-coded dark.

## What to build

Both on `better_ui::fluent` (ticket 80) with `AppIcon` real icons and
`theme::Appearance`.

Better Launcher is complete when it matches the Windows 11 Start menu:
pinned apps (pin, unpin, reorder by drag), recommended / recently used,
all apps A–Z with letter jump, search ranked by name prefix first, then word
prefix, then keywords and generic name, then fuzzy, with a calculator and
"open file/folder" results; right-click menu (open, pin, uninstall through
Better Manager when it manages that app, open file location, run in
terminal for terminal apps); keyboard throughout.

The app chooser is complete when it is a Fluent content dialog sized to its
content: the recommended apps for the file type with real icons, "other apps"
collapsed, "always use this app" check box, a search, and the plain-language
reason an app is or is not suggested shown only on demand.

## Acceptance

- [ ] Every audit finding L1–L6 and C1–C7 fixed, each shown by a screenshot.
- [ ] Search ranking has unit tests with the "cal" case: Calendar and
      Calculator first, no terminal in the first five.
- [ ] Light and dark, English and zh-TW, screenshotted and looked at.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.
