# Better Files operation policy

What a Better Files job promises, what it refuses to promise, and what it
costs. Issue #6 requires the metadata and partial-transfer behaviour to be
documented rather than assumed; this is that document, and the tests named
against each section are the enforcement.

## Jobs, not window work

A file operation is a job the engine owns. `files-operations` runs jobs on a
worker pool that no window is attached to.

- `JobHandle` is a receipt: an identifier and an event stream. It implements no
  `Drop`. Dropping every handle to a running copy does nothing to the copy.
- Only `JobEngine::cancel` stops a job.
- Dropping the engine waits for running jobs to finish. It cancels jobs that are
  parked on a pause or a conflict, because a parked job has nobody left to
  answer it.

Proof: `tests/lifecycle.rs::a_job_that_outlives_its_handle_finishes_anyway`.

## Concurrency policy

| Level | Policy | Why |
| --- | --- | --- |
| Jobs | Two at a time by default | More helps a network share and hurts a spinning disk, where two interleaved copies cost more in seeks than they gain in overlap |
| Items within a job | One at a time, in sorted-name order | Makes throughput measurable, conflict decisions ordered, and the operation log a sequence rather than a transcript |
| Locks | Never held across a filesystem call | Pausing waits for the current chunk, not the current file |

## Copy correctness

| Property | Policy | Note |
| --- | --- | --- |
| Modification time | Preserved to nanosecond resolution | What every sort by date depends on |
| Access time | Preserved | Costs nothing; dropping it makes a backup look freshly read |
| Creation time | Not carried | Linux exposes `statx(STATX_BTIME)` for reading and has no interface for setting it |
| Permission bits | Preserved, masked by the destination's mount options | An executable script stays executable |
| Ownership | Not carried | Changing an owner needs `CAP_CHOWN`; this crate never runs privileged. A copy is owned by whoever made it |
| POSIX ACLs | Carried only where the filesystem exposes them as `system.posix_acl_*` extended attributes | No portable unprivileged interface beyond that. An ACL that would not cross is recorded in the operation log, not dropped silently |
| Extended attributes | Copied where the destination takes them; per-attribute refusals logged, never fatal | A FAT stick with no xattrs must not fail a copy |
| Symbolic links | Copied as links, same target text | Following them turns one link farm into forty copies of its target |
| Hard links | Not preserved between separately copied files | Needs a job-wide inode map; a real feature, and not this ticket |
| Sparse regions | Preserved through `SEEK_HOLE`/`SEEK_DATA`; dense copy where the filesystem does not answer | A 100 GB sparse image must not become 100 GB of zeroes |
| Durability | `fsync` per file then per parent directory when the destination is declared removable | The device-level flush that makes a disk safe to unplug is `storage-service`'s |

Proof: `tests/metadata_policy.rs`, eleven tests. Two of them state their own
limit: the sparse test skips its hole assertions on a filesystem that does not
support holes, and the extended-attribute test skips on a filesystem that
refuses `user.*` attributes. Both report which happened rather than passing
quietly.

## Partial copy and move

Every destination file is written to a temporary name in the destination
directory — `.<name>.betteros-part-<pid>-<n>` — and renamed into place only
after its bytes, its metadata, and its verification are complete. `rename(2)`
within one directory is atomic, so an interrupted, cancelled, or failed copy
leaves either the previous content or nothing. It never leaves a truncated file
under the real name. The temporary is removed on every exit path.

A move is:

- **Same filesystem**: `rename(2)`. No bytes move.
- **Different filesystem**: copy, re-check the source's metadata, verify the
  destination, then delete the source — in that order. An interrupted
  cross-device move therefore leaves the source intact and no destination.

The source of a move is deleted only after a `(inode, device, size, mtime)`
re-check confirms nothing else rewrote it while the copy ran. A mismatch is
`files.operation.error.externally_modified` and the source survives.

Proof: `tests/lifecycle.rs::cancelling_mid_copy_leaves_no_partial_destination`,
`tests/metadata_policy.rs::a_source_rewritten_during_a_cross_filesystem_move_is_not_deleted`.

## Conflicts

Four kinds — already exists, case conflict, permission, no space — and four
answers: skip, overwrite, rename, cancel. An answer carries its own scope: this
item, or every remaining conflict of the same kind in this job. That scope is
the whole point; it is what turns a thousand prompts into one.

Standing answers are keyed by conflict kind. A user who said "overwrite every
existing file" has said nothing about what to do when the disk fills up, and
the job still asks.

A job with no standing answer and no responder parks in
`waiting-on-conflict` and stays there. That is what "waiting" means. A headless
caller supplies a `ConflictPolicy` up front.

## Errors

Every condition Issue #6 names has its own variant and its own stable key, so a
translated string is keyed off the variant rather than matched against English:
`no_space`, `quota_exceeded`, `permission_denied`, `read_only`, `not_found`,
`already_exists`, `is_a_directory`, `not_a_directory`,
`destination_inside_source`, `symlink_loop`, `name_too_long`, `invalid_name`,
`device_lost`, `cross_device`, `externally_modified`, `verification_failed`,
`confirmation_required`, `cancelled`, `interrupted`, `conflict_unresolved`,
`trash_unavailable`, and a final `io` that keeps the raw errno.

Classification is by errno, not by `std::io::ErrorKind`: several of the
interesting ones are still `Uncategorized` on stable Rust.

Paths travel as `PathBuf`, never as `String`. A failure about a file whose name
is not valid UTF-8 names the real file, in the error, in the log, and in the
persisted record. Only the `Display` rendering is lossy.

### What is proven against real conditions, and what is not

| Condition | How it is tested |
| --- | --- |
| Permission denied, source and destination | Real: `chmod 000`, skipped when the suite runs as root |
| Symlink loop | Real: a link pointing at an ancestor, with the follow-links policy |
| Concurrent external modification | Real: the job is paused mid-copy, the source is rewritten, the job resumes |
| Source disappearing mid-job | Real: removed while the job is paused |
| Filename encoding | Real: copy, move, delete, trash, restore, and the persisted record, all with invalid UTF-8 names |
| Destination inside its own source | Real: refused at submission |
| Very long path | Classification only: a path over `PATH_MAX` cannot be created to copy from, so what is proven is that `ENAMETOOLONG` becomes the named error |
| Full disk, exhausted quota | Classification only: filling a filesystem needs one the suite may not create |
| Device disappearing | Classification only: needs hardware. All three errno values the kernel produces are covered |
| Case conflict | Classification only: needs a case-insensitive mount |
| Cross-filesystem move | Forced by policy flag, not by a second mount: the code path is identical, what is simulated is the `EXDEV` that selects it |

## Trash

The freedesktop layout, write side included. Trashing claims its name by
creating `info/<name>.trashinfo` with `O_EXCL` before moving anything, so two
processes trashing `notes.txt` at the same moment cannot both win the name.
The move itself is a `rename(2)`; a failed move removes the record it just
wrote, so a record never outlives the data it describes.

- **Collisions in the trash**: the second `report.txt` becomes `report.1.txt`,
  and both keep their own original path, so both restore to the right place.
- **Collisions on restore**: refused, not overwritten. The job raises a conflict
  and the user chooses. A restore that silently replaced a newer file with a
  deleted one would be the most destructive thing a file manager could do
  quietly.
- **Permanent delete**: data first, record second, so an interruption leaves an
  orphaned record — which the read side already skips and reports — rather than
  a file nothing can name.
- **Cross-filesystem**: `rename` fails with `EXDEV`, and the job falls back to
  copying into the home trash and deleting the source, in that order. The
  per-device `.Trash` and `.Trash-$uid` directories the specification also
  allows are **not** implemented. Creating a top-level `.Trash`, checking its
  sticky bit, and falling back per-uid is separate work with its own permission
  cases, and using the home trash while claiming device-trash support would put
  the user's files somewhere they did not expect.

## Persistence and recovery

Every state change writes the job's record, through a temporary and a rename, so
the file on disk always holds a complete document. While a job is running the
record is rewritten at most every 250 ms rather than after every item; see the
measured cost below for why, and for what that costs in recovery precision.

A record found in `running`, `paused`, or `waiting-on-conflict` after a restart
belonged to a process that is gone. Recovery moves it to `failed` with
`files.operation.error.interrupted` and keeps the item list, so the operation
centre can say "this copy stopped partway, 412 of 900 files were done, here are
the rest".

Recovery does **not** restart the job. Resuming needs the original spec, and the
record deliberately does not hold one: a permanent delete's spec carries a
confirmation the user gave to a process that no longer exists, and
reconstructing it from a file would make that confirmation forgeable by anyone
who can write to the state directory. The user re-submits.

A record this build cannot parse, or one claiming a newer schema, is reported as
damaged and left on disk. It may be a record a newer build wrote.

Ticket 33 scopes persistence to surviving a UI restart. Whether a job should
survive a logout or a reboot is one of Issue #6's deferred decisions and stays
open.

## No shell strings

There is no `std::process::Command` in `files-operations` or in the trash write
side, and no way to spawn a process from either. Every operation is a syscall on
a path, so Issue #6's rule holds by construction. `tests/no_shell_strings.rs`
scans both crates' sources on every test run and fails on any spawn primitive.

## Measured cost

`cargo bench -p files-operations`, on an ext4 root filesystem, 3 iterations,
median reported. **The large-file figures are page-cache figures**: 4.3 GB/s is
memory bandwidth, not disk bandwidth. What they measure honestly is the
engine's own overhead relative to the syscalls; what they do not measure is a
real device. The benchmark prints the filesystem behind its temporary directory
so a tmpfs run cannot be mistaken for a disk run.

| Benchmark | Median | Note |
| --- | --- | --- |
| One 512 MB file, copy | 101.2 ms | 5,306 MB/s, page cache |
| The same copy, verification off | 96.7 ms | Verification costs about 4.5 ms, roughly 4% |
| 100,000 files of 512 bytes, copy | 3,493 ms | 28,628 files/s |
| 100,000 files, plan only | 175.2 ms | 5% of the copy: the walk that buys an honest total |
| 100,000 files, same-filesystem move | 1,478 ms | 67,643 files/s, no bytes copied |
| One 128 MB file, cross-filesystem move | 39.0 ms | Copy, verify, delete |
| 10,001 items, copy with a job store attached | 419.4 ms | Against 350 ms for the same count with no store |
| 10,001 items, one record write | 21.6 ms | Paid at most every 250 ms, not per item |
| The record on disk, 10,001 items | 17.5 MB | Every item and a bounded log |
| One 64 MB file, `fsops::copy_file` directly | 11.0 ms | No job |
| The same copy as a job | 16.4 ms | The engine costs about 5.4 ms, or roughly 50% on a copy this short |

The engine's overhead is fixed per job — the plan walk, the worker handoff, the
first record write — so it is a large fraction of an 11 ms copy and a rounding
error on a real one. The number worth watching is the small-file rate, because
that is where the per-item bookkeeping actually lands.

### What the benchmark changed

Writing the record after every item was quadratic and the benchmark caught it:
201 items cost 142 ms with a store attached against 12 ms without, because each
of the 201 writes serialized a record that had grown by one more item. Records
are now written at most every 250 ms while a job runs, plus immediately on every
state change and at the end. After the change the same job costs 14.7 ms.

Two things follow from the 17.5 MB record for 10,001 items:

- The record is a full rewrite, not a journal. A job of a million items would
  produce a record of gigabytes, so a very large job needs an append-only item
  journal before it is offered. That is a follow-up, named rather than
  discovered later.
- The throttle costs recovery precision. An item that finished in the last
  quarter second before a crash comes back marked pending, so a resubmitted job
  re-copies it. That is conservative in the safe direction, and the conflict
  model already covers a destination that is unexpectedly there.

What is not measured, and should be before Better Files claims copy performance:
a real spinning disk, a real USB device with `fsync` per file, and a network
share. All three need hardware the test host does not have.
