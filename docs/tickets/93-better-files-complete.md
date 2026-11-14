# Ticket 93 — Better Files does everything folio does, in Fluent

**Blocked by:** 80 (done) · **Status:** merged, not released

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

After the independent review of the fifth pass:

- **Write image goes to a whole drive.** Targets are drives (vendor, model,
  size), never a partition; the dialog names every volume on the drive and
  says all of them are erased (Format's own warning now says only that
  volume is). Every mounted volume on the drive is unmounted first, and one
  that will not unmount stops the write before a byte is written.
- **The storage service no longer holds its lock while an image copies**:
  the drive and its volumes refuse every other request until the copy ends,
  `ImageProgress` signals report how far it is, and the window shows the
  percentage. The copy runs on a thread of its own (zbus's executor has no
  tokio runtime). Labels are measured in UTF-16 units for exFAT and bytes
  for ext4 and FAT.
- **Undo:** a move answered with Merge is undone item by item (the source
  folders are made again, the folders the move made go once empty), Merge
  with no clash inside is recorded too, and a change that can no longer be
  undone is said so and forgotten instead of blocking the older ones.
- **Replace or Merge onto the source itself, or a folder it is inside, is
  refused** before anything reaches the trash
  (`files.operation.error.replaces_source`).
- **Durable jobs:** a copy or move records how to start it again and the
  answers given about folders; at start the window offers every copy or
  move a dead process left unfinished (Resume, Dismiss), and a resumed job
  leaves its finished items alone and keeps the folder answers.
- **LocalSend** pins the peer's certificate to the SHA-256 it announced,
  refuses plain-HTTP peers, and reads at most 1 MB of an answer.
- **Window:** a clash card with the clashing name, where each one is (paths
  shortened in the middle) and "Apply to the rest" as a check box; list
  columns and grid tiles sized beside every open panel; path-bar crumbs
  and sidebar use the same names (文件); a trash can and one icon per
  personal folder; "1 item", "2 hidden items", line counts as lines;
  file types from the language pack when the database XML has no
  translation (純文字文件); duplicate tailnet names told apart by their DNS
  names; a short search hint in a narrow pane; "Working out the time left";
  no estimate on a finished job; the same file name in another folder is a
  new preview.
- **Preview:** video frames scaled down to the window; textures freed on
  the window being drawn; LibreOffice and thumbnailers run in a process
  group ended whole on timeout; thumbnailers write into a private 0700
  folder; the media tests keep GStreamer's registry out of `~/.cache`.
- **Plugin pages** (`plugin:<id>`), as in the parity list.

Screenshots of this round (`shots-93b/`, a private headless sway with a fake
storage service on its own session bus, a fake drive and a LocalSend
stand-in): `30-dark-plugin-section-devices-1600-dark-en.png`,
`31-plugin-page-1600-dark-en.png`,
`32-format-dialog-fake-drive-1600-dark-en.png`,
`33-write-image-dialog-whole-drive-1600-dark-en.png`,
`34-clash-card-name-paths-checkbox-1600-dark-en.png`,
`35-merged-move-done-1600-dark-en.png`,
`36-undo-after-merge-move-1600-dark-en.png`,
`37-localsend-in-share-submenu-1600-dark-en.png`,
`38-list-preview-details-1600-dark-zh.png`,
`39-list-preview-details-1600-light-zh.png`,
`40-preview-follows-click-1600-light-en.png`,
`41-compact-sidebar-icons-tooltip-960-light-en.png`. The earlier rounds'
`shots-93/` (01–28) were lost when the machine's temporary folder was
cleared on a restart.

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

### Second review fixes

- **LocalSend identity as v1.18.2 requires it:** one self-signed
  certificate, made once under `$XDG_DATA_HOME/better-os/files/localsend`
  (folder 0700, key 0600), presented as the TLS client certificate; the
  announced fingerprint is the upper-case SHA-256 of its DER. Discovery
  takes answers as HTTPS `register` calls and ignores one whose certificate
  does not back its claimed fingerprint. A chunk size past what a number
  holds is refused (checked in debug and release).
- **Undo that stops part way** keeps what ran on the redo list and what is
  left on the undo list, so it can be finished later.
- **Error keys become sentences** (`errors::explain`): every storage and job
  engine key the window can show reads as what to do, in English and
  Traditional Chinese; no raw key reaches a notice or a failed job's line.
- **Notices take the tone of what they say:** a finished undo, format or
  image write is a success; refusals stay warnings.
- **Plugin page:** its own sidebar row is lit, and New, Sort, View, cut,
  copy, paste and trash are off while it shows. A plugin's section titles
  follow the window's language through its catalog.
- **Navigation pane:** a pane taller than the window scrolls with a
  scrollbar always in sight, and its headers and dividers keep their size
  instead of being squeezed out (960×720 lost "實驗室機器" and "裝置").
- **List icons** come from the desktop's icon theme first in the light
  theme too.
- **Image write in the storage service:** the D-Bus call awaits the
  progress signaller instead of blocking the executor; the drive's busy
  mark is released when the write is dropped, however it ends; an unlocked
  LUKS volume backed by the drive is unmounted first (followed through
  `CryptoBackingDevice`), and the write is refused when it cannot be.
- **Media tests** remove their temporary GStreamer folder when they end.

Screenshots of this round (`.shots/` in the worktree, the private headless
sway with the fake storage service and a fake drive):
`50-plugin-page-own-row-lit-commands-off-1600-light-en.png`,
`51-write-image-dialog-1600-light-en.png`,
`52-write-done-success-style-1600-light-en.png`,
`53-clash-card-1600-light-en.png`,
`54-completed-job-no-time-left-1600-light-en.png`,
`55-undo-merge-move-success-style-1600-light-en.png`,
`56-compact-sidebar-scrollbar-960-light-zh.png`,
`57-open-pane-plugin-title-zh-960-light-zh.png`,
`58-plugin-title-from-catalog-1600-light-zh.png`,
`59-list-view-960-light-zh.png`,
`60-open-pane-headers-960-light-zh.png`.

### Third review fixes

- **LocalSend register listener:** each connection is served on a
  short-lived thread whose reads and writes wait only for the time left in
  the discovery window, so a client that trickles bytes holds neither the
  listener nor other devices past it. The listener binds only the address
  the announcement leaves from (the one the system routes LocalSend's group
  through), and a register that does not speak HTTPS is ignored, as an
  announcement is.
- **Image write busy mark:** the copy holds the drive's mark through the
  progress it is given (`ImageWrite::holding`), so the drive is released
  only when the copying thread really ends, even when the caller went away
  first. Unlocked LUKS volumes backed by the drive join the busy set, so
  nothing remounts one mid-write.
- **Error sentences live in `i18n.rs`** (`ErrorCopy`, one table per
  language; a missing sentence is a compile error), and the disk service's
  refusals ("unmount was refused for …: target is busy") read as
  translated sentences naming `/dev/sdc1` rather than the UDisks object
  path.
- **Navigation pane:** rows end where the scrollbar begins, and a count
  badge in the compact pane ends at its row's edge instead of being cut off
  at the pane's. Checked at a scrolling height in Better Files, Git
  (settings), Manager, Monitor and the Fluent gallery.

Screenshots (`.shots/`, private headless sway):
`61-files-scrolling-pane-eject-clear-960-light-en.png`,
`62-files-compact-pane-scrolling-960x560-light-en.png`,
`63-git-settings-nav-scrolling-960x340-dark-en.png`,
`64-gallery-nav-scrolling-960x420-dark-en.png`,
`65-manager-compact-nav-scrolled-960x380-dark-en.png`,
`66-manager-open-nav-scrolled-960x380-dark-en.png` (it shows this
machine's real service states; keep it local),
`67-monitor-compact-nav-scrolling-960x380-dark-en.png`,
`68-monitor-open-nav-scrolling-960x380-dark-en.png` (real processor load;
keep it local), `69-gallery-compact-count-badges-inside-1200-dark-en.png`.
