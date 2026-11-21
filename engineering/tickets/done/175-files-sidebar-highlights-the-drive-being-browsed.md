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
- Review follow-up, remote folders: a server this window mounted (a tailnet
  machine, a plugin's mount item) is remembered by the address it was asked
  for (`FilesSession::server_folder`), and its row stands for the GVfs folder
  it was mounted at. A folder on a network or FUSE filesystem
  (`FilesSession::foreign_mount`, read from the kernel's mount table once per
  folder) lights only a line at or inside that mount, so `/` never lights for
  `/run/user/<uid>/gvfs/sftp:host=…`, a phone lights its own drive row, and a
  share with no row (an SMB address typed in the path bar, sshfs, NFS) lights
  nothing. The Trash and Recent light their own rows only, as before.
- Tests: `sidebar::highlight_tests` cover `/`, a folder on the system drive
  with and without a matching favourite, a folder on a second drive, a
  folder on a drive mounted inside another, a mounted server and a phone
  under GVfs, GVfs, sshfs and SMB folders with no row, the Trash and Recent;
  `tests::a_mounted_server_is_remembered_by_the_address_it_was_asked_for`.
- Seen in a private headless GNOME Shell 46, English and Traditional Chinese:
  `/home/tim/Developer/better-os-evidence/175/` (`favourite-documents`,
  `system-drive-folder-no-favourite`, `second-drive`, `inner-drive`, each
  `-en` and `-zh-TW`). Each image is the Better Files window alone
  (`ScreenshotWindow`). Everything listed is a fixture tree under the
  sandbox's scratch directory, which is also its `HOME`; the drives are
  fixtures (`BETTER_FILES_TEST_DRIVES`) whose mount points are inside that
  tree, except the system drive's `/`; dconf reads only the sandbox's own
  user database (`DCONF_PROFILE`); the bus could activate only dconf. The
  system drive row shows this machine's os-release name and the fixture's
  sizes. Two cases are left to the tests rather than shown: `/` itself
  (showing it lists and measures the real root), and remote folders (no
  server in the sandbox).
- The first set of screenshots, taken earlier the same day, listed the real
  `/` and measured the real `/home`, and Better Manager's guard check showed
  the laptop's model and battery; they were deleted. That sandbox had a
  scratch `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_RUNTIME_DIR`
  and no `DCONF_PROFILE`; the taskbar's pinned applications came from the
  system-wide default in
  `/usr/share/glib-2.0/schemas/25_zorin-os-default-settings.gschema.override`,
  not the person's dconf.
