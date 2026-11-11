# Better Git: the first window

What the window looks like. What it does, and its exact states, are in
`docs/tickets/77-better-git-first-window.md`; the visual rules are in
`DESIGN.md`. Where the two and this file differ, the ticket decides behaviour
and `DESIGN.md` decides the look.

## Direction

Practical. The reference is File Explorer and Settings on Windows 11: a window
that is all working surface, with no hero, no illustration, and no ornament.
The owner chose WinUI for the whole suite, so there is no film or style
exploration for this window.

## Layout

A fixed three-part window, minimum 960 × 600.

```
┌──────────────────────────────────────────────────────────────┐
│ titlebar: Better Git                               – □ ×     │ 32
├──────────────────────────────────────────────────────────────┤
│ [repo ▾ better-os]   [branch  main]            [Open…]       │ 48  command bar
├───────────────────────┬──────────────────────────────────────┤
│ Changes (4)           │ crates/git-core/src/status.rs        │ 40  diff header
│ ☑ M status.rs         │ ┌──────────────────────────────────┐ │
│ ☐ A diff.rs           │ │ 12  12   context line            │ │
│ ☐ ? notes.txt         │ │ 13     - removed line            │ │
│ ☐ D old.rs            │ │     13 + added line              │ │
│                       │ └──────────────────────────────────┘ │
├───────────────────────┤                                      │
│ [Summary            ] │                                      │
│ [Description        ] │                                      │
│ [   Commit to main  ] │                                      │
└───────────────────────┴──────────────────────────────────────┘
        320                          flexible
```

- **Command bar** (48 tall, window background): the repository as a standard
  button with its name in Body Strong, the current branch as text with a branch
  icon (not a button in this ticket; it becomes one in ticket 78), and "Open…"
  at the trailing edge.
- **Changes pane** (320 wide, card fill, 1px stroke on its trailing edge): a
  36px header with "Changes" in Body Strong, the count in secondary text, and a
  checkbox that stages or unstages everything. Below it the list, then the
  commit form pinned to the bottom.
- **Diff pane** (flexible, window background): a 40px header with the selected
  file's path, then the diff in one card.

## Changes list

Rows are 36px. Each row, leading to trailing: a checkbox (checked when the
file is staged, indeterminate when it is staged and also changed again), a 16px
state letter in a 4px-radius square, the file name in Body, and the parent
folder in Caption secondary text, truncated at the leading end.

| State | Letter | Color role |
| --- | --- | --- |
| Modified | M | Caution |
| Added, untracked | A | Success |
| Deleted | D | Critical |
| Renamed | R | Accent |
| Conflicted | ! | Critical, and the row shows "Conflicted" as its caption |

The selected row has the subtle fill and the 3px accent bar. Clicking the
checkbox stages or unstages and does not change the selection. Clicking the row
selects it and shows its diff.

## Diff

Monospace 13/20 in a card with no padding. Each line has two right-aligned
line-number columns in secondary text (old, new), a one-character marker, and
the text. A hunk header is its own row in secondary text on the card fill.

| Line | Row fill | Marker |
| --- | --- | --- |
| Added | Success at 12% | `+` |
| Removed | Critical at 12% | `-` |
| Context | none | none |

Long lines scroll horizontally with the card; they do not wrap. The pane says
"Binary file" for a binary diff, "No changes" for a file whose content did not
change, and "Select a file to see its changes" when nothing is selected, each
centered in Body secondary text.

## Commit form

Two fields and one button, 12px apart, 16px padding. Summary is a single-line
field with placeholder "Summary (required)". Description is a three-line field
with placeholder "Description". The button is the window's one accent button,
full width, labelled "Commit to <branch>". It is disabled, with the control
fill and secondary text, while nothing is staged or the summary is empty.

## States

- **No repository open**: the content area shows one centered column: Subtitle
  "Open a repository", Body secondary "Choose a folder that contains a Git
  repository.", and an accent "Open…" button. Recent repositories, when there
  are any, are a list of 36px rows under it.
- **Failure** (a commit git refused, a folder that is not a repository, git not
  installed): an info bar under the command bar in the Critical role, with
  git's own message as its text and a close button.
- **Clean working tree**: the changes pane shows "No changes" centered in
  secondary text, and the commit form stays in place, disabled.
- **Loading** a diff or a status: the previous content stays until the new one
  arrives. No spinner for anything that takes under 200ms.

## Depth, rhythm, shape

- Depth: two fills. The window background behind the diff, the card fill for
  the changes pane and the diff card. No shadow anywhere in this window.
- Shape: 4px on rows' highlights, checkboxes, fields, buttons, the state
  letter, and the diff card. Nothing is 8px, because nothing floats.
- Motion: none beyond the control library's own hover and press. No reveal
  animations.

## Copy

Every string is in `git-gui`'s `i18n` module in zh-TW and English. The zh-TW
strings: 變更、摘要（必填）、說明、提交到 <branch>、開啟…、開啟儲存庫、
選擇含有 Git 儲存庫的資料夾。、沒有變更、選擇檔案以檢視變更、二進位檔、衝突.
