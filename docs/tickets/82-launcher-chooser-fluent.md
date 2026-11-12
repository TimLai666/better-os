# Ticket 82 — Better Launcher and the app chooser, rebuilt in Fluent and complete

**Blocked by:** 80 · **Status:** merged, not released

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

## What was built

### Better Launcher

- **A Start menu, not a full-screen grid.** The window asks for 640 by 720
  (`launcher_gui::overlay_size`) and draws a 640 by 720 panel on the flyout
  fill; a tiling compositor that stretches the window gets the panel centred
  in it. With nothing typed it shows the Start page: **Pinned** (a six-column
  grid of 96 by 84 tiles, a 32px icon over two lines of Caption, three rows
  visible) with an **All apps ›** button, and **Recommended** (the six apps
  opened most recently, two columns, with "5 minutes ago" / "Yesterday").
  **All apps** lists every app under `#`, A–Z, and Other headers; a header
  opens a grid of letters (letters with no apps disabled) that jumps to its
  section and puts it at the top. Typing replaces either page with results
  and clearing the box brings the page back, so browsing and searching are
  still one query-driven screen (Issue #2).
- **Search, in Windows' order.** `launcher-core`'s matcher now ranks by a
  `Tier` before anything else: the name beginning with the query, then a
  later word of the name, then the start of a keyword, the generic name, or
  the program name, then fuzzy matches. Letters merely inside a keyword, a
  program name, or the name in another language are not results at all,
  which is what put Foot, Terminal, and Chrome under "cal". Only the name in
  the active locale is matched loosely. `launcher_gui::ranking` puts a
  **calculator** result first for arithmetic (`calculator.rs`: `+ - * / ^`,
  `×`, `÷`, parentheses; a lone number or `0.2.9` is not a sum) and **files
  and folders** for a query starting with `/` or `~` (the path itself, then
  what its folder holds that begins with the typed part, folders first,
  hidden ones once a `.` is typed). The selected result is shown large beside
  the list with its actions, as Windows does.
- **Pins and history** (`pins.rs`). Pinned apps live in
  `$XDG_CONFIG_HOME/better-os/launcher/pinned` and launch history in
  `$XDG_STATE_HOME/better-os/launcher/recent`, both desktop IDs only, written
  through a renamed temporary file. A machine that has never arranged its
  pins starts from one app per category (web browser, file manager, mail,
  word processor, spreadsheet, text editor, terminal, calculator, calendar,
  photos, music player), chosen by what apps declare, never by ID; an emptied
  list stays empty. Pins are reordered by dragging a tile onto another
  (`better_ui::fluent::list::GridTile::reorderable`) or with "Move to front".
  History feeds Recommended only; search ranking still does not read it.
- **Right-click menu, and the same on Shift+F10 or the Menu key**: Open (or
  "Run in terminal" for an entry with `Terminal=true`, which is what Open
  does for it), Pin to Start / Unpin from Start, Move to front (pinned), Remove
  from list (Recommended), Open file location (the folder of the desktop
  entry, through `xdg-open`), and Uninstall for apps whose desktop entry a
  Better OS manifest installs (`launcher_platform::ManagedApplications`,
  read from the eight shipped manifests). Uninstall opens Better Manager; no
  removal happens in the launcher. Better Manager itself is not offered.
- **Keyboard throughout.** The search box keeps focus; the launcher takes
  Up and Down always, Left and Right only inside the pinned and Recommended
  grids (otherwise they move the text cursor), Home, End, Page Up and Page
  Down in lists, Enter for the highlighted item or the best match, Escape to
  close the letter grid, then leave All apps, then close. The highlighted
  item draws WinUI's focus ring.
- **Real icons and the stored appearance.** Every app's icon is resolved off
  the render thread through the desktop's own icon theme
  (`better_ui::icons::IconSearch::from_environment` now reads GTK's
  `gtk-icon-theme-name`, follows `Inherits=`, and searches each theme's
  `index.theme` application directories, 48px and up first). The window
  applies the appearance stored in `better-os/launcher/appearance`: dark by
  default (ADR 0004), and the palette button at the bottom offers Dark,
  Light, and Use system setting, which follows the desktop.

### The app chooser

- A **WinUI content dialog** (`better_ui::fluent::dialog::ContentDialog`,
  inline): "Select an app to open “notes.txt”", a search box, the
  recommended apps with their own icons (the current default first, captioned
  "Opens this kind of file now"), an **Other apps (N)** row that folds the
  rest away, an **Always use this app to open .txt files** check box, and
  Open (accent) and Cancel. The standalone window is a dialog window (the
  fluent titlebar's new close-only form) that resizes to what the dialog
  holds (`app_chooser_gui::dialog_height`), up to 640px, after which the
  list scrolls.
- **No jargon on the rows.** No MIME type, no desktop file name, no
  packaging chips. Each selected row has an ⓘ button that shows, on demand,
  one plain sentence for why the app is listed (`Reason`, `reason_text`).
- **No layout jump.** The dialog opens with the default or the first
  recommended app already selected, so selecting never inserts or removes a
  line above the list; selection is the list fill and the 3px accent bar.
  Up and Down from the search box walk the list (onto "Other apps" opens it),
  Enter opens, Escape cancels.
- The embedded API is unchanged (`AppChooser::new`, `ChooserEvent`,
  `ChooserMode`, `ChooserTarget`), so Better Files still compiles against it;
  the choose-executable mode is restyled with the same controls.

### Added to `better_ui::fluent` (with tests)

`ListRow` (one `ListViewItem` with an app icon or glyph, a trailing element,
and an expandable detail line), `GridTile` (a Start-menu `GridViewItem`,
draggable within a group) and `move_item`, `row_icon_size`, a trailing icon
on `Button`, `ContextMenu::return_focus`, a dialog form of `TitleBar`
(`caption_buttons`), and `AppIcon::element` to draw an app icon in any
palette. Two kit bugs were fixed: a menu or flyout panel let a click through
to whatever it floated over (choosing "Move to front" also launched the tile
under it), and now occludes.

### Not done, and not verified

- Only a headless sway session was used. No GNOME desktop: window
  placement, the 640 by 720 size request honoured by Mutter, and the
  clipboard after the launcher exits (a copied sum lives with the process on
  Wayland unless the compositor keeps it; Mutter is believed to) are
  unverified. The sandbox was shared with another worker's window, so the
  chooser was floated with `swaymsg` to show its real size.
- Not exercised by hand: Open file location and opening a path (both go to
  `xdg-open`, tested as one argument vector), Uninstall opening Better
  Manager, Run in terminal, picking a program in the choose-executable mode (only
  browsing was looked at), and the chooser embedded in Better Files.
- Windows' Start menu pages its pins; this one scrolls them. There is no
  account or power button, and Recommended lists apps, not recent files.
  Calculator results are copied to the clipboard rather than opening
  Calculator. Letter headers group non-Latin names under "Other" rather than
  by Bopomofo or strokes.
- App names stay in English where a desktop entry carries no zh_TW
  translation; that is the entry's data.
- `AGENTS.md` still calls Better Launcher a near-fullscreen overlay; this
  ticket was not allowed to edit it.

### Screenshots

Headless sway (wayhand sandbox, 1920×1080), `HOME` and the XDG directories in
a scratch directory, cropped to the window:
`/tmp/claude-1000/-home-tim-Developer-better-os/3b559518-c3b2-4104-bc77-d1c2e845c29a/scratchpad/shots-82/`

- Launcher: `launcher-01-start-dark-en.png` (L1, L4, L6),
  `launcher-02-search-cal-dark-en.png` (L2, L3),
  `launcher-03-keyboard-down-shift-f10-menu-dark-en.png`,
  `launcher-04-calculator-dark-en.png`, `launcher-05-open-folder-dark-en.png`,
  `launcher-06-dragged-foot-first-and-pinned-menu-with-uninstall-dark-en.png`,
  `launcher-07-start-pinned-and-recommended-dark-en.png` (L6),
  `launcher-08-all-apps-a-z-dark-en.png`, `launcher-09-letter-jump-grid-dark-en.png`,
  `launcher-10-start-dark-zh-TW.png`,
  `launcher-11-start-light-zh-TW-after-choosing-light.png` (L5),
  `launcher-12-search-cal-keyboard-light-zh-TW.png`,
  `launcher-13-start-light-en.png`, `launcher-14-all-apps-light-en.png`.
- Chooser: `chooser-01-dialog-sized-to-content-dark-en.png` (C1, C2, C3),
  `chooser-02-why-on-demand-dark-en.png`,
  `chooser-03-keyboard-down-selects-no-layout-jump-dark-en.png` (C4, C5),
  `chooser-04-search-dark-en.png`,
  `chooser-05-other-apps-expanded-light-zh-TW.png` (C6, C7),
  `chooser-06-why-and-always-checked-light-zh-TW.png`,
  `chooser-07-always-checked-dark-zh-TW.png` (then Open wrote
  `text/plain=org.gnome.TextEditor.desktop` to the scratch `mimeapps.list`
  with a rollback record and opened the file),
  `chooser-08-current-default-first-light-en.png`,
  `chooser-09-choose-a-program-light-en.png` (the choose-executable mode,
  `/usr/bin` opened).

They live in a session scratch directory; copy them somewhere durable before
it is cleaned.

### Independent review, and what it changed

An independent review of this ticket found twelve problems. Each is fixed,
and each fix has a test that fails without it (the tile edge, which cannot be
inspected headless, is checked against the control's source).

1. **Calculator display.** `12345678*1234567` showed
   `15241566651426.001953125` and `10^300` showed `inf`, because every value
   was rounded through `value * 1e10`. Results now keep fifteen significant
   digits, switch to `1e300`-style notation outside that range, and a result
   that is not a finite number (`10^400`) is not offered.
2. **Open with nothing visible.** A search that matched nothing left the
   earlier, now hidden app selected, and Open (with "Always use" on, writing
   it as the default) still acted on it. The selection now follows what the
   list shows (`visible_selection`), is cleared when nothing matches, and the
   dialog's primary button is disabled and ignores Enter
   (`ContentDialog::primary_enabled`, `DialogButtons::key_with`).
3. **Path queries on the render thread.** Every keystroke of a `/` or `~`
   query ran `stat` and `read_dir` in the frame, so a dead network mount
   froze the launcher. Typing now reads no filesystem: the model asks for a
   read (`path_request`), a separate thread does it with a 750 ms deadline
   (`path_hits_within`), and the answer is dropped if the query changed
   since (a generation counter, `apply_path_hits`).
4. **The desktop's icon theme.** GNOME and Zorin keep the theme in
   `org.gnome.desktop.interface icon-theme`, not in GTK's `settings.ini`, so
   apps whose icon only that theme has fell back to letters. The theme is now
   read through `gsettings` first and `settings.ini` second
   (`IconThemeSource`, injected in tests, which never read dconf), and every
   chain ends with Adwaita before hicolor.
5. **Atomic writes.** Pins and history were written through a fixed
   `pinned.tmp` and never synced. They now go through a uniquely named
   temporary file in the same folder, `sync_all`, and an atomic rename.
6. **An unreadable pins file.** A pins file that was not UTF-8 or could not
   be read was treated as never arranged and later overwritten with the
   starting set. It is now read lossily; only a missing file means never
   arranged; an unreadable one is left alone, the starting set is shown for
   that session only, and the launcher says so.
7. **Names in other languages.** An entry's Catalan or Hindi name ranked as
   a name. Only the name in the active locale and the untranslated name get
   the name tiers; other translations (`FieldKind::Translation`) rank with
   keywords.
8. **Theme directories outside the theme.** An `index.theme` `Directories=`
   entry that was absolute or contained `..` was searched. Such entries are
   now ignored (`safe_subdirectory`).
9. **Uninstall named no component.** "Uninstall" opened Better Manager on
   its usual page. Better Manager now takes `--component <id>` (shown in its
   `--help`) and opens that component's page, unless an operation is running
   or first-run setup is showing; the launcher runs the manager's own
   desktop-entry program with that argument (`open_manager_component`).
10. **Key-path tests touched the machine.** The headless key tests read the
    real appearance file, application folders, and icon themes, and started
    a watcher on real folders. The launcher now takes its sources
    (`OverlaySources`: session folders, Start store, appearance path, icon
    search, path filesystem, watching on or off), and the tests give it
    folders that do not exist, no store, no appearance file, no icon
    directories, no filesystem, and no watcher.
11. **The check box named the wrong kind of file.** For `a.tar.gz` it said
    ".gz files" while writing the default for compressed tar archives. It now
    names the type by shared-mime-info's own description in the person's
    language ("Always use this app for Tar archive (gzip-compressed)",
    "一律使用這個應用程式開啟「Tar 封存檔 (gzip 壓縮)」"), and falls back to
    the extension only when no description in that language exists.
12. **A dragged tile moved the grid.** Lighting the drop target added a 1px
    border and grew the tile by 2px. The edge is now always there and
    transparent, and only its colour changes.

Screenshots after the review, in the same folder:
`review-chooser-no-results-open-disabled-dark-en.png`,
`review-chooser-tar-gz-described-light-en.png`,
`review-chooser-tar-gz-described-dark-zh-TW.png`,
`review-launcher-icons-from-zorinblue-light-gsettings-dark-en.png`,
`review-launcher-all-apps-icons-from-zorinblue-light-light-en.png`.
The icon screenshots read the theme through `gsettings` from a scratch
`GSETTINGS_BACKEND=keyfile` setting (`icon-theme='ZorinBlue-Light'`), with no
GTK `settings.ini` in the scratch home.

Still not verified after the review: a dead network mount in practice (the
deadline is tested with a filesystem stub that blocks), Better Manager opened
on a component from the launcher on a desktop where it is installed, and
`gsettings` on a GNOME session rather than a scratch keyfile.

