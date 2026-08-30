# 33 — files-operations: durable job engine and the basic file operations

**Epic:** Better Files (Issue #6)
**User Story:** A copy keeps running when its window closes, says exactly which
items failed and why, and asks about a name conflict once instead of a thousand
times.
**Blocked by:** 32-files-core-navigation
**Status:** todo

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

- [ ] All eight job states are representable and each has a test.
- [ ] A job exposes per-item and aggregate progress, throughput, and a
      remaining-time estimate with its confidence.
- [ ] Create, rename, copy, move, duplicate, trash, restore, and permanent
      delete all run as jobs.
- [ ] Permanent delete requires explicit confirmation.
- [ ] A conflict decision can be applied to all remaining conflicts in the job.
- [ ] Failed items can be retried and skipped individually.
- [ ] Jobs are not tied to a window's lifetime, proven by a test that drops the
      owning UI handle mid-job and finds the job completed.
- [ ] A crashed or closed window leaves no job in an unknowable state.
- [ ] Every job verifies its final state, and partial copy or move behavior is
      documented and tested.
- [ ] The metadata preservation policy is documented and enforced by tests.
- [ ] A failed operation retains its error detail and affected paths.
- [ ] No file operation constructs a shell string.

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
