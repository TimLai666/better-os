# Ticket 53 — Deleting a file on a removable disk puts it in that disk's trash

**Epic:** Better Files (Issue #6) · **User Story:** a person who trashes a large
file on a USB disk does not fill their home partition with a copy of it ·
**Branch:** `ticket-53-55` · **Blocked by:** none · **Status:** ready

## What it delivers

Trashing a file on another device copies it into the home trash and deletes the
original. After this ticket, Better Files follows the FreeDesktop trash
specification's per-volume rule: it uses `$topdir/.Trash/$uid` when `.Trash`
exists, is a directory, is not a symbolic link, and has the sticky bit, and
otherwise `$topdir/.Trash-$uid`, creating it with mode 0700. Only when neither
can be used does it fall back to the home trash copy.

## Acceptance criteria

- [ ] The top directory of a path is the mount point of its device, found by
      walking up while the device number stays the same.
- [ ] `.Trash` that is missing, a symlink, or lacks the sticky bit is rejected
      for `$topdir/.Trash-$uid`, as the specification requires.
- [ ] A `.trashinfo` in a per-volume trash records the path relative to the top
      directory, and restore resolves it back against that top directory.
- [ ] The Trash view lists items from the home trash and from the per-volume
      trash of every mounted device the user can read.
- [ ] Emptying or permanently deleting from the Trash view works for either
      kind of trash.
- [ ] Tests use real temporary directories. A second device is not available in
      tests, so the top-directory walk is tested through a seam that supplies
      device numbers.
