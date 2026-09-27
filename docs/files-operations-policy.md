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
| Hard links | Preserved within one job: the first path to an inode is copied, every later path is linked to that copy with `link(2)`. A cross-filesystem move keeps them the same way; a same-filesystem move is a rename and never touches them | Before linking, the first copy is re-read and must still be the file the job wrote (same inode, size, and modification time). A changed or vanished first copy, or a refused link — another filesystem, a filesystem without hard links, a permission — is copied instead and logged as `hard_link_not_preserved` with the reason. The map lives as long as the job: a retry keeps it, a job resubmitted after a restart starts empty and so never links to a destination an earlier process wrote. Links to files outside the job's sources are not recreated |
| Sparse regions | Preserved through `SEEK_HOLE`/`SEEK_DATA`; dense copy where the filesystem does not answer | A 100 GB sparse image must not become 100 GB of zeroes |
| Durability | `fsync` per file then per parent directory when the destination is declared removable | The device-level flush that makes a disk safe to unplug is `storage-service`'s |

Proof: `tests/metadata_policy.rs`, eleven tests. Two of them state their own
limit: the sparse test skips its hole assertions on a filesystem that does not
support holes, and the extended-attribute test skips on a filesystem that
refuses `user.*` attributes. Both report which happened rather than passing
quietly. Hard links are proven by `tests/hard_links.rs` and by a unit test in
`exec.rs` that makes `link(2)` fail for real with `EXDEV`.

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
`trash_unavailable`, `archive_entry_refused`, `archive_limit_exceeded`,
`archive_unreadable`, and a final `io` that keeps the raw errno.

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
| Trash on another device | Simulated through a device probe that reports which device a path is on, with the device trash really inside a temporary directory. The `.Trash` checks, the 0700 creation, relative records, restore, and purge are real |

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
- **Another device**: an item goes to its own device's trash, not the home
  one. The device's top directory is its mount point, found by walking up
  while the device number stays the same. On it, `$topdir/.Trash/$uid` is used
  when `$topdir/.Trash` is a real directory with the sticky bit, and
  `$topdir/.Trash-$uid`, created with mode 0700, otherwise. A `.Trash` that is a
  symbolic link or lacks the sticky bit is refused and the refusal is logged. A
  per-user directory that is a symbolic link, not a directory, or owned by
  someone else is not used. The record in a device trash holds the path
  relative to the top directory, so it survives the device being mounted
  somewhere else, and a relative path containing `..` is refused on read.
- **A device with no usable trash**: only then is the item copied into the home
  trash and the source deleted, in that order, and the log says why
  (`device_trash_unavailable` followed by `cross_device_fallback`).
- **The Trash view** lists the home trash and every existing device trash the
  user can read, found from the mount table each time it is opened. Kernel
  interfaces, automount points, package images, and FUSE views of other
  storage are not searched. Restore and permanent delete act on each item in
  the trash it is in, so one selection may span several.

## Archives

Compress and extract are jobs like any other: one item per archive, pause and
cancel between chunks, retry of a failed item, the same job record and item
journal, and the same `JobObserver` calls around the run.

| Format | Written as | Read |
| --- | --- | --- |
| `.zip` | deflate; UTF-8 names only | stored and deflate entries |
| `.tar` | GNU tar, long names and non-UTF-8 names kept as bytes | ustar, GNU, and pax, including pre-POSIX tar by name |
| `.tar.gz`, `.tgz` | gzip at the default level | one or more gzip members |
| `.tar.zst`, `.tzst` | zstd at the fastest level, roughly `zstd -1` | one or more zstd frames, 100 MiB window at most |

The format of an archive being read comes from its first bytes, and from its
name only when they say nothing. The name decides the folder it is extracted
into: `photos.tar.gz` goes into `photos/` beside it.

Every library is pure Rust: `zip` with only its flate2 deflate backend,
`tar`, `flate2` on `miniz_oxide`, and `ruzstd`, chosen over the `zstd` crate
because that one compiles the C library.

### Writing an archive

The archive is written under a temporary name beside its destination and
renamed into place when the stream is complete, the same promise a copied file
makes. A cancelled or failed archive leaves no file and no temporary. An
archive already at the destination is a conflict answered like any other: skip,
rename (`photos (copy).zip`), or overwrite.

Permission bits and modification times go in; a zip stores local time with
two-second resolution between 1980 and 2107, a tar stores seconds. Symbolic
links go in as links. A file that changes size or is rewritten while it is
being read fails the archive with `externally_modified`, because an archive
header states the size before the content. Hard links are stored as separate
files. Sockets, fifos, and device nodes are left out and logged.

A zip cannot hold a name that is not UTF-8. Such a name fails the job with
`archive_entry_refused` and `files.archive.entry.name_not_utf8`, naming the
entry, rather than being renamed.

### Extracting: an archive is untrusted input

Every entry name is a path the job is being asked to write, so each is checked
first. An entry that breaks a rule stops the extraction with
`archive_entry_refused`, which carries the archive, the entry's name as the
archive spells it, and one of these reasons:

| Reason | Refused entry |
| --- | --- |
| `files.archive.entry.absolute_path` | A name starting with `/` |
| `files.archive.entry.parent_traversal` | A name with a `..` component |
| `files.archive.entry.unusable_name` | A component that is not a usable filename |
| `files.archive.entry.through_link` | An entry whose parent directory, as already extracted, is a symbolic link |
| `files.archive.entry.replaces_existing` | An entry replacing a directory or a link, or a link replacing anything |
| `files.archive.entry.link_outside` | A symbolic link whose target is absolute or resolves above the folder |
| `files.archive.entry.hard_link_outside` | A hard link whose target is absolute, climbs with `..`, or passes through a link |
| `files.archive.entry.hard_link_target_missing` | A hard link whose target is not a regular file the archive already extracted |

A symbolic link's target is resolved the way the kernel would, against the
links already extracted, and a `..` counts only when it climbs out of a
directory that exists at that moment. A name that does not exist yet could be
made a link by a later entry, which would change what the `..` means. Nothing is
written through a link and no link or directory is replaced, so a link that
resolved inside when it was checked still does when the extraction ends. A
regular file that appears twice is the one replacement allowed: the later one
wins.

Permission bits are carried without set-user-ID, set-group-ID, and sticky
bits, and ownership is not carried. Device nodes, fifos, and sockets in an
archive are skipped and logged.

### Limits

| Limit | Default | Constant |
| --- | --- | --- |
| Bytes written by one extraction | 64 GiB | `policy::MAX_EXTRACTED_BYTES` |
| Entries read by one extraction | 1,000,000 | `policy::MAX_EXTRACTED_ENTRIES` |

Both travel in `CopyPolicy::extract_limits`. Bytes are counted as they are
written, so an entry whose header understates its size is stopped too. A zip
declares its entry count and sizes up front, and one whose declarations already
exceed a limit is refused before anything is written. Reaching a limit fails
the item with `archive_limit_exceeded`, which names the limit and its value.

### What is left behind

Nothing, on every path but a crash. Everything is extracted into a temporary
directory beside the destination, `.<folder>.betteros-part-<pid>-<n>`, and
renamed to the folder only once every entry is in. A refused entry, a limit, a
damaged archive, or a cancel removes the temporary directory. Neither
operation claims rollback: there is never a partial result for a rollback to
undo.

Proof: `tests/archives.rs`, 27 tests: the four formats both ways, each rule
above with a hostile tar or zip assembled in the test, both limits for every
format, pause, resume, cancel, retry, and a record read back by a later
process. Unit tests in `extract.rs` cover the name split, link resolution,
concatenated zstd frames, and format detection.

## Persistence and recovery

A job is stored as two files: a header, `job-<id>.json`, holding the state,
the progress, and the bounded operation log, and an item journal,
`job-<id>.items.jsonl`, one JSON line per planned item followed by one line per
status change and per checksum. Nothing in the header grows with the item
count.

- **A running job appends.** At most every 250 ms the job appends the status
  lines that changed since the last persist, then rewrites the header through a
  temporary and a rename. Nothing already in the journal is rewritten.
- **The journal is written whole** — compacted to one line per item — at
  submission, once the items are planned, when the job finishes, when a crash is recovered, and when it has
  grown past four times its live entries, which only repeated retries reach. A
  failed append also forces the next persist to write it whole, so a partial
  line is never appended after.
- **Replay** reads the item lines and applies the status lines over them. A
  final line that does not parse is a crash mid-append and is ignored; a line
  that does not parse anywhere else, or a status for an item never planned,
  reports the record as damaged.
- **Job numbers continue across processes.** An engine with a store numbers
  its jobs after the highest number already in it, readable or not, and skips
  a number whose record appeared since. A new process therefore never
  overwrites an earlier one's record, including an interrupted job recovery
  would report. Two processes submitting at the same instant can still pick
  the same number.
- **The first format migrates on load.** A schema version 1 record, one JSON
  document holding every item, is read as it stands and rewritten as a header
  and a journal.

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

A job does not outlive the login session
([ADR 0016](decisions/0016-files-job-lifetime.md)). Logging out ends the Better
Files process and the job with it; nothing resumes it on its own at the next
login or after a reboot, and the next window reports it as interrupted, as it
does after a crash.

## No shell strings

There is no `std::process::Command` in `files-operations` or in the trash write
side, and no way to spawn a process from either. Archive and extract are
libraries in the process, not a call to `tar` or `unzip`. Every operation is a syscall on
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
| 10,001 items, copy with a job store attached | 481.6 ms | About 355 ms is 10,001 files at the 100,000-file rate measured in the same run (28,145 files/s) |
| 10,001 items, one whole write | 4.8 ms | Header and compacted journal, after planning and at the end; a running job's persist appends instead, see below |
| The record on disk, 10,001 items | 5.6 MB | Header and journal together, after the job finished |
| One 64 MB file, `fsops::copy_file` directly | 11.0 ms | No job |
| The same copy as a job | 16.4 ms | The engine costs about 5.4 ms, or roughly 50% on a copy this short |

The three 10,001-item rows were measured again for ticket 55, after the item
journal replaced the whole-record rewrite. The other rows are ticket 33's run.

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

### What one persist writes

Measured by `persist_bytes` in the same benchmark, which runs at full size even
in `--test` mode because it copies nothing. The record is a copy job's, with
every item planned and the log at its cap.

| Items | Before: the whole single-file record | After: one item changed | After: 7,157 items changed | After: whole write, after planning and at the end |
| --- | --- | --- | --- | --- |
| 10,001 | 15,538,968 bytes | 559,727 bytes | 995,139 bytes | 5,030,116 bytes |
| 100,001 | 137,941,019 bytes | 561,778 bytes | 997,190 bytes | 45,262,167 bytes |

Before the journal, every persist of a running job rewrote the first column,
four times a second. After it, a persist writes the header and the changed
lines, and that cost is the same at 100,001 items as at 10,001. 7,157 items is
one 250 ms interval at the measured small-file rate. The whole write happens
once the items are planned and at the end, not per persist. The earlier 17.5 MB figure for
10,001 items came from a real job with longer paths; the 15.5 MB here is the
same format for the synthetic record, measured on the same ext4 host as the
table above.

Nearly all of the header is the operation log: 2,304 records at its cap, whose
paths are stored as byte arrays so a name that is not UTF-8 survives. That is
what keeps a persist near half a megabyte. It is bounded, and it does not grow
with the job.

One thing still follows from the throttle:

- The throttle costs recovery precision. An item that finished in the last
  quarter second before a crash comes back marked pending, so a resubmitted job
  re-copies it. That is conservative in the safe direction, and the conflict
  model already covers a destination that is unexpectedly there.

What is not measured, and should be before Better Files claims copy performance:
a real spinning disk, a real USB device with `fsync` per file, and a network
share. All three need hardware the test host does not have.
