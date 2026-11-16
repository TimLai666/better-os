# Ticket 145 — Better Monitor: expand a process with its arrow; a load figure people understand

**Blocked by:** none

## Why

The owner reported on 2026-11-16 (Better Monitor 0.2.16):

1. On the Processes page a process with children expands only on a double
   click; its disclosure arrow does nothing when clicked.
2. Under CPU, the row 「等待執行（1 分鐘平均）0.95」 and its note 「平均有多少
   工作正在執行或等著用處理器。只要低於邏輯處理器的數量，就沒有工作等很久。」
   cannot be understood.

## What to build

- **Arrow**: a single click on the disclosure arrow expands or collapses the
  row (its whole hit area, not only the glyph — same rule as ticket 130);
  double click on the row keeps its current meaning; keyboard Right/Left
  expand and collapse. Check every tree in the suite for the same fault
  (Better Git, Better Files columns, Manager) and fix at the shared source.
- **Load**: show the load average as how busy the processor is, with the raw
  number as detail. For example 「處理器忙碌程度：約 4%（1 分鐘平均）」 with
  the line 「24 個邏輯處理器中，平均約 1 個在工作或排隊等待」, and the raw
  load average (0.95 / 1, 5, 15 minutes) in the details tooltip. Above 100%
  it says work is waiting: 「有工作在排隊等處理器」. Wording in both
  languages through the monitor's copy, defined in `monitor-views` (no GPUI)
  with tests; never a figure that reads as a fact when the load is unknown.

## Done when

- Tests: arrow click toggles, keyboard toggles, double click unchanged; load
  wording for idle, busy, overloaded and unknown.
- Screens in zh-TW and English.
