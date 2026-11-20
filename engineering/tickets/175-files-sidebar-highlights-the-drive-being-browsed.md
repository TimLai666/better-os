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
