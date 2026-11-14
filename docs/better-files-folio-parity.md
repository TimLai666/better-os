# Better Files and folio: the feature inventory

Ticket 93 asks Better Files to do everything `omacom/folio` does. This file is
the list, read from folio's source at commit `ac9d14f` (2026-11-12, MIT). Every
line names the folio file and line it was read from, and is ticked only when a
test or a screenshot proves Better Files does it. An unticked line is not built
yet; its note says what is missing.

Paths under `folio:` are in the folio repository. Screenshots are in the
ticket's screenshot folder (`shots-93/`, listed in ticket 93's What was built).
Tests are named by file and function.

Status at the end of the fifth ticket-93 pass: **108 of 113 lines ticked**, 5 deliberately different (marked `[~]`, with the reason, each accepted by the owner), 0 not built.

## 1. Views and zoom

- [x] Grid of tiles with the file's real type icon — folio: `README.md:7`,
      `src/FileTile.qml`. Themed icons from the icon theme
      (`views.rs` `IconCache`, `content.rs` `icon_candidates`);
      shot `01-grid-960-dark-en.png`.
- [x] List with name, size, date — folio: `README.md:7`, `src/FolderList.qml`,
      `src/FileRow.qml`. Shot `04-list-readable-types-960-light-en.png`.
- [x] Columns step aside when the list is narrow — folio: `plans/folio.md:76`.
      `content::visible_columns`; test `tests/columns.rs`.
- [x] Finder-style columns view (Ctrl+3) — folio: `README.md:7`,
      `src/FolderColumns.qml`. Up to three folders above this one (from home,
      or from `/`), each with the next one along selected; this folder; and
      what the selected folder holds. A click opens a folder, or a file's
      folder with the file selected. `columns.rs`, tests
      `tests/columns_view.rs`; shot `24-columns-view-1600-light-en.png`.
- [x] Ctrl+1 / Ctrl+2 switch grid and list — folio: `src/FolderPane.qml:186-187`.
      Test `tests/shortcuts.rs` `the_keys_on_the_sheet_do_what_it_says`.
- [x] Zoom bigger, smaller, normal (Ctrl+= / Ctrl+- / Ctrl+0) — folio:
      `src/FolderPane.qml:189-191`. Same test (`NormalSize`), View menu.
- [x] Ctrl+scroll zooms — folio: `src/HelpOverlay.qml:50`. Caught before the
      list scrolls (`render.rs`), one size a notch: `prefs::zoom_step`, test
      `ctrl_wheel_up_is_larger_down_is_smaller_and_stops_at_the_ends`.
- [x] Grid tiles show size and date under the name — folio:
      `plans/folio.md:66`. `content::tile_caption` (a folder shows its day;
      the item count is not counted, so a large folder never costs a
      listing to draw a tile); test
      `a_tile_says_a_files_size_and_day_and_a_folders_day`; shot
      `22-details-pane-share-menu-1600-light-en.png`.
- [x] The view, zoom, sort and hidden setting are remembered (owner's
      addition; folio forgets them) — one file under `$XDG_STATE_HOME`,
      a default plus an override per changed folder, nothing written into
      folders, overrides for missing folders dropped. Tests
      `tests/settings_store.rs` (11), `src/tests.rs`
      `a_view_is_remembered_for_its_own_folder_and_the_rest_keep_the_default`;
      shot `06-…-reopened-last-folder-…` (list view and light kept).
- [x] The last folder is reopened — owner's addition. `app.rs` start
      location; same test; shot 06.

## 2. Navigation, address bar and sidebar

- [x] Breadcrumb path bar — folio: `src/PathBar.qml`. `pathbar.rs`,
      tests `tests/pathbar.rs` (5); shots 01–08.
- [x] Ctrl+L / Alt+D type a path — folio: `src/FolderPane.qml:182`.
      Click on the address box or the keys turn it into a field;
      test `tests/shortcuts.rs`.
- [x] Tab completes folder names in the path field — folio: `plans/folio.md:74`.
      `pathbar::complete_path` (one match completes with `/`, several as far
      as they agree, `~` is home, case first then without); tests in
      `tests/pathbar.rs`. Not screenshotted: the helper cannot type.
- [x] A server address (`sftp://host/`, `smb://`, `ftp://`, `dav(s)://`,
      `afp://`, `nfs://`) in the path field — folio: `src/PathBar.qml:77`.
      `network.rs`: `gio mount` (arguments as a list, 30 s timeout), then the
      folder `gio info` names; tests `tests/network.rs` and
      `a_server_address_asks_for_a_password_then_opens_the_mounted_folder`
      (a fake server). Not tried against a real server.
- [x] Backspace / Alt+Up go to the parent; Alt+Left / Alt+Right back and
      forward — folio: `src/FolderPane.qml:193-195`. Existing key table,
      `src/tests.rs` `every_documented_shortcut_maps_to_its_command`.
- [x] Mouse back / forward buttons — folio: `plans/folio.md:80`. On the
      window's root (`render.rs`). Not exercised: the virtual pointer only
      sends left and right buttons.
- [x] Alt+Home goes home — folio: `src/FolderPane.qml:196`. `tests/shortcuts.rs`.
- [x] F5 reloads — folio: `src/FolderPane.qml:201`. Existing.
- [x] Sidebar lists the home folders that exist — folio: `src/Sidebar.qml:280`,
      `plans/folio.md:78`. `sidebar::shown_rows`, test `tests/sidebar.rs`;
      shot `03-nav-open-background-menu-960-dark-en.png`.
- [x] Pinned folders stored as GTK bookmarks, shared with file dialogs —
      folio: `src/Sidebar.qml:306`, `plans/folio.md:78`. Existing
      `bookmarks.rs`; shot 03 ("Favorites").
- [x] Pin by drag onto the sidebar — folio: `src/Sidebar.qml:348`. The whole
      navigation pane takes the drop (`shell.rs` `navigation`). Not
      exercised by pointer in the sandbox.
- [x] Pin / unpin with Ctrl+P — folio: `src/FolderPane.qml:185`.
      `session::toggle_pin`; test `tests/shortcuts.rs`.
- [x] Unpin, rename, reorder a pin from its right-click menu — folio:
      `src/Sidebar.qml:333`. `context_menu::sidebar_menu`; tests
      `tests/context_menu.rs`; shot `06-pinned-folder-menu-…-zhTW.png`.
- [x] Recents (GTK `recently-used.xbel`) — folio: `src/Sidebar.qml:376`,
      `plans/folio.md:69`. `files_platform::recent` (read only, local files
      that still exist, each name once), listed by the reader as
      `Location::Recent`, a "Recent" place in the pane, newest first. Tests
      in `recent.rs` and `reader.rs`
      `recents_lists_the_shared_list_newest_first`; shot
      `12-recents-1600-light-en.png`. Newest is by the file's modified time,
      not the list's visit time.
- [x] Trash in the sidebar — folio: `src/Sidebar.qml:376`. Existing; shot 03.
- [x] Dropbox folder with its sync status — folio: `plans/folio.md:78`,
      `plugins/dropbox/status`. Shown when `~/Dropbox` and the `dropbox`
      command are there, with `dropbox filestatus`'s word as its caption
      (`share.rs`, refreshed every minute off the window's thread); parsing
      tested in `tests/share.rs`. Not seen: Dropbox is not installed here.
- [x] Tailnet machines in the sidebar, opened over SFTP — folio:
      `README.md:9`, `plugins/tailscale/machines`. From `tailscale status
      --json`, online first; a click mounts `sftp://<machine>/` through GVfs.
      Tests in `tests/share.rs`; shot `22-…` shows this machine's real
      tailnet. Opening one was not tried (it would reach a real machine).
- [x] The sidebar never disappears; it collapses to icons on a narrow window
      — owner audit F2; WinUI `NavigationView` compact mode. Shot 01 (960px,
      48px pane), shot 03 (opened with ☰).
- [x] Tab / Shift+Tab move between sidebar and files — folio:
      `src/Main.qml:119-120`. The pane is one tab stop (kit
      `NavigationView`); not separately screenshotted.
- [x] Split view (Ctrl+Alt+N, Ctrl+W closes the right pane) — folio:
      `README.md:11`, `src/Main.qml:114-115`. The right pane is a second
      Better Files of its own (tabs, history, selection) in the same window
      and process; also in `…`. Key test in `tests/shortcuts.rs`; shot
      `25-split-view-1600-light-en.png`. Each half still lays its command
      bar and columns out as if it had the whole width, so a half can clip;
      the keyboard and clicks work.
- [x] New window (Ctrl+N) in the running process — folio:
      `src/FolderPane.qml:180`. `app::open_window_at`, also in `…`; opens on
      the same folder and shares the one job engine. Key test in
      `tests/shortcuts.rs`; shot `16-new-window-1600-light-en.png` (sway
      tiles the two). Each window keeps its own undo history; folio's is
      shared between windows.
- [x] `better-files <file>` opens its folder with the file selected — folio:
      `plans/folio.md:82`. Existing `launch.rs` tests.
- [x] `org.freedesktop.FileManager1` "Show in folder" — folio:
      `src/filemanager1.cpp`. `filemanager1.rs`: `ShowFolders`, `ShowItems`,
      `ShowItemProperties`; each opens a window in this process, the item
      selected. Taken only when no other file manager owns the name. URI
      handling tested in `tests/filemanager1.rs`; the bus call itself was not
      exercised.

## 3. Selection

- [x] Shift+click / Shift+arrows select a range — folio:
      `src/HelpOverlay.qml:37`. Existing `content.rs` tests.
- [x] Alt+click (and Ctrl+click) add or remove one — folio:
      `src/HelpOverlay.qml:38`. `views.rs` `attach_row_handlers`.
- [x] Rubber-band selection on empty space — folio: `src/HelpOverlay.qml:39`,
      `src/Selecting.qml`. `content::band_hits`, tests `tests/rubber_band.rs`
      (5); Ctrl adds to the selection; shot
      `11-rubber-band-1600-light-en.png`. Dragging past the edge does not
      scroll the list yet.
- [x] Ctrl+A selects everything — folio: `src/FolderPane.qml:151`. Existing.
- [x] A selected item is filled, with WinUI's accent bar, never a white
      outline — owner audit F10. Shot `08-list-selected-fill-bar-960-light-zhTW.png`.

## 4. File operations

- [x] Copy, cut, paste, interoperable clipboard — folio:
      `src/FolderPane.qml:138-142`. Existing jobs; command bar shot 02.
- [x] Ctrl+Shift+C copies the path as text — folio: `src/FolderPane.qml:144`.
      `session::path_text`, `app::copy_path_text`; test `tests/shortcuts.rs`.
- [x] F2 rename, Ctrl+Shift+D duplicate, Ctrl+Shift+N new folder — folio:
      `src/FolderPane.qml:148,179,181`. Existing operations, new keys tested.
- [x] Delete moves to the trash; Shift+Delete asks, then deletes — folio:
      `src/FolderPane.qml:145-147`. Existing.
- [x] Compress and extract — folio: `src/FileMenu.qml:191-196`. Existing
      (`files-operations` archive/extract).
- [x] Undo and redo of eight kinds (Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y) — folio:
      `src/fileops.h:193-194`, `src/FolderPane.qml:149-150`. `undo.rs`:
      rename, move, copy, duplicate, new folder (and new file), compress,
      extract, delete. What undo removes goes to the trash and redo brings
      it back; what a Replace displaced comes back; every step is checked by
      drive and inode first, with folio's "Can't undo that: X isn't where it
      was left." / "… already exists in Y." Tests `tests/undo.rs` (11) and
      `ctrl_z_undoes_the_folder_just_made_into_the_trash_and_ctrl_y_brings_it_back`;
      "Undo Copy" in `…`, shot `14-undo-in-overflow-1600-light-en.png`, and
      the refusal shown in shot `15-undone-copy-notice-1600-light-en.png`
      (taken before the merge bug it exposed was fixed: a merged-into folder
      was logged as created; it is not any more, and
      `undoing_a_merge_takes_back_only_what_it_brought_and_restores_what_it_replaced`
      pins it). A move across drives is undone by rename only, so it is
      refused with the reason rather than copied back.
- [x] Clash dialog: Keep both, Skip, Replace, Merge, "do this for the others"
      — folio: `src/ClashDialog.qml:17-94`. A folder landing on a folder is
      asked about once, as a folder, and offers Merge. Replace and Merge
      never delete: what they displace goes to the job's trash (the item's
      own device's trash when it is on another drive), and if it cannot, the
      item fails and the old one stays. The prompt says so in folio's words.
      Tests in `files-operations/tests/conflicts.rs`
      (`replacing_a_file_sends_the_old_one_to_the_trash`,
      `replacing_a_folder_sends_the_whole_old_folder_to_the_trash`,
      `merging_keeps_what_is_only_there_and_trashes_what_it_replaces`,
      `skipping_a_folder_skips_everything_that_would_have_gone_into_it`,
      `keeping_both_folders_copies_the_new_one_under_its_own_name`,
      `a_replace_that_cannot_reach_the_trash_leaves_the_old_file_alone`) and
      `a_folder_landing_on_a_folder_also_offers_merge_and_says_nothing_is_deleted`;
      shot `13-folder-clash-merge-1600-light-en.png`; after Merge the old
      `summary.txt` was in the trash and `keep.txt` untouched (checked on
      disk).
- [x] Background jobs with progress and cancel — folio: `plans/folio.md:105`.
      Existing durable job engine (tickets 50–56).
- [x] Escape cancels the work under way — folio: `src/FolderPane.qml:155`.
      Escape steps back one thing at a time: a dialog or notice, then the
      selection, then the newest running job. Test
      `escape_clears_the_selection_first_and_then_cancels_the_running_job`.
- [x] Open Terminal Here — folio: `src/FileMenu.qml:302`, `src/fileops.h:151`.
      `terminal.rs`: `xdg-terminal-exec --dir=` first, as folio, then the
      usual GNOME/KDE/tiling terminals with their own flags; the folder is
      one argument, never through a shell. On empty space's menu and in `…`.
      Tests `tests/terminal.rs` (4) and
      `open_terminal_here_starts_the_terminal_in_this_folder_or_says_none_is_installed`.
- [x] Open With, with the default marked — folio: `src/FileMenu.qml:113-125`.
      Existing embedded Better App Chooser.

## 5. Context menus

- [x] Right-click a file: Open, Open With, Preview, Cut, Copy, Copy Path,
      Rename, Duplicate, Compress / Extract, Move to Trash — folio:
      `src/FileMenu.qml:98-328`. `context_menu::selection_menu`, tests
      `tests/context_menu.rs`; shot `02-right-click-file-menu-960-dark-en.png`.
- [x] Right-click a folder adds Open in New Tab, Pin / Unpin, Paste Into
      Folder — folio: `src/FileMenu.qml:113,138,169`. Same tests.
- [x] Right-click empty space: Paste, Copy Path, New Folder, New File,
      Select All, Show Hidden — folio: `src/FileMenu.qml:169-182,307`.
      `context_menu::background_menu`; shot 03.
- [x] Right-click in the Trash: Restore, Delete Permanently — folio:
      `src/FileMenu.qml:98,328`. `tests/context_menu.rs`
      `the_trash_restores_or_deletes_for_good`.
- [x] Empty Trash… — folio: `src/FileMenu.qml:105`, `src/fileops.h:134`.
      On the Trash's empty-space menu and command bar; it is the permanent
      delete of everything there, behind the same confirmation. Tests
      `empty_space_opens_a_terminal_and_the_trash_empties_only_when_it_holds_something`,
      `empty_trash_with_nothing_in_it_asks_nothing`.
- [x] Menu from the keyboard: Shift+F10, Menu, Alt+Enter — folio:
      `src/FolderPane.qml:172`. Key test in `tests/shortcuts.rs`; kit
      `is_menu_key` test.
- [x] A drive's menu: Open, Eject, its removal policy — folio:
      `src/DriveMenu.qml:28-39`. `sidebar_menu(Device)`; not screenshotted
      (no drive in the sandbox).
- [x] Share submenu (Taildrop, Dropbox link) — folio: `src/FileMenu.qml:270`.
      In the selection's menu when there is something to share with: "Send
      to <machine> with Taildrop" for each online machine
      (`tailscale file cp`, each file one argument, on a worker thread, a
      notice when done) and "Copy Dropbox link" (`dropbox sharelink`) for a
      file in the Dropbox folder. Shot `22-…`; command words tested in
      `tests/share.rs`. Nothing was actually sent.
- [x] LocalSend in the Share submenu — folio: `plugins/localsend`.
      `localsend.rs`: LocalSend's protocol v2.2 as v1.18.2 speaks it
      (rustls, socket2, rcgen): a multicast announcement when a selection's
      menu opens (never in the background), answers taken as HTTPS
      `register` calls, then `prepare-upload` and one `upload` per file
      accepted. Better Files makes one self-signed certificate once
      (`$XDG_DATA_HOME/better-os/files/localsend`, key 0600), shows it as
      the TLS client certificate LocalSend requires, and announces its
      DER's upper-case SHA-256 as its fingerprint; a peer's certificate is
      pinned to the fingerprint it announced, and a `register` whose
      certificate does not back its claim is ignored. Tests
      `tests/localsend.rs` against a stand-in HTTPS receiver that demands a
      client certificate and checks it against the announcement, including
      a declined transfer. Not tried with a real LocalSend device.
- [x] Burn an image to a USB drive or SD card — folio:
      `src/FileMenu.qml:203-216`, `src/fileops.h:166-172`. "Write to
      <drive>…" on an `.iso` or `.img`, after a warning dialog; then a typed
      `WriteImage` request to the storage service, which refuses one not
      marked confirmed, a relative or missing image, or a drive being
      written to, and writes through UDisks2's `OpenForRestore` (polkit asks)
      onto the whole drive. Tests with the fake drive only:
      `an_image_is_written_to_the_fake_drive_only_when_confirmed`,
      `an_image_is_written_only_after_the_confirmation`. Never run against a
      real drive.

## 6. Preview

- [x] Space toggles the preview of the selected file — folio:
      `src/FolderPane.qml:137`. Existing in-window preview pane (text,
      images, folders).
- [x] Space opens a large preview in its own process, arrows step through
      the folder — folio: `README.md:8`, `src/PreviewWindow.qml:50-65`.
      `better-files-preview` (`src/bin/preview.rs`, beside `better-files`);
      ← → step through the folder by name, Space / Esc / Q close, `+ - 0`
      zoom. Test `space_opens_the_selected_file_in_the_preview_program`;
      shots 17–21. GIF animation is gpui's image loader's and was not
      looked at frame by frame.
- [x] Audio and video play in the preview — folio: `plans/folio.md:81`.
      GStreamer's `playbin` in the preview process
      (`files-preview/src/media.rs`), video frames as BGRA into a GPUI image,
      with play/pause (button or Enter), seeking (bar or Shift+← →), volume,
      and a plain message naming a missing codec. Tests in
      `files-preview/tests/media.rs` (a WAV to its end, pause, seek, a seek
      between key frames, volume, video frames paused and playing, a file
      that is not media) and `tests/media_missing_codec.rs`; shots
      `26-preview-video-1600-light-en.png`,
      `27-preview-missing-codec-1600-light-en.png`.
- [x] Rendered Markdown — folio: `plans/folio.md:81`. gpui-component's
      Markdown view; shot `18-preview-markdown-1600-light-en.png`.
- [x] PDF pages — folio: `src/PdfPreview.qml`. poppler's `pdftoppm`, page by
      page (Page Up / Page Down), when installed; otherwise the window says
      what to install. `viewer.rs`; test
      `poppler_renders_and_searches_a_real_pdf_when_installed`; shot
      `19-preview-pdf-1600-light-en.png`.
- [x] Archive contents in the preview (owner list; `plans/folio.md:98`).
      zip, tar, tar.gz, tar.zst listed without extracting, with the readers
      the extract operation uses; test
      `an_archive_lists_what_is_inside_without_extracting`; shot
      `20-preview-archive-1600-light-en.png`.
- [x] Syntax-highlighted code (owner list; `plans/folio.md:99`). A lexical
      highlighter in `viewer.rs` (keywords, strings, comments, numbers) for
      18 languages; the workspace has no tree-sitter grammars. Tests
      `rust_keywords_strings_comments_and_numbers`,
      `python_and_shell_use_hash_comments_and_c_uses_blocks`; shot
      `17-preview-code-1600-light-en.png`.
- [x] Search inside a PDF (owner list; `plans/folio.md:100`). `pdftotext`,
      pages split at form feeds; Ctrl+F, Enter goes to the next page with a
      match, the status line lists them. Tested with a real two-page PDF;
      typing in the window was not exercised.
- [x] Office documents through LibreOffice to PDF when installed (owner list;
      `plans/folio.md:97`). `soffice --headless --convert-to pdf` with a
      profile of its own, 90 s timeout; shot
      `21-preview-office-via-libreoffice-1600-light-en.png` (an .odt).
- [x] Details pane: type name, size, modified, opens with, and for the Trash
      the original location and the deletion date — folio:
      `src/PreviewPane.qml:140-258`. Above the preview in the side pane;
      `FilesSession::opens_with`, test
      `the_details_pane_says_what_a_file_opens_with`; shot `22-…`.

## 7. Search and filter

- [x] `/` searches this folder and below — folio: `src/FolderPane.qml:169`,
      `plans/folio.md:75`. Existing recursive search; `/` bound
      (`tests/shortcuts.rs`).
- [x] Type-to-filter inside a folder (owner list; `plans/folio.md:110`).
      Ctrl+Shift+F opens a filter field over the folder; as it is typed the
      folder narrows to names containing it (no case), never reloading and
      never looking below the folder; "Showing N of M", Clear, Esc; cleared
      when the folder changes. `DirectoryModel::set_name_filter` (tests in
      `model.rs`), `a_filter_narrows_this_folder_and_is_gone_in_the_next`.
      Bare letters stay type-ahead (jump to a name): the filter bar on
      Ctrl+Shift+F and type-to-jump are both kept, as decided for this
      ticket. Not screenshotted: the screenshot helper cannot type.
- [x] Sorting: name, type, newest, oldest, size; ascending and descending;
      folders first — folio: `src/SortMenu.qml:10-14`. Sort menu in the
      command bar; `context_menu::step_sort` and
      `the_sort_steps_through_folios_five_orders`.
- [~] `S` / `Shift+S` step the sort — folio: `src/FolderPane.qml:199-200`.
      Deliberately different. folio has no type-ahead, so its bare letters
      are free for commands; Better Files jumps to a name as letters are
      typed (an existing, tested feature), and binding S would make every
      name starting with "s" unreachable that way. Binding it "only when no
      type-ahead is in progress" does not help: the first letter of a jump
      is exactly when none is. The five-step order is
      `FilesSession::step_sort` and the Sort menu.
- [x] Ctrl+H shows hidden files — folio: `src/FolderPane.qml:197`. Existing,
      now remembered per folder.

## 8. Keyboard

- [x] `?` (and F1) shows every shortcut — folio: `src/HelpOverlay.qml:32-70`.
      `shortcuts.rs` (39 lines, both languages), tests
      `tests/shortcuts.rs`; shot `09-shortcut-sheet-960-light-en.png`.
- [x] The keys folio leaves off its sheet are on ours: Alt+D, Alt+Up and
      Shift+F10 — folio `src/FolderPane.qml:172,182,193`.
- [~] `Q` closes the window — folio: `src/Main.qml:113`. Deliberately
      different, for the reason S is: a bare Q is the first letter of a jump
      to a name. Ctrl+W closes the tab and the title bar's × the window.
- [x] Ctrl+F6 cycles panes — folio: `plans/folio.md:147`. Between the
      sidebar and the files; test
      `ctrl_f6_moves_the_keyboard_to_the_sidebar_and_back`.

## 9. Trash

- [x] The freedesktop trash shared with every file manager
      (`~/.local/share/Trash`, `.Trash-<uid>` on drives) — folio:
      `plans/folio.md:69`. Existing `files-platform/src/trash.rs` (per-device
      trash, ticket 50s).
- [x] Restore — folio: `src/fileops.h:131`. Existing.
- [x] Delete permanently, asking first — folio: `src/fileops.h:133`. Existing.
- [x] Empty Trash — folio: `src/fileops.h:134`. See section 5.

## 10. Drives

- [x] Removable drives listed with Eject — folio: `src/Sidebar.qml:389-423`.
      Existing device rows, now navigation items with an Eject action
      (`shell.rs` `device_item`). Not screenshotted: no drive in the sandbox.
- [x] "Safe to unplug" only after the drive's jobs finish — Better Files'
      own guarantee (ticket 55). Existing.
- [x] Mount an unmounted drive from its menu — folio: `src/DriveMenu.qml:28`.
      A Mount line for an unmounted drive (Eject stays, as before), through
      the storage layer's existing mount request; tests
      `a_drive_that_is_not_mounted_offers_mount_and_still_ejects`,
      `mount_from_the_drive_menu_asks_the_storage_layer_and_stays_put`.
- [x] Format (exFAT, FAT32, ext4) — folio: `src/FormatDialog.qml`,
      `src/fileops.h:160-163`. "Format…" in a drive's menu opens a kit dialog
      (filesystem choice, name, the warning); the storage service gets a
      typed `FormatDevice` request and calls UDisks2's own `Format` (it
      unmounts and asks polkit), refusing an unconfirmed request, a label the
      filesystem cannot hold, a system volume, or a drive being written to.
      Tests with fakes only:
      `format_is_refused_until_the_person_confirms_erasing_and_then_goes_through_the_platform`,
      `a_label_the_filesystem_cannot_hold_is_refused_before_anything_is_erased`,
      `nothing_is_erased_while_a_file_operation_is_writing_to_the_drive`,
      `format_asks_first_validates_the_name_and_only_then_reaches_the_storage_layer`.
      Never run against a real drive; the dialog was not screenshotted (no
      drive in the sandbox).
- [x] SMB, NFS and other servers through GVfs — folio: `plans/folio.md:125`.
      Typed in the path bar (see section 2). MTP phones appear when GVfs
      mounts them, as any mount does; not tried with a phone.
- [x] Password prompt for a server — folio: `src/MountPrompt.qml`. A kit
      `ContentDialog` with a user field and a masked password field; the
      answers go to `gio mount`'s standard input, which is then closed, and
      the field is emptied. Same session test as server addresses.

## 11. Thumbnails and cache

- [x] Real thumbnails from the shared freedesktop cache
      (`~/.cache/thumbnails`), failure markers — folio: `README.md:5`,
      `src/thumbnails.cpp`, `plans/folio.md:67-68`.
      `files_preview::thumbnails`: MD5 of the escaped URI, `Thumb::MTime`
      checked against the file, `normal`/`large`, any program's `fail/`
      marker honoured and our own written, PNGs written 0600 through a
      rename; made on a worker thread for images Better Files decodes (PNG,
      JPEG, GIF, BMP). Tests in `thumbnails.rs` (5) and
      `an_image_without_a_thumbnail_gets_one_made_in_the_background`; shot
      `10-thumbnails-1600-light-en.png`.
- [x] Installed thumbnailers (`/usr/share/thumbnailers/*.thumbnailer`) for
      videos, PDFs, fonts and other types — folio: `src/thumbnails.cpp`.
      `parse_thumbnailer`, `make_with`: the command's words filled in and run
      without a shell, stopped after 10 s, the picture stored with the
      standard's text chunks, a fail marker on failure. Tests
      `a_thumbnailer_file_reads_as_its_command_and_types`,
      `an_installed_thumbnailer_fills_the_cache_and_a_slow_one_is_stopped`;
      the ODT tile in shot `22-…` came from an installed thumbnailer.

## 12. Themes and languages

- [x] Light and dark, and following the desktop; the choice is saved —
      owner audit F13. `fluent::theme::Appearance` saved under
      `$XDG_CONFIG_HOME/better-os/files/appearance`; shots 01 (dark) and
      04 (light, after restart: 06).
- [x] English and Traditional Chinese, switched live — folio follows the
      desktop's language (`src/translator.cpp`). Language submenu; shots 06,
      08 (zh-TW).
- [x] File type names in the person's language — folio: `plans/folio.md:77`.
      `files_platform::MimeDescriptions`, tests in `mime.rs`; shot 08
      ("Rust 源碼", "Markdown 文件").
- [~] Fifty languages — folio: `src/i18n/*.json`, `docs/i18n.md`. English
      and Traditional Chinese are complete. Every word is in one table per
      language (`i18n.rs`, a compile error when a language misses one), so a
      language is added by a translator filling a table; the other 48 are
      deliberately not machine-translated.
- [~] Right-to-left layout — folio: `src/HelpOverlay.qml:34`. Deliberately
      not built now: no right-to-left language ships (the other languages are
      not machine-translated, above), so a mirrored layout would have no
      language to switch it on and nothing to check it against. It comes
      with the first right-to-left translation.

## 13. Plugins

- [x] Sidebar sections, Share entries, actions, details and place statuses
      from plugins — folio: `docs/plugins.md`, `src/plugins.cpp`.
      `plugins.rs`, `plugin_host.rs`: folio's manifest as it is (`"folio": 1`),
      `requires`, folio's trust checks, commands without a shell in their own
      process group (killed whole; poll 10 s, details 2 s), 1 MB output
      limit, `items`/`places`/`actions`, `open` (path, GVfs mount, run),
      `to: items` submenus, `under`/`types`/`files`/`single`/`detach`,
      results (`notice`, `copy`, `path`, `error`), and `i18n` catalogs.
      A plugin's own `view` is its page in the main pane (`plugin_page.rs`):
      opened from the section's last row (with the item count) or by typing
      `plugin:<id>` in the path bar, its items in the plugin's order with
      icon, name and subtitle, the selected item's `details` beside them,
      ↑ ↓ Enter Escape and a double click; the section's `rows: "opened"`
      lists the items opened before, most recent first, kept in
      `$XDG_STATE_HOME/better-os/files/plugins-opened.json`. Details are
      fetched by one worker for the latest file only. Tests
      `tests/plugins.rs` (12); shots `30-dark-plugin-section-devices`,
      `31-plugin-page`.
- [~] The bundled Tailscale, Dropbox and LocalSend plugins — folio:
      `plugins/*/manifest.json`. Deliberately built in rather than shipped as
      plugins: `share.rs` and `localsend.rs` do what folio's three plugins do,
      with the window's own dialogs, and need no shell scripts.

## 14. Errors and safety

- [x] Never overwrite on paste: a clash asks — folio: `README.md:10`.
      Existing conflict prompt (see section 4 for what is missing).
- [x] Say what is wrong in the window, closable — folio: `plans/folio.md:59`.
      Notices are a Fluent InfoBar with a close button (owner audit F3's
      yellow strip that never went away).
- [x] Nothing crawls the home folder in the background — folio:
      `plans/folio.md:54`. Existing: folders are listed when opened.

## 15. The owner's audit (F1–F14)

- [x] F1 a 20-button toolbar that wraps → one command bar; what does not fit
      goes behind `…` (`fit_count`). Shots 01, 05.
- [x] F2 sidebar vanishes at 960 → compact pane with ☰. Shots 01, 03.
- [x] F3 right-click pins instead of opening a menu → right-click opens the
      menu; pinning is a menu line and Ctrl+P. Shot 02.
- [x] F4 four permanent buttons on every pin → in the pin's menu. Shot 06.
- [x] F5 Desktop and Music marked unavailable → missing places are left out.
      Shot 03, `tests/sidebar.rs`.
- [x] F6 view and appearance buttons showing different things → View and
      Appearance are menus with check marks. Shot 05.
- [x] F7 "Operations" is really the jobs panel → "File operations". Shot 05.
- [x] F8 26px rows, 12px text, `text/plain` → 36px rows, 14px text, type
      names. Shots 04, 07.
- [x] F9 path field is not a breadcrumb → breadcrumbs. Shots 01–09.
- [x] F10 selection is a white outline → fill and accent bar. Shot 08.
- [x] F11, F12 emoji icons → themed icons and the kit's icon set. Shot 01.
- [x] F13 appearance not saved → saved and restored. Shot 06.
- [x] F14 the same sentence twice in an empty sidebar → one empty-state line,
      the sidebar's drop zone is gone. Shot 03.

Screenshots 01–09 are the window at 960px (the shared sandbox tiled it);
10–16 are at 1600px on a private headless sway, so no other window could
split it.
