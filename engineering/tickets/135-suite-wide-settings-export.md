# Ticket 135 — Export and import every Better OS setting in one place

**Blocked by:** none

## Why

The owner asked on 2026-11-16 why the Touchpad page has its own "Export
gestures" (匯出手勢設定): export and import should cover the whole of
Better OS at once, not one page.

## What to build

- One Export / Import in Better Manager's Settings that writes a single file
  holding every component's own settings (the personal-data allowlist from
  ticket 85 says what each component owns), with each part labelled by
  component and schema version.
- Import shows what the file holds and which components it would change,
  lets the person untick parts, and applies through each component's own
  code path (no raw file copying), with undo.
- History, recent lists, trust lists and caches are not exported unless the
  person asks for them; nothing machine-specific (device ids) is applied to
  another machine without saying so.
- The Touchpad page's own export either becomes a shortcut to this with only
  Touchpad ticked, or goes; decide and record in the spec.

## Done when

- Round-trip tests per component; an older file imports; a file from a newer
  version is refused with the reason.
