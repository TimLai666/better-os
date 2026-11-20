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

## Notes

- Arrow: `better_ui::fluent::list::DetailsTable` drew the chevron as a bare
  glyph with no handler. It now gives a tree row's chevron a 24px by
  row-height hit area that opens or closes the row on one press, without
  selecting or invoking it, and takes Right and Left on the selected row
  through `DetailsTable::on_expand` and `list::tree_step`; a double click
  still invokes. Better Monitor uses it on the Processes page (an app's
  processes) and on the Details tree, which could not be collapsed at all
  before: `ProcessTableModel` now keeps the closed PIDs, the window carries
  them through a background round and an update restart. A group closed
  while a round was being built no longer springs open for one round
  (`TableSettings::apply_apps` sets exactly the open groups).
- Other trees: Better Files' Columns view shows a chevron on a folder row,
  but the whole row is the target, so it has no such fault; Better Git and
  Better Manager draw no tree rows (their expanders are `SettingsExpander`
  cards whose whole header toggles). No change there.
- Notes: the processor card's 「等待執行（1 分鐘平均）」 row keeps its name and its
  note now sits directly under it (`card_notes::cpu_facts`). The graphics
  card's note, shown when its utilization is added up from programs' use,
  explained the Utilization figure without naming it; the figure and the
  note now both carry `*`, and the note names Utilization. Memory's note
  already names Cached and Available; the battery and startup notes are
  about the whole section.
- Tests: `better-ui` `one_press_on_a_tree_rows_arrow_opens_and_closes_it`,
  `a_double_click_on_a_tree_row_still_invokes_it`,
  `right_and_left_open_and_close_the_selected_tree_row`,
  `right_opens_a_closed_tree_row_and_left_closes_an_open_one`;
  `monitor-views` `a_closed_tree_row_hides_its_whole_branch_and_opens_again`,
  `closing_the_root_hides_every_descendant_and_a_gone_pid_is_forgotten`;
  `monitor-gui` `the_load_note_sits_under_the_load_row_in_both_languages`,
  `a_graphics_figure_and_its_note_carry_the_same_mark`,
  `what_was_closed_while_a_round_was_built_stays_closed`.
- Seen in a headless sway with real pointer presses and keys, zh-TW and
  English, in `/home/tim/Developer/better-os-evidence/145/`: one press in
  either corner of the arrow's hit area opens and closes a group, Right and
  Left on the selected group, a double click still toggling it, the Details
  tree closing a branch, and the processor card's note under its row. The
  marked graphics figure was not on screen: this machine's card reports its
  utilization from the driver, so the note does not appear; the mark is held
  by the test above.
- Review fixes:
  - Closing a row moves a selection inside it onto the closed row, on the
    Details tree (`ProcessTableModel::selection_after_closing`) and on the
    Processes page (`AppsModel::group_holds`), so End task, End process tree,
    and Properties never act on a hidden row.
  - A search shows every ancestor of a match open, even one the person
    closed; what they close during a search is kept apart and forgotten when
    the search changes, so the tree is as they left it once it is cleared.
    The Processes page has no such case: its search matches application
    names only, so a closed group never hides a match.
  - Closed processes are remembered by PID and start token
    (`ProcessKey`, the same token a process action checks), so a new process
    on a closed PID opens as new. The update hand-off stores them as
    `closed_processes`; a 0.2.16 hand-off, which has none, still reads.
  - `DetailsTable::parents` gives `TreeView`'s other keys where a window
    says each row's parent: Right on an open row moves to its first child,
    Left on a child moves to its parent. Both Monitor tables pass it; a table
    that does not behaves as before.
  - The graphics card's marked note sits right after the figures it marks.
- Seen live (screen updates not paused), English: a selected child whose
  parent is closed with its arrow moves onto the parent, and the branch stays
  closed 3 and 7 seconds later while rows reorder
  (`en-US-7` to `en-US-10`); on the Processes page a selected process moves
  onto its group when the group is closed, still closed 4 seconds later
  (`en-US-11` to `en-US-13`). No process action was clicked.

### Checks that ran

Run in the worktree through the build lock wrapper, on 2026-11-21:

- `cargo fmt --all --check`: exit 0.
- `cargo +stable clippy -p better-ui -p monitor-views -p monitor-gui -p git-gui
  -p manager-gui -p app-chooser-gui -p fluent-gallery -p awake-gui
  -p launcher-gui -p touchpad-gui --all-targets -- -D warnings` (rustc
  1.97.1): exit 0.
- `cargo test -p better-ui -p monitor-views -p monitor-gui -p git-gui`:
  exit 0, 534 passed, 0 failed.
- Not run: clippy and tests of `files-gui`, a dependent of `better-ui`. It
  does not build on this machine: `glib-sys` finds no `glib-2.0` through
  pkg-config (GStreamer, through `files-preview`). Nothing in `files-gui`
  changed.
- `escape_gives_focus_back_to_a_toolbar_arrow_segment` was also run with the
  focus hand-back removed, and failed, so it guards the fix.
