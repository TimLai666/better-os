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
- **Load note** (owner, 2026-11-16): keep the row 「等待執行（1 分鐘平均）」
  as it is; the problem is that the note at the bottom of the card does not
  say which row it explains. Tie the note to its row: mark the row and the
  note with the same marker (e.g. 「等待執行（1 分鐘平均）*」 and the note
  starting with 「*」), or put the note directly under that row as secondary
  text. Apply the same rule to every card in Better Monitor whose note
  explains one row, so a note always names or marks the row it belongs to.

## Done when

- Tests: arrow click toggles, keyboard toggles, double click unchanged; the note is
  tied to its row.
- Screens in zh-TW and English.
