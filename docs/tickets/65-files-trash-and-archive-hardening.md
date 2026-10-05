# Ticket 65 — The Trash view, job numbering, and zip reading hold up under bad conditions

**Epic:** Better Files (Issue #6) · **User Story:** a person whose network share
is unreachable, or who runs two Better Files windows, or who opens a hostile
zip, gets a working window and a clear error · **Branch:** `ticket-65` ·
**Blocked by:** none · **Status:** implemented, not merged

## What it delivers

Three gaps ticket 53, 55, and 56 left open. The Trash view reads every mounted
filesystem's trash, network shares included, so an unreachable server can stall
the listing. Two processes submitting a job at the same instant can pick the
same job number, because nothing claims it on disk before the first write. And
a zip is read whole, central directory included, before its entry count is
checked against the extraction limit.

## Acceptance criteria

- [x] The Trash view does not read the trash of a network or FUSE filesystem
      unless its trash directory is already known to be reachable, and a slow
      or unreachable filesystem cannot block the home trash from listing. The
      rule for which filesystems are skipped is written down with its reason.
- [x] A job number is claimed atomically on disk before the job is accepted,
      so two engines on one store never share a number, and recovery ignores a
      claim that never became a record.
- [x] A zip whose end-of-central-directory record declares more entries than
      the limit, or a central directory larger than a fixed bound, is refused
      before the central directory is read, naming the archive.
- [x] Each gap has a test that fails before the change.

## What was built

Each of the three gaps is closed where it was found: the Trash listing in
`files-gui`, job numbering in `files-operations`' store, and zip reading in
`files-operations`' extractor.

- **The Trash view and network shares.** The listing now sends the home
  trash's rows before it looks for any device, then reads the trashes on
  local disks. It never touches a network or FUSE filesystem itself.
  `files_platform::trash::NETWORK_FILESYSTEMS` names the network filesystems
  (`nfs`, `nfs4`, `cifs`, `smb3`, `smbfs`, `9p`, `afs`, `ceph`, `coda`,
  `davfs`, `glusterfs`, `lustre`, `ncpfs`), and every FUSE mount (`fuse`,
  `fuseblk`, `fuse.<name>`) is treated the same way, with the reason written
  beside the list: a `stat` on a hard-mounted NFS share whose server is gone
  waits forever, and a FUSE mount stalls whenever the process behind it does.
  `volume_trashes` now skips them, `network_or_fuse_mounts` names them without
  touching them, and `trashes_on_mount` is the per-volume search both use. In
  `files-gui`, each such mount gets a probe thread that finds and reads its
  trash, and the listing waits one second (`PROBE_DEADLINE`) for all of them
  together. A mount that answered in time is reachable and its items are
  listed, once per volume even if it is mounted twice. One that did not
  answer is reported as a skipped entry named after its mount point, with the
  reason `files.trash.error.filesystem_not_answering`. While its probe is still
  stuck it is known not to answer: it is not probed again and later listings
  do not wait for it, so a dead server costs one leaked thread, not one per
  opening of the Trash.
- **Job numbers.** `JobStore::claim` creates an empty `job-<id>.claim` with
  `O_EXCL` before a job is accepted. Of two processes claiming one number,
  exactly one wins and the other takes the next number. A record is looked
  for before the create and again after it, because the winner may have
  written its header and dropped its claim in between. The engine releases
  the claim once the header is on disk (`release_claim`). `highest_id` and
  `remove` count claims too, so a claim left by a process that died before its
  first write keeps its number, and recovery, which reads headers only,
  neither reports it nor deletes it. If the store cannot create a claim at all
  it cannot hold the record either, so the job runs unclaimed, as before.
- **Zip declarations.** The new `files_operations::zip_end` reads a zip's
  end-of-central-directory record from the last 65,557 bytes of the file,
  choosing the same record the `zip` crate takes first: the last signature
  whose comment fits inside the file. When one of its fields is saturated and
  a ZIP64 locator sits right before it, it reads the ZIP64 record, which must
  end exactly where the locator starts. The extractor refuses an archive that
  declares more entries than `ExtractLimits::max_entries`, or a central
  directory over `policy::MAX_ZIP_CENTRAL_DIRECTORY_BYTES` (256 MiB, fixed),
  with `archive_limit_exceeded` naming the archive, before the `zip` crate is
  called. The second limit is the new `ArchiveLimit::CentralDirectoryBytes`
  (`central_directory_bytes`). A zip with no end record in its last 65,557
  bytes, or whose ZIP64 locator leads nowhere, is refused as unreadable
  (`files.archive.error.zip_end_record_missing` and
  `files.archive.error.zip64_end_record_unusable`).

Changed: `crates/files-platform/src/trash.rs` and `lib.rs`,
`crates/files-gui/src/reader.rs`, `crates/files-operations/src/store.rs`,
`engine.rs`, `extract.rs`, `error.rs`, `policy.rs`, `lib.rs`, and the new
`zip_end.rs`, and `docs/files-operations-policy.md`. No dependency changed and
`Cargo.lock` is untouched.

Tests, each seen failing before its fix:

- `files-platform`: `a_network_or_fuse_mount_is_left_to_a_probe_rather_than_searched`,
  a `mountinfo` fixture of all ten example filesystems plus a local disk and a
  document portal, each holding a real trash in a temporary directory. Before,
  all ten network and FUSE trashes were returned by `volume_trashes`.
- `files-gui`: `a_share_that_never_answers_does_not_hold_up_the_trash` (a
  probe that blocks until the test lets go: the home trash lists within the
  deadline, the share is named as skipped, and a second listing neither probes
  it again nor waits), `a_share_that_answers_has_its_trash_listed_once`, and
  `the_home_trash_is_on_screen_before_any_device_is_looked_for`. Before, the
  first never finished, the second listed the share twice, and the third saw
  no row while the device lookup was blocked.
- `files-operations`: `tests/persistence.rs::a_number_another_process_claimed_before_its_first_write_is_skipped`,
  `two_engines_on_one_store_never_share_a_job_number` (two engines, eight
  threads, 200 submissions: 115 distinct numbers before, 200 after), and
  `recovery_ignores_a_claim_that_never_became_a_record`; `store.rs`'s
  `a_number_is_claimed_once_and_a_recorded_number_cannot_be_claimed`; and
  `tests/archives.rs`'s three zips whose end records declare too many entries
  (classic and ZIP64, 5,000,000,000 entries) or too large a directory (classic
  and ZIP64), each followed by bytes that are not a directory. Before, all
  three failed as `archive_unreadable` with the `zip` crate's own message:
  the archive went straight to the crate and its declared counts were never
  compared with a limit. Five unit
  tests in `zip_end.rs` cover a written zip, a comment containing a signature,
  a missing end record, a locator that leads nowhere, and a ten-gibibyte
  reader from which nothing before the last 65,557 bytes is read.

### Deliberately not done

- **The `zip` crate's own fallback is not closed.** When the end record that
  was checked does not lead to a readable directory, the crate looks further
  back for an earlier end record and reads the directory that one declares,
  which nothing here checked. The crate checks a ZIP64 entry count against
  the bytes in front of it, so the cost is bounded by the file's size, not by
  256 MiB. The entry limit is applied again to what the crate opened. Closing
  this needs the central directory parsed here rather than in the crate.
- **A FUSE disk can be missed on a slow first answer.** `fuseblk`, which is
  how `ntfs-3g` mounts an NTFS USB disk, is probed like a share. A disk that
  needs more than a second to spin up has its trash left out of that listing
  and named as skipped; the next opening lists it.
- **A stale claim is never removed.** A claim whose process died before its
  first write stays in the store and keeps its number. Removing it safely
  would need to know the process is gone, and a skipped number costs nothing.

### Not verified

- **No real network share or FUSE mount.** The suite mounts nothing. The
  filesystem rule is tested on a `mountinfo` fixture and the stall on a probe
  that blocks on a lock, not on an NFS server that went away.
- **No second real process.** Two engines in one test process share a store
  directory and race on it from eight threads. The claim uses the same
  `O_EXCL` create either way, but two Better Files windows were not run.
- **The window was not opened.** The Trash view's rows and its skipped count
  come from the reader and its tests.
