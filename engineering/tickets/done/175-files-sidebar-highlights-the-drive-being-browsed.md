# Ticket 175 — Better Files' sidebar highlights the drive whose folder is open

**Blocked by:** none

## Why

Found on 2026-11-21 while reviewing ticket 138. With `/` open, the system
drive's sidebar row is not highlighted, because a drive row carries no
location (`NavTarget::location` in `crates/files-gui/src/shell.rs`). The same
holds for any mounted drive whose folder is open.

## What to build

- The sidebar row that is highlighted is the most specific place containing
  the open folder: a favourite or a folder row when one matches exactly, else
  the drive whose mount point is the longest prefix of the path.

## Done when

- Tests: `/`, a folder on the system drive with and without a matching
  favourite, a folder on a second drive, a folder on a drive mounted inside
  another drive.
- Seen in a sandbox.

## Notes

Built on 2026-11-21.

- `files_gui::sidebar::highlighted_row` is the rule: a favourite or folder
  row that is exactly the open folder wins, else the drive whose mount point
  is the longest prefix of its path (by path components, so `/media/a/Data`
  does not contain `/media/a/Data-old`), first line on a tie. Places such as
  Home light only on an exact match, as the ticket says. `NavTarget::place`
  in `shell.rs` gives each line its place: a folder row its location, a
  storage-service device and a mount-table row their mount point, a listed
  drive its mount point by key.
- Tests: `sidebar::highlight_tests` cover `/`, a folder on the system drive
  with and without a matching favourite, a folder on a second drive, a
  folder on a drive mounted inside another, and the Trash.
- Seen in a private headless GNOME Shell 46 (scratch home, a bus whose only
  service directory held dconf's activation file, a fake `tailscale`,
  fixture drives `Data` and `Inner` mounted inside it), English and
  Traditional Chinese: `/home/tim/Developer/better-os-evidence/175/`
  (`root`, `favourite-documents`, `system-folder-no-favourite`,
  `second-drive`, `inner-drive`, each `-en` and `-zh-TW`). The system drive
  shows the machine's os-release name, as in ticket 138's screens.
