# Ticket 270 — Every Better Monitor chart can be read without seeing it

**Blocked by:** none

## Why

Found on 2026-11-21 while auditing the old draft pull requests #18, #20, #21 and #22 (Better Monitor's first interface, July and August 2026) before closing them. Their code is not carried over: it collected in the window and called `setpriority` and `kill` there, against AGENTS.md. This is one of the things they had that `main` lacks.

Ticket 23 required that nothing be read by colour alone and that a chart
have a table alternative, and noted that the rule stops holding by itself
once charts are added. Ticket 81 added charts; none has a text form.

## What to build

- Beside every line chart (`better_ui::fluent::chart`, used in
  `crates/monitor-gui/src/pages.rs`): the current value, average, minimum,
  maximum and number of samples of what the chart shows, as text a screen
  reader reaches, in English and zh-TW; and a way to see the samples as a
  table.
- In the kit, so any window that draws a chart gets it.

## Done when

- Tests: the summary's numbers for a known series, an empty series, one
  sample; both languages. Seen in a sandbox, light and dark.
