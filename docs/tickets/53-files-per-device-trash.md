# Ticket 53 — Deleting a file on a removable disk puts it in that disk's trash

**Epic:** Better Files (Issue #6) · **User Story:** a person who trashes a large
file on a USB disk does not fill their home partition with a copy of it ·
**Branch:** `ticket-53-55` · **Blocked by:** none · **Status:** released in v0.2.8

## What it delivers

Trashing a file on another device copies it into the home trash and deletes the
original. After this ticket, Better Files follows the FreeDesktop trash
specification's per-volume rule: it uses `$topdir/.Trash/$uid` when `.Trash`
exists, is a directory, is not a symbolic link, and has the sticky bit, and
otherwise `$topdir/.Trash-$uid`, creating it with mode 0700. Only when neither
can be used does it fall back to the home trash copy.

## Acceptance criteria

- [x] The top directory of a path is the mount point of its device, found by
      walking up while the device number stays the same.
- [x] `.Trash` that is missing, a symlink, or lacks the sticky bit is rejected
      for `$topdir/.Trash-$uid`, as the specification requires.
- [x] A `.trashinfo` in a per-volume trash records the path relative to the top
      directory, and restore resolves it back against that top directory.
- [x] The Trash view lists items from the home trash and from the per-volume
      trash of every mounted device the user can read.
- [x] Emptying or permanently deleting from the Trash view works for either
      kind of trash.
- [x] Tests use real temporary directories. A second device is not available in
      tests, so the top-directory walk is tested through a seam that supplies
      device numbers.

## What was built

An item on another device now goes to that device's own trash. The home-trash
copy is kept, but only as the last resort, and the job log says when it was
used and why.

- **The top directory.** `files_platform::trash::top_directory` walks up from
  the item's canonical parent directory while the device number stays the
  same. The device numbers come through a `DeviceProbe` trait. The host
  implementation is `lstat`'s `st_dev`, and the tests supply their own, which is
  how the walk is tested without a second device.
- **Choosing the trash.** The executor compares the item's device with the home
  trash's device first (`trash::shares_device`). Same device: a `rename` into
  the home trash, as before. Different device: `trash::volume_trash_for`. A
  `rename` into the home trash that still fails with `EXDEV`, which a bind mount
  can cause, is sent the same way.
- **The volume trash.** `volume_trash` uses `$topdir/.Trash/$uid` only when
  `.Trash` is a real directory, not a symbolic link, with the sticky bit set.
  Otherwise it uses `$topdir/.Trash-$uid`, created with mode 0700 (set
  explicitly, not left to the umask). A per-user directory that is a symbolic
  link, not a directory, or owned by another uid is refused. A shared `.Trash`
  that fails its check is logged as `device_trash_unavailable` with the reason,
  because the specification asks for the failure to be reported.
- **Relative records.** A record in a volume trash stores the path relative to
  the top directory. `TrashDirectory::new` recognises the two volume layouts
  from the root's own name, so a restore or purge named only by its trash root
  (`TrashItemRef` is unchanged) resolves the record against the right top
  directory. A relative path containing `..` or any other non-normal component
  is refused on read. A relative record in the home trash is now resolved
  against `$XDG_DATA_HOME`, as the specification says. Before, it came back as
  a relative path.
- **The Trash view.** `FilesReader` lists the home trash and then every
  existing volume trash from `volume_trashes`, which reads the mount table on the
  listing thread each time the Trash is opened. It creates nothing. Kernel
  interfaces, `autofs` (asking it would trigger a mount), `squashfs` package
  images, and FUSE views of other storage are skipped, and a volume mounted
  twice is listed once. A reader built with `FilesReader::new` looks at no
  device, so no test reads the host's disks.
- **Acting on the right trash.** Trash entries are now identified by their
  stored path, not their stem, because two trashes can each hold a
  `report.txt`. Restore and permanent delete take each selected item's trash
  root from where the item is stored (`trash_root_of`), so one selection can span
  the home trash and a device trash. The session no longer refuses to restore
  just because the session has no home trash.
- **The uid** comes from `/proc/self/status`. That avoided adding `libc` to
  `files-platform`, which would have changed `Cargo.lock`.

Changed: `crates/files-platform/src/trash.rs` and `lib.rs`,
`crates/files-operations/src/exec.rs`, `log.rs`, and `spec.rs` (doc only),
`crates/files-core/src/entry.rs` (`EntryId::TrashItem` now holds a path),
`crates/files-gui/src/reader.rs` and `commands.rs`, and three call sites in
`crates/files-gui/src/session.rs`. `docs/files-operations-policy.md` now
describes the behaviour.

Tests: 12 in `files-platform` (top-directory walk, each `.Trash` rejection, the
0700 creation, a symlinked private trash, relative records and `..`, discovery
from a mountinfo fixture, the uid parser), 4 executor tests in `exec.rs` through
a fake device probe (own device, a refused `.Trash` reported, no usable trash
falls back to the home copy, same device unchanged), 2 job-level tests in
`tests/trash_jobs.rs` (restore resolves against the top directory, one delete
job across both kinds of trash), 3 reader tests and 1 commands test in
`files-gui`.

### Deliberately not done

- **Nothing creates a shared `$topdir/.Trash`.** That directory is the
  administrator's to create, and the specification says so.
- **There is no separate "Empty Trash" command.** There was none before either.
  Emptying is selecting everything in the Trash view and deleting it
  permanently, which now works across trashes.
- **Network filesystems are listed.** An NFS or SMB share can have a
  `.Trash-$uid` too, and leaving it out would hide items the user trashed there.
  The cost is that a share whose server has gone away can stall the Trash
  listing thread. It cannot stall the window or the other listings.

### Not verified

- **No real second device.** The suite cannot mount one, so the device
  decision is proven through the probe seam. The volume trash itself is a real
  directory in a temporary directory, so the `.Trash` checks, the 0700 mode,
  relative records, restore, and purge are real. What has not happened is a
  real `EXDEV` from a real USB disk, a FAT or exFAT volume, where ownership is
  a mount option, or a read-only medium.
- **The window was not opened.** The Trash view's rows come from the reader
  and its tests, and nothing about how a row is drawn changed.
