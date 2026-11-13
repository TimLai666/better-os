//! What Better Git knows about a repository, and how it reads git's own
//! machine-readable output.
//!
//! Nothing here runs a process or touches a file. `git-platform` runs `git`
//! and hands the bytes it printed to the parsers in this crate, so every rule
//! about what a line of git output means is testable from recorded output.
//!
//! Only machine-readable formats are parsed: `status --porcelain=v2 -z
//! --branch`, unified diffs produced with `--no-color`, and `log`,
//! `for-each-ref`, `stash list`, and `worktree list` with explicit formats.
//! The one exception is [`remote::classify_refusal`], which picks an
//! explanation for a refused network operation from git's message, because
//! git prints nothing else for it; git's message is always shown with it.

pub mod branch;
pub mod conflict;
pub mod diff;
pub mod image;
pub mod log;
pub mod patch;
pub mod remote;
pub mod stash;
pub mod status;
pub mod tag;
pub mod worktree;

pub use branch::{Branch, NameProblem, check_branch_name, parse_branches, sanitize_ref_name};
pub use conflict::{Operation, count_conflict_markers};
pub use diff::{
    DiffLine, DiffLineKind, FileDiff, Hunk, LineKey, SplitRow, change_fingerprint, is_large,
    line_key, parse_diff, split_rows,
};
pub use image::{ImageKind, image_kind, sniff_image};
pub use log::{Commit, CommitFile, parse_log, parse_name_status};
pub use patch::{PatchMode, RawDiff};
pub use remote::{
    HostedRepository, Refusal, Remote, classify_refusal, parse_hosted, parse_remotes,
};
pub use stash::{Stash, parse_stashes};
pub use status::{
    BranchHead, ChangeKind, ConflictKind, FileChange, RepositoryStatus, parse_status,
};
pub use tag::{Tag, check_tag_name, parse_tags};
pub use worktree::{Worktree, parse_worktrees};

use thiserror::Error;

/// A line of git output this crate could not read as the format it expected.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ParseError {
    /// The record does not have the shape the format documents. The detail
    /// names the record type and what was missing, never file contents.
    #[error("git.parse.malformed:{0}")]
    Malformed(String),
}
