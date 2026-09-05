# 42 — First-run layout collapse, desktop entries, icons, and window controls

**Epic:** Field defects from a real Zorin 18 install of `v0.2.2`
**User Story:** A person who installs Better OS on their own machine can find
the applications in the grid, recognise them by their icons, read the first-run
page in their own language, and close the windows they open.
**Blocked by:** none (36, 40, 41 merged)
**Status:** done

## Where these came from

Four reports from one Zorin 18 GNOME Wayland desktop running the published
`v0.2.2`, at the `zh_TW` locale. None of them is a one-off: each is one
occurrence of a class that had more instances elsewhere in the suite.

---

## Bug 1 — the first-run step list collapsed to one character per line

### What was seen

On the 首次啟動 page the left step list rendered one Chinese character per
line — 檢 / 查 / 目 / 前 / 系 / 統 stacked vertically — while the 相容性檢查
card beside it took the width it was owed. The page was unreadable in Chinese
and correct-looking in English, which is why it survived to a release.

### Root cause

`ManagerApp::bullet_row` in `crates/manager-gui/src/components.rs`. The row is
an `h_flex` holding a fixed icon well and a text column:

```rust
.child(div().size_7().flex_shrink_0() /* icon */)
.child(
    v_flex()
        .min_w_0()      // ← no grow factor
        .gap_1()
        .child(/* title */)
        .child(/* detail */),
)
```

The text column had **no flex grow factor**. Its flex basis therefore stayed
`auto`, and a nested flex column reports its min-content width for that basis.
In a language that permits a line break between any two characters, the
min-content width of a paragraph is one character. `min_w_0()` removed the
content-based minimum that would otherwise have floored it, and because nothing
asked the column to grow, it was never widened again — the row simply left the
rest of its width empty.

This was confirmed by rendering, not by reading. The window was run on the host
and captured; the before and after images are in the ticket's verification
notes below.

The house already had the correct pattern in three places — `key_value_row`,
`section_header`, and `better_ui::application_list_row` all write
`.flex_1().min_w(...)` or `.flex_1().min_w_0()` on the same shape.
`bullet_row` was the one that omitted it.

### The fix

`bullet_row`'s text column now carries `flex_1()` and an explicit
`min_w(STEP_LABEL_MIN_WIDTH)`, and the two leaf text elements carry `min_w_0()`
so they wrap inside the column rather than widening it. The enclosing row
already carries `flex_wrap`, so when the window is genuinely too narrow for two
columns at their declared minimum the columns stack and the label gets the full
width instead of being squeezed.

### The rest of the class

The same shape — a `v_flex` text column with `min_w_0()` and no grow factor,
sitting beside a `flex_shrink_0` icon inside an `h_flex` — was found in nine
more places and fixed the same way. Four of them are sidebar headers, whose
`SidebarHeader` is itself an `h_flex`, so they were the same defect wearing a
component's name:

| file | what it draws |
| --- | --- |
| `crates/manager-gui/src/components.rs` | the first-run step row, and the component list row |
| `crates/manager-gui/src/pages_defaults.rs` | the Better Defaults component row |
| `crates/manager-gui/src/pages_main.rs` | the catalog source and age line |
| `crates/manager-gui/src/shell.rs` | the sidebar header and the sidebar footer |
| `crates/monitor-gui/src/shell.rs` | the sidebar header |
| `crates/touchpad-gui/src/pages.rs` | the sidebar header and the sidebar footer |
| `crates/awake-gui/src/shell.rs` | the sidebar header |

A leaf text `div` with `min_w_0()` is **not** an instance. Those size to
max-content and shrink normally; only a nested flex column collapses. The
scan that found these is recorded in the verification notes.

### The policy the tests now hold

`crates/manager-gui/src/layout.rs` gains the geometry of the first-run row —
page padding, surface inset, column gap, column minimum, the row's icon gutter,
and `STEP_LABEL_MIN_WIDTH`, which is also the number the rendered element
carries, so the policy and the element cannot drift. `first_run_column` says
whether the two columns sit side by side or stack and how wide one is;
`step_label_width` says what the label then gets; `characters_per_line` turns
that into the number the bug report was actually about.

`crates/manager-gui/src/tests.rs` asserts, at 720/1024/1280/1920/2560 logical
pixels and at 100%, 125% and 150% scaling, in both locales, that a step label
holds at least `MIN_READABLE_CHARACTERS` on a line — and, separately, that a
column collapsed to its min-content width holds exactly one, which is the
failure state stated as a number rather than as prose.

---

## Bug 2 — installed applications did not appear in the applications grid

### What was seen

After `apt install`, nothing appeared in the GNOME applications grid.

### Two independent causes, both closed

1. **Two packages shipped no desktop entry at all.** `better-manager` and
   `better-monitor` installed a binary and nothing that named it.
   `packaging/manager/io.betteros.Manager.desktop` and
   `packaging/monitor/io.betteros.Monitor.desktop` now ship, following the
   `io.betteros.Files.desktop` precedent. The five already-published entry
   filenames were **not** renamed: their desktop file IDs are already in the
   wild and may be pinned.

2. **No icon file existed anywhere in the project.** Every entry named
   `Icon=better-<app>` and no package shipped a file any of those names
   resolved to, so even the four applications that did reach the grid drew a
   blank tile. Six original project-owned SVGs now live in `packaging/icons/`
   and install to `usr/share/icons/hicolor/scalable/apps/`. They are one
   family: a rounded square in the Better OS primary `#1f2937`, one accent
   element in the tertiary `#4f6df5`, and a glyph that stays legible at 16
   pixels — stacked packages, a pulse line, a grid with a magnifier, a folder,
   a trackpad with two contacts, and an open eye. No copied artwork.

`verify-deb.sh` now runs `desktop-file-validate` on every shipped entry when
the tool is present, and fails any entry whose `Icon=` names a file the same
package does not carry — the check that would have caught the second cause.

### A third cause, found while fixing the first two

Neither of the above is sufficient on Wayland. The compositor matches a window
to its desktop entry by the client's `app_id`, and **no Better OS window set
one**. Without it GNOME cannot find the application's icon or name for the dock
or the window switcher no matter what the package installs. Every window now
sets an `app_id` equal to its desktop entry's filename without the suffix, and
`better_ui::window_chrome::window_options` is the one place that does it.

### Icon naming

Theme names stay `better-manager`, `better-monitor`, `better-launcher`,
`better-files`, `better-touchpad`, `better-awake`, because four existing
entries already referenced exactly those. No `Icon=` line needed correcting;
the names were right and the files were missing.

### Desktop database refresh

**No postinst is needed.** `desktop-file-utils` registers
`interest-noawait /usr/share/applications` and `hicolor-icon-theme` registers
`interest-noawait /usr/share/icons/hicolor`, so dpkg refreshes both caches by
trigger after any package writes into either directory. Read from the trigger
files on the host rather than assumed. A postinst of our own would repeat the
work and would fail on a machine without those packages. Recorded beside the
container e2e's file list, which asserts the installed files rather than a
refreshed cache — that image has neither package, so no trigger fires there and
asserting a cache would test the image rather than the `.deb`.

---

## Bug 3 — the windows had no close, minimize or maximize button

### What was seen

On the GNOME Wayland session the Better OS windows had no window controls and
could not be dragged.

### Root cause

Mutter offers no server-side decorations to an `xdg-toplevel` client. A GPUI
window that draws no titlebar therefore has none, and no way to be closed,
minimized, maximized or moved. Nothing in the code was wrong; the titlebar had
simply never been written. It was invisible in development because the X11
path still gets a frame from `mutter-x11-frames`.

### The fix

`better_ui::window_chrome` is one shared bar, built on
`gpui_component::TitleBar` — which already talks to the compositor through
`start_window_move`, `zoom_window`, `minimize_window` and `remove_window`, and
already carries the hover and active states for the three controls. The wrapper
adds the application glyph, the localized window title, and the window options
every decorated window opens with, so seven windows share one bar rather than
seven near-copies.

Wired into: `manager-gui`, `monitor-gui`, `files-gui`, `touchpad-gui`,
`awake-gui`, and the `app-chooser-gui` standalone window.

### The launcher is the deliberate exception

Better Launcher gets **no** titlebar, and `overlay_window_options` records why
in code. It is a near-fullscreen overlay a person summons, uses once, and
dismisses with Escape or by launching something. A titlebar would give it a
second way to close, a drag region it should not have, and a maximize button
for a window that is already the size it wants. It still sets `app_id`, because
the shell matches a window to its icon and name by that string whether or not
the window is decorated.

Escape is untouched: the only launcher change is its `WindowOptions`, and the
`escape` arm of `LauncherOverlay::on_key` — which emits `OverlayEvent::Closed`,
whose subscriber calls `cx.quit()` — was not edited. The overlay was run on the
host and drew no titlebar, as intended. **The Escape key itself was not
pressed**: this environment has no key-injection tool and the repository has no
test that exercises that path. Someone should press it once on the real
desktop.

The app chooser's titlebar lives in its standalone `main.rs` rather than in
`Render for AppChooser`, because the same surface is drawn inside Better Files
as an overlay and an overlay must not carry a window titlebar.

---

## Verification actually run

| gate | result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo check --workspace` | clean |
| `cargo test --workspace` | 167 test targets, all ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `packaging/build-deb.sh` | eight packages built |
| `packaging/verify-deb.sh` | eight packages verified, `desktop-file-validate` ran on every entry |
| headless smoke, `ZED_HEADLESS=1`, 8s | all seven windows stayed alive |
| on-host composit run | manager-gui ran on the real Wayland session and on X11 without crashing |

The layout fix and every titlebar were confirmed by capturing the running
window on the host and looking at it, not by reading the code. The first-run
page before the fix showed the step list one character per line; after it, both
steps read normally and the compatibility card keeps its width.

## Known limits

- GPUI's `WindowOptions::icon` is **X11 only** and takes a raster image, so the
  shipped SVGs are not set as a GPUI window icon. On the Wayland session the
  user actually runs, the window icon comes from the desktop entry matched by
  `app_id`, which is now set — that is the path that fixes the reported
  problem. The titlebar itself shows the application's stock glyph.
- The layout policy tests assert the intended geometry. They would not by
  themselves catch a future removal of `flex_1` from a text column; that class
  is guarded by the shared primitives and by this ticket's record of the shape.
- `better-launcher.desktop` and `better-awake.desktop` each draw a
  `desktop-file-validate` hint for carrying two main categories. Both predate
  this ticket and neither is an error.
