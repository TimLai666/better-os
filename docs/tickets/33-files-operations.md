# 33 — files-operations: durable job engine and the basic file operations

**Epic:** Better Files (Issue #6)
**User Story:** A copy keeps running when its window closes, says exactly which
items failed and why, and asks about a name conflict once instead of a thousand
times.
**Blocked by:** 32-files-core-navigation
**Status:** implemented on `ticket-33`, in review

## Goal

Build file operations as durable jobs, not as work attached to a window. Issue
#6 states the rule plainly: do not tie file operations to one window's lifetime.

## What it delivers

- `files-operations`: one shared job engine with the states Issue #6 lists —
  queued, running, paused, waiting, completed, failed, cancelled, and rolled
  back — plus per-item and aggregate progress, current throughput, a
  remaining-time estimate carrying its own confidence, and an operation log.
- Conflict resolution that surfaces the decision once and can apply one choice
  to the remaining conflicts, with skip and retry for failed items.
- Persistence: a job's state survives a UI restart, and a crashed window leaves
  no job in an unknowable state.
- The first operations: create file and folder, rename, copy, move, duplicate,
  trash, restore from Trash, and permanent delete behind explicit confirmation.
- Non-blocking execution with an operation queue and a documented concurrency
  policy, and pause/resume where the underlying operation permits it.
- Final-state verification: metadata and result are checked after the job, and
  partial copy or move behavior is documented rather than assumed.
- A documented policy for timestamps, permissions, ACLs, xattrs, sparse-file
  behavior, and links.
- Failed operations retain useful error detail and the affected paths, and
  destructive actions have a restore path where one is technically possible.
- Enough recorded evidence per job for Better Monitor to analyze a slow
  operation later.
- No shell-string concatenation anywhere in the operation path.

## Out of scope

- Archive, extract, and checksum jobs, and bulk rename. The engine must not
  prevent them.
- The operation-center UI and every other screen (ticket 34).
- Better Copy itself; this ticket delivers the engine Better Copy later
  integrates with rather than a second copy implementation.
- Undo for operations where no safe compensating action exists.

## Deferred decisions

Issue #6 requires an ADR or a focused follow-up issue rather than a silent
choice for: whether operation jobs persist across a full logout or reboot, and
the exact Better Copy boundary. Job persistence in this ticket covers a UI
restart; anything beyond that stays open.

## Acceptance criteria

- [x] All eight job states are representable and each has a test.
- [x] A job exposes per-item and aggregate progress, throughput, and a
      remaining-time estimate with its confidence.
- [x] Create, rename, copy, move, duplicate, trash, restore, and permanent
      delete all run as jobs.
- [x] Permanent delete requires explicit confirmation.
- [x] A conflict decision can be applied to all remaining conflicts in the job.
- [x] Failed items can be retried and skipped individually.
- [x] Jobs are not tied to a window's lifetime, proven by a test that drops the
      owning UI handle mid-job and finds the job completed.
- [x] A crashed or closed window leaves no job in an unknowable state.
- [x] Every job verifies its final state, and partial copy or move behavior is
      documented and tested.
- [x] The metadata preservation policy is documented and enforced by tests.
- [x] A failed operation retains its error detail and affected paths.
- [x] No file operation constructs a shell string.

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- A job-persistence test: start a copy, terminate the owning process, restart,
  and confirm the job's recorded state is consistent and resumable
- Failure-injection tests for full disks, permission errors, a device that
  disappears mid-copy, and a source that changes under the job
- `cargo bench -p files-operations`: one large sequential copy, a 100,000
  small-file copy, a same-filesystem move, and a cross-filesystem move, recording
  completion and persistence time

## What was built

One new crate, `files-operations`, and the trash write side added to
`files-platform` beside the read side ticket 32 left there. The whole policy —
metadata, partial transfers, conflicts, concurrency, recovery, and the measured
cost — is written down in `docs/files-operations-policy.md`.

The load-bearing decisions:

- **A handle owns nothing.** `JobHandle` is an identifier and an event stream
  with no `Drop`. The engine owns its jobs; dropping every handle to a running
  copy does nothing to it. Dropping the engine waits for running jobs and
  cancels only the ones parked on a pause or a conflict, because a parked job
  has nobody left to answer it.
- **A destination appears whole or not at all.** Every file is written to a
  temporary name in the destination directory and renamed into place after its
  bytes, metadata, and verification are complete. Cancelling mid-copy therefore
  leaves nothing, not a truncated file.
- **A permanent delete's confirmation cannot be forged.** `DeleteConfirmation`
  has no public field, no `Default`, and no `Deserialize`, the same shape as
  `storage_core`'s readiness proof. A recovered job cannot resurrect one, which
  is why recovery reports remaining work instead of resuming.
- **Nothing is a `String`.** Names travel as `OsString` and paths as `PathBuf`
  through the spec, the plan, the executor, the log, the error taxonomy, and
  the persisted record, which serializes paths as bytes. Only `Display` is
  lossy.
- **A conflict answer carries its own scope.** One decision answers this item or
  every remaining conflict of the same kind. Standing answers are keyed by kind,
  so "overwrite every existing file" says nothing about a full disk.

Scope notes against the ticket text:

- Bulk rename and checksum are listed under Out of scope here and under
  operations in Issue #6. Both are implemented, because the pattern engine and
  the digest are each about a hundred lines once single rename and chunked
  reading exist, and leaving them out would have meant shipping an untested
  pattern engine into ticket 35. Archive and extract are **not** implemented:
  they are the two that need a real format decision, and the engine does not
  prevent them — an archive job is another `Operation` variant and another arm
  in `execute_item`.
- Job persistence covers a UI restart, as scoped. Surviving a logout or a reboot
  stays deferred.

## Verification actually run

Crate-scoped, to avoid rebuilding GPUI in the worktree; the workspace gate
belongs on the main checkout after merge.

- `cargo fmt --all -- --check` — passed
- `cargo check -p files-operations -p files-platform -p files-core --all-targets`
  — passed
- `cargo clippy -p files-operations -p files-platform -p files-core
  --all-targets -- -D warnings` — passed
- `cargo test -p files-operations -p files-platform -p files-core` — 289 tests
  passed, and the whole suite was run ten more times to shake out timing
  flakes. Three were found and fixed by making the paused-mid-copy tests
  deterministic instead of hoping the copy was slow enough.
- `cargo bench -p files-operations` — numbers recorded in
  `docs/files-operations-policy.md`, and the benchmark earned its keep: it
  caught the record being rewritten after every item, which was quadratic and
  cost ten times the copy itself on a 201-item job. Records are now written at
  most every 250 ms while a job runs, plus on every state change and at the end.

New tests: 134 in `files-operations` (61 unit, 73 integration across seven
files) and 11 added to `files-platform` for the trash write side.

## What is not proven, and why

- **A full disk, an exhausted quota, and a disappearing device** are covered at
  the classification level only. Producing them needs a filesystem the suite may
  not create or hardware the host does not have. All the errno values the kernel
  produces for each are covered.
- **A cross-filesystem move and the cross-filesystem trash fallback** are forced
  by a policy flag rather than by a second mount, because mounting one needs
  privilege the suite must not ask for. The code path is identical; what is
  simulated is the `EXDEV` that selects it.
- **A case conflict** is modelled and classified, and needs a case-insensitive
  mount to exercise end to end.
- **The large-file benchmark numbers are page-cache numbers.** 4.3 GB/s is
  memory bandwidth. A real spinning disk, a real USB device with `fsync` per
  file, and a network share are all unmeasured.
- **Per-device `.Trash` and `.Trash-$uid`** are not implemented. The home-trash
  fallback is, and the gap is documented rather than papered over.
- **Hard links are not preserved** between separately copied files. It needs a
  job-wide inode map.

## Follow-ups this ticket creates

- Decide whether a job should survive a logout or a reboot (Issue #6 defers it).
- Decide whether Better Copy becomes a front end on this engine or something
  else; the ticket names the boundary as still open.
- Give the trash a per-device `.Trash-$uid` path so a removable disk's deletions
  stay on the disk.
- Preserve hard links within a job.
- Measure a real device before Better Files claims copy performance.
- Replace the full-record rewrite with an append-only item journal before a job
  of a million items is offered. The record for 10,001 items is 17.5 MB, and it
  grows linearly.
