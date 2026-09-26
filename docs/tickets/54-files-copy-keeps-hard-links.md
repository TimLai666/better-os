# Ticket 54 — A copied folder keeps its hard links

**Epic:** Better Files (Issue #6) · **User Story:** a person copying a backup
tree that uses hard links does not get a copy several times its size ·
**Branch:** `ticket-53-55` · **Blocked by:** none · **Status:** merged, not released

## What it delivers

Two paths inside one copy job that name the same inode become two independent
files at the destination. After this ticket, the second and later paths are
created as hard links to the first copied destination, when the destination
filesystem supports it.

## Acceptance criteria

- [x] Within one job, files sharing a `(device, inode)` pair with a link count
      above one are copied once and linked afterwards.
- [x] If `link(2)` fails (cross-device destination, filesystem without hard
      links, permission), the file is copied instead and the job records why.
- [x] A move within one filesystem is a rename and is unaffected. A move across
      filesystems keeps links the same way a copy does.
- [x] A job resumed after a restart does not link to a destination that no
      longer holds the first copy's content.
- [x] The policy table that says hard links are not preserved is updated.

## What was built

Within one copy, move, or duplicate job, the first path to a hard-linked file
is copied and every later path to the same file is created as a hard link to
that copy.

- **The plan marks the candidates.** The walk gives every regular file whose
  link count is above one an `InodeKey`, its `(device, inode)` pair. The plan
  then counts that file's bytes once, at its first path, so the progress total
  is the data the job will actually copy.
- **The job keeps the map.** `JobControl` gained two methods, `copied_inode`
  and `record_copied_inode`. The engine keeps the map in the job's shared state:
  a destination is recorded after its copy is written and verified. A second
  path with the same key is linked to that destination under a temporary name,
  then renamed into place, the same atomic pattern a copy uses. Because of
  that, a destination the user chose to overwrite is replaced whole.
- **Before linking, the first copy is checked.** The destination is read again
  and must still be the file the job wrote: same inode, device, size, and
  modification time. If it has changed or gone, or if `link(2)` fails (another
  filesystem, a filesystem without hard links, a permission), the file is
  copied instead. The log records `hard_link_not_preserved` with a stable
  reason key, and the new copy becomes the one later paths link to.
- **Moves.** A move within one filesystem is still a `rename` and never reaches
  this code. A move across filesystems goes through the same copy path, so it
  keeps links the same way. The source is removed after the link, with the same
  unchanged-source check a copied file gets.
- **Across a restart.** The map lives as long as the job, not the process. A
  retry in the same process keeps it, and the check above stops it from linking
  to a first copy that changed between runs. A job resubmitted after a restart
  starts with an empty map, so it never links to a destination an earlier
  process wrote. Recovery never resumes a job anyway: the store's rule is that
  the user resubmits.
- **The policy table** in `crates/files-operations/src/policy.rs` and in
  `docs/files-operations-policy.md` now says hard links are preserved within a
  job, and on what conditions.

Changed: `crates/files-operations/src/plan.rs`, `exec.rs`, `engine.rs`,
`log.rs` (`HardLinked`, `HardLinkNotPreserved`), `policy.rs`, and `lib.rs`.

Tests: `crates/files-operations/tests/hard_links.rs` (six tests: one tree,
separate sources of one job, a cross-filesystem move, a same-filesystem move
left as a rename, a retry after the first copy was rewritten, a resubmitted job
beside a leftover destination), and one unit test in `exec.rs` where `link(2)`
really fails with `EXDEV`, because the recorded first copy is on `/proc`.

### Deliberately not done

- **Links to files outside the job's sources are not recreated.** A file whose
  other link is outside what was selected is copied as a file. There is nothing
  in the destination for it to link to.
- **Symbolic links that share an inode are not linked.** A hard link to a
  symbolic link is rare, and the link item is small enough that copying it
  costs nothing.
- **The first-copy check is inode, size, and modification time.** An edit that
  keeps the size and puts the modification time back would pass it. The inode's
  change time would catch that, but linking itself changes the change time, so
  it cannot be the test.

### Not verified

- A destination filesystem that has no hard links at all, such as FAT or exFAT,
  was not available. The fallback on a refused `link(2)` is proven with a real
  `EXDEV`, not with the `EPERM` those filesystems return. The code path is the
  same one.
