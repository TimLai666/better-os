//! Better Files' durable job engine and its file operations.
//!
//! Issue #6 states the rule this crate exists to enforce: **do not tie file
//! operations to one window's lifetime**. A copy is not work a window is
//! doing. It is a job the engine owns, with its own state, its own progress,
//! its own log, and a record on disk that outlives the process that started
//! it. Closing the window that started a copy does nothing to the copy.
//!
//! ## The shape of it
//!
//! - [`spec`] — what a job was asked to do. Pure data: no closures, no command
//!   strings, no window handles. A permanent delete cannot be built without a
//!   [`spec::DeleteConfirmation`], which has no `Deserialize` and so cannot be
//!   conjured from a file.
//! - [`plan`] — the walk that turns a spec into a counted list of items, with
//!   the visited set that makes a symlink loop terminate.
//! - [`policy`] — the documented copy correctness policy: timestamps,
//!   permissions, ACLs, extended attributes, sparse files, links, durability,
//!   and what happens to a partially finished copy.
//! - [`fsops`] — the syscalls. Temporary name plus atomic rename, so a
//!   cancelled copy leaves nothing; `SEEK_HOLE`/`SEEK_DATA`, so a sparse file
//!   stays sparse.
//! - [`exec`] — the operations themselves, driven through the [`exec::JobControl`]
//!   trait so they can be tested with no engine at all.
//! - [`engine`] — the worker pool, the pause condition variable, the conflict
//!   parking, and the [`engine::JobHandle`] that owns nothing.
//! - [`store`] — the record on disk, and what recovery does with one whose
//!   process died.
//!
//! ## What it does not do
//!
//! There is no `std::process::Command` anywhere in this crate and no string is
//! ever assembled into a command line. Every operation is a syscall on a path,
//! which is how Issue #6's "no shell-string concatenation" requirement is met
//! by construction. `tests/no_shell_strings.rs` checks it on every run.
//!
//! Archive and extract are not here. Ticket 33 puts them out of scope and the
//! engine does not prevent them: an archive job is another [`spec::Operation`]
//! variant and another arm in [`exec::execute_item`].

pub mod checksum;
pub mod conflict;
pub mod engine;
pub mod error;
pub mod exec;
pub mod fsops;
pub mod log;
pub mod plan;
pub mod policy;
pub mod progress;
pub mod spec;
pub mod state;
pub mod store;

pub use conflict::{
    Conflict, ConflictDecision, ConflictKind, ConflictPolicy, Resolution, ResolutionScope,
};
pub use engine::{EngineConfig, JobEngine, JobEvent, JobHandle, JobId, JobSnapshot};
pub use error::OperationError;
pub use exec::{ItemOutcome, JobControl, build_plan, preview_bulk_rename};
pub use log::{LogEvent, LogRecord, MetadataProperty, OperationLog, SkipReason};
pub use plan::{ItemKind, Plan, PlanItem};
pub use policy::{
    CopyPolicy, DestinationDurability, FailurePolicy, FsyncPolicy, MoveStrategy, SparsePolicy,
    SymlinkPolicy,
};
pub use progress::{Confidence, ItemProgress, Progress, RemainingTime, Throughput};
pub use spec::{
    ChecksumAlgorithm, DeleteConfirmation, DeleteTarget, JobSpec, Operation, OperationKind,
    RenamePattern, TrashItemRef,
};
pub use state::JobState;
pub use store::{ItemRecord, ItemStatus, JobRecord, JobStore, Recovery, StoreError};
