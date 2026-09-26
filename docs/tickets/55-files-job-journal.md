# Ticket 55 — A job of a million items can be recorded without rewriting it

**Epic:** Better Files (Issue #6) · **User Story:** a person can copy a very
large tree and still resume it after Better Files restarts · **Branch:**
`ticket-53-55` · **Blocked by:** none · **Status:** implemented, not merged

## What it delivers

A job record is rewritten whole on every persist: about 17.5 MB at 10,001
items, growing linearly and rewritten every 250 ms. After this ticket a job is
stored as a small header plus an append-only item journal, so a persist writes
only what changed.

## Acceptance criteria

- [x] Item progress is appended, not rewritten. The header is rewritten
      atomically and stays small.
- [x] Recovery replays the journal and tolerates a torn final line from a crash
      mid-append by ignoring that line.
- [x] A record written by the current format is read and migrated on load.
- [x] The journal is compacted when a job finishes or when it exceeds a fixed
      multiple of the live item count.
- [x] A benchmark or test records the bytes written per persist at 10,001 and
      100,001 items, before and after.

## What was built

A job is now stored as two files. `job-<id>.json` is the header: state,
progress, and the bounded operation log. `job-<id>.items.jsonl` is the item
journal: one JSON line per planned item, then one line per status change and
one per checksum. The header holds nothing that grows with the item count.

- **A running job appends.** At most every 250 ms the engine appends a status
  line for each item that changed since the last persist, then rewrites the
  header through a temporary and a rename. What is appended and what is written
  is taken under the job's lock and written after the lock is released, so a
  snapshot never waits on the disk. The journal is appended before the header,
  so the header never describes progress the journal does not hold.
- **Replay and a torn line.** The journal is replayed by reading the item lines
  and applying the status lines over them. A final line that does not parse is
  an append a crash cut short, and it is dropped. A bad line anywhere else, or a
  status for an item that was never planned, reports the record as damaged,
  the same way an unreadable record was reported before. Recovery rewrites the
  journal whole, so a torn fragment never sits under a later append.
- **Migration.** A schema version 1 record, the single JSON file holding every
  item, is read as it is (it is exactly `JobRecord`'s serialized shape) and
  rewritten on load as a header and a journal. The journal goes first and the
  header replaces the old file last, so a crash between the two leaves the old
  file, which migrates again next time. The schema version is now 2.
- **Compaction.** The journal is written whole, one line per item and
  checksum, at submission (still empty), once the items are planned, at a
  cancellation before start, at the end of the job, at recovery, and whenever it has grown past `COMPACT_FACTOR` (4) times
  its live entries. A normal run writes two lines per item, so only repeated
  retries reach that multiple. A failed append also forces the next persist to
  write the whole journal, so nothing is ever appended after a partial line.
- **Checksums** moved from the header into the journal, because a checksum
  job's digests grow with its item count.

Changed: `crates/files-operations/src/store.rs` (format, replay, migration,
`JournalCursor`), `engine.rs` (the incremental persist and the points that
write whole), `lib.rs`, `benches/operations.rs` (`persist_bytes`), and
`docs/files-operations-policy.md`.

Tests: 8 new in `store.rs` (appending leaves the existing bytes untouched and
the header small, a torn final line, damage before the end, a status for an
unplanned item, version 1 migration, compaction, the cursor's rewrite rule,
and `remove` taking the journal too), and 2 in `tests/persistence.rs`. One
shows a running job's journal growing by appending, with the earlier bytes left
as they were, and compacted when the job ends. It failed when the engine was
temporarily forced back to writing the whole record every time. The other shows
one persist costing the same at 10,001 and 100,001 items.

### Bytes written per persist

From `cargo bench -p files-operations`, function `persist_bytes`. The record is
a copy job's, with every item planned and the log at its cap. "Before" is that
record serialized the way the version 1 writer did, which every persist of a
running job used to rewrite. "After" is what one persist appends plus the
header it rewrites.

| Items | Before | After, 1 item changed | After, 7,157 items changed | Whole write (after planning, at the end) |
| --- | --- | --- | --- | --- |
| 10,001 | 15,538,968 bytes | 559,727 bytes | 995,139 bytes | 5,030,116 bytes |
| 100,001 | 137,941,019 bytes | 561,778 bytes | 997,190 bytes | 45,262,167 bytes |

7,157 items is one 250 ms interval at the measured small-file copy rate. A
persist is now the same size at 100,001 items as at 10,001. The two differ by
the digits of a larger progress count, not by anything that grows with the
job.

### Deliberately not done

- **The operation log stays in the header.** It is capped at 2,304 records and
  makes up almost all of the ~560 KB a persist writes, because its paths are
  stored as byte arrays so that non-UTF-8 names survive. It is bounded, so the
  header does not grow with the job, but a smaller persist would need the log
  either journaled too or given a more compact path encoding. Both change a
  format a second time, and neither was needed to meet this ticket.
- **No `fsync`.** The whole-record writer had none either. A power cut can
  still lose the last interval, and on some filesystems a rename, which is the
  loss the throttle already accepts.
- **Recovery still does not resume a job**, for the reason `store.rs` gives:
  a permanent delete's confirmation must not be reconstructable from a file.

### Not verified

- A job of a million items was not run. The million-item claim rests on the
  per-persist cost being flat from 10,001 to 100,001 items. The whole write at
  the start and end of such a job would be about 450 MB of journal, written
  twice. That is the next thing to measure before one is offered.
- A real crash mid-append was simulated by writing a partial line, not by
  killing a process during `write(2)`.
