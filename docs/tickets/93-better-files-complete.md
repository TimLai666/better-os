# Ticket 93 — Better Files does everything folio does, in Fluent

**Blocked by:** 80 (done) · **Status:** ready

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
