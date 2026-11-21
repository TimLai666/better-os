# Ticket 275 — Choose which columns Better Monitor's tables show

**Blocked by:** none

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks.

The model has 22 columns and the Details table shows a fixed set
(`crates/monitor-gui/src/tables.rs`, `detail_columns`). Windows Task Manager
has "Select columns".

## What to build

- A column chooser for the Processes and Details tables (right-click on the
  header, and from the page's menu), with the choice, order and widths kept
  as the person's own setting (named in the manifest's `paths`), a way back
  to the default set, and keyboard access.

## Done when

- Tests for the stored choice, an unknown column in an old file, the default.
  Seen at 1600 and 960 wide, both languages.
