# Ticket 93 — Better Files does everything folio does, in Fluent

**Blocked by:** 80 (done) · **Status:** implemented, not merged

## Why

The owner wants Better Files rebuilt with every feature of `omacom/folio`
(Omarchy's file manager, MIT, commit `ac9d14f` read on 2026-11-12), in Rust
and GPUI on `better_ui::fluent`, and judges the current window unusable (audit
findings F1–F14: a 20-button toolbar that wraps, a sidebar that vanishes at
960px, a right click that pins a file instead of opening a menu, emoji icons,
no breadcrumb, appearance not saved).

## What to build

Everything in the folio feature inventory, section by section (views and
zoom, navigation and sidebar, file operations with undo/redo of eight kinds,
background jobs, the full context menus, Space preview in its own process,
search below the folder, selection, the complete shortcut table including
the keys folio leaves out of its help sheet, Tailscale / Dropbox / LocalSend
/ SFTP sharing, trash with restore, drives with eject, format and image
burning, the shared freedesktop thumbnail cache, themes and 50-language
readiness, the plugin system, never-overwrite clash handling), plus what the
owner added on 2026-11-12:

- Clash dialog keeps folio's four answers, Keep both, Skip, Replace, Merge.
  Replace and Merge never delete: what they displace goes to the trash.
- Sort order, hidden-file visibility, view, zoom, and the last folder are
  remembered (folio forgets them). They live in one file of Better Files' own
  under the XDG state directory: one default for every folder, plus an
  override for a folder the person changed, keyed by its path. Nothing is ever
  written into the folders themselves (no `.DS_Store`, no `.directory`), and
  an override for a folder that no longer exists is dropped.
- Trash is the freedesktop.org one every Linux file manager shares
  (`~/.local/share/Trash`, and `.Trash-<uid>` at the top of a removable
  drive), so what Better Files trashes shows up in Nautilus and the other way
  round.
- The five things folio planned and never built: type-to-filter inside a
  folder, archive contents in the preview, syntax-highlighted code preview,
  search inside a PDF, and Office documents previewed by converting to PDF
  with LibreOffice when it is installed.
- Better Files' own existing guarantees stay: durable jobs that survive a
  crash, device "safe to unplug" only after its jobs finish, per-device
  trash, hard links kept on copy, archive entry checks (tickets 50–56).

The inventory with source references is in the session research report; the
worker copies it into `docs/better-files-folio-parity.md` and ticks each item
with the test or screenshot that proves it.

## Acceptance

- [ ] Every line of `docs/better-files-folio-parity.md` ticked with evidence.
- [ ] Every audit finding F1–F14 fixed, each shown by a screenshot.
- [ ] Light and dark, English and zh-TW, 1600 and 960 wide.
- [ ] Tests never touch the person's real home, bookmarks, trash, drives, or
      thumbnail cache.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests,
      and the license check pass.

## What was built

Five passes. `docs/better-files-folio-parity.md` ticks 108 of its 113 lines;
the other five are deliberately different, each with its reason there, and
the owner accepted all five: no machine-translated languages, no
right-to-left layout while no right-to-left language ships, the three folio
plugins built in instead, and no bare S or Q keys.

First pass:

- **The window, in `better_ui::fluent`.** The title bar, a `NavigationView`
  pane (Places with Recent and Trash, pinned folders, Applications, drives),
  an address row (Back, Forward, Up, Reload, single-line breadcrumbs that
  turn into a text field on click or Ctrl+L / Alt+D, search), one
  `CommandBar` whose overflow `…` holds the rest (Language and Appearance
  included), a `TabView` once there are two tabs, an `InfoBar` for notices.
  Every owner-audit finding F1–F14 is fixed.
- **Kit additions, with tests:** drop-down commands and any `…` entry on
  `CommandBar`, toggle items on `MenuItem`, captions, a trailing action and a
  context-menu handler on `NavigationView` items, submenus kept on screen,
  menus that take the pointer (a click on an item no longer lands on the row
  under it), single-line breadcrumbs, `IconSearch::resolve_in`.
- **Content:** 36px rows, themed type icons, type names from the shared MIME
  database in the person's language, columns that step aside when narrow.
- **Context menus** for files, folders, empty space, the Trash, pins, places
  and drives, by right click or Shift+F10 / Menu / Alt+Enter.
- **Remembered views** per folder plus a default and the last folder, in
  `$XDG_STATE_HOME/better-os/files/views.json`, nothing written into folders;
  appearance saved under `$XDG_CONFIG_HOME/better-os/files/appearance`.
- **Keys** from folio and a `?` / F1 sheet listing every binding.

Second pass:

- **Replace and Merge never delete** (`files-operations`). What they displace
  goes to the job's trash, or the item's own device's trash; when it cannot,
  the item fails and the old one stays. A folder landing on a folder is asked
  about once, as a folder, with Merge, Replace, Skip and Keep both; the
  answer covers everything inside it. A folder merged into is not logged as
  created, so neither rollback nor undo can take it away.
  `EngineConfig::displaced_trash` lets every test use a scratch trash.
- **Undo and redo** of rename, move, copy, duplicate, new folder and file,
  compress, extract and delete (`undo.rs`), built from what each job logged,
  checked by drive and inode before anything moves, with folio's refusals.
  Ctrl+Z is now Undo (it was Restore from Trash); Ctrl+Shift+Z and Ctrl+Y
  redo; "Undo Copy" and "Redo …" in `…`.
- **Recents** from the shared `recently-used.xbel`, **thumbnails** from and
  into the shared freedesktop cache (MD5 of the URI, mtime check, fail
  markers) made on a worker thread, **rubber-band selection**, a **folder
  filter** (Ctrl+Shift+F), **Empty Trash**, **Open Terminal Here**
  (`xdg-terminal-exec` first, no shell), **New Window** (Ctrl+N, same
  process).
- The clash prompt is built from kit buttons; the rest of the job panel,
  the preview pane, the dialogs and the app details panel still are not.

Third pass:

- **The large preview, its own process** (`better-files-preview`, Space):
  images, rendered Markdown, code with a lexical highlighter, PDF pages and
  PDF search through poppler, office documents through LibreOffice,
  archive listings, folders; ← → step through the folder. Audio and video
  are deliberately not played (no decoder in the workspace); the window
  says so and shows the poster frame.
- **Details** above the side preview: type, size, modified, opens with,
  and the original place and deletion date in the Trash.
- **Servers in the path bar** through GVfs (`gio mount`, then `gio info`),
  with a sign-in dialog; **Tab completion**; **mouse back/forward**;
  **Ctrl+wheel zoom**; **size and date under grid tiles**; **Escape**
  stepping back to cancelling the running job; **Ctrl+F6**;
  **org.freedesktop.FileManager1**; **installed thumbnailers** with a 10 s
  limit; **Mount** in a drive's menu; **tailnet machines** in the sidebar
  (opened over SFTP), **Taildrop** and **Dropbox link** in a Share submenu,
  the **Dropbox folder** with its sync state, each only when its tool is
  installed.
- **Kit controls** now draw the job panel (cards, `ProgressBar`, standard
  and accent buttons), the dialogs (`ContentDialog`, `TextBox`,
  `CheckBox`), the side preview and the app details panel; files-gui has no
  gpui-component button left. The embedded Better App Chooser is its own
  crate and was not touched.

Fourth pass (after main was merged in: tickets 84 and 92):

- **Columns view** (Ctrl+3) and **split view** (Ctrl+Alt+N, Ctrl+W closes
  the right pane, also in `…`).
- **Format and write image**, through a new privileged path in the storage
  service: `DeviceControl::format` and `write_image` (UDisks2's own
  `Format` and `OpenForRestore`, polkit asking), coordinator checks (the
  person confirmed, the label fits, not a system volume, nothing writing to
  the drive), typed `FormatDevice` and `WriteImage` requests and client
  calls, and the window's confirmation dialogs. Tested with the fake drive
  only.
- **LocalSend** over its own protocol, discovery only when a selection's
  menu opens.
- **Plugins** as folio's `docs/plugins.md` defines them: sidebar sections,
  place statuses, actions and Share entries, details rows, results,
  catalogs, folio's trust checks and timeouts. Not yet a plugin's own view
  in the main pane.
- **The side preview now follows a click.** A click changed nothing the
  frame loop watched, so the pane went on saying "Select something to
  preview"; the selection now asks for the preview itself
  (`the_side_preview_follows_a_click_without_waiting_for_anything_else`).

Fifth pass:

- **Audio and video in the large preview**, through GStreamer
  (`gstreamer`, `gstreamer-video`, `gstreamer-app` bindings; the LGPL
  libraries are the system's, linked dynamically). `playbin` decodes;
  video frames reach an `appsink` as BGRA and become the GPUI image the
  window draws, the old frame's texture given back each time. Play/pause,
  a seek bar, volume, Enter and Shift+← →. A missing decoder is named
  ("This file needs a codec that isn't installed: VP8 decoder…") and the
  controls go away when nothing can play. Seeks are accurate, not to the
  nearest key frame (a regression test proves the key-frame seek landed at
  zero).
- **Packaging:** `better-files` now ships `better-files-preview` (it was
  missing from the package, so Space did nothing in an installed copy) and
  depends on `gstreamer1.0-plugins-base`; `verify-deb.sh` checks both. The
  license inventory lists the GStreamer and GLib system libraries. CI
  installs the GStreamer development packages and the good plugins the
  tests use.
- **Split view lays each half out at its own width**
  (`layout::split_viewport_width`): tiles, list columns, breadcrumbs and
  the command bar fit the half. In a narrow half the search box is
  narrower, two crumbs fold to "… › current", and the current folder's
  name ends in an ellipsis instead of being cut off
  (`two_crumbs_in_a_narrow_bar_fold_to_the_current_folder`).
- **LocalSend devices join an open menu** the moment they answer
  (`localsend::KnownPeers`, whose generation the window watches), and a
  device that stopped answering leaves at the end of the search.

Screenshots (`shots-93/`): 01–09 at 960px (the shared sandbox tiled the
window), 10–16 at 1600px on a private headless sway:
`10-thumbnails-1600-light-en.png`, `11-rubber-band-1600-light-en.png`,
`12-recents-1600-light-en.png`, `13-folder-clash-merge-1600-light-en.png`,
`14-undo-in-overflow-1600-light-en.png`,
`15-undone-copy-notice-1600-light-en.png`,
`16-new-window-1600-light-en.png`, `17-preview-code-1600-light-en.png`,
`18-preview-markdown-1600-light-en.png`, `19-preview-pdf-1600-light-en.png`,
`20-preview-archive-1600-light-en.png`,
`21-preview-office-via-libreoffice-1600-light-en.png`,
`22-details-pane-share-menu-1600-light-en.png` (it shows this machine's real
tailnet machine names; keep it local),
`23-side-preview-follows-click-1600-light-en.png`,
`24-columns-view-1600-light-en.png`, `25-split-view-1600-light-en.png`,
`26-preview-video-1600-light-en.png`,
`27-preview-missing-codec-1600-light-en.png`,
`28-split-view-narrow-1000-light-en.png`.
