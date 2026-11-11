//! What Better Git knows about a repository, and how it reads git's own
//! machine-readable output.
//!
//! Nothing here runs a process or touches a file. `git-platform` runs `git`
//! and hands the bytes it printed to the parsers in this crate, so every rule
//! about what a line of git output means is testable from recorded output.
//!
//! Only porcelain formats are parsed: `status --porcelain=v2 -z --branch` and
//! unified diffs produced with `--no-color`. Text git writes for people is
//! never read for meaning.

pub mod diff;
pub mod status;

pub use diff::{DiffLine, DiffLineKind, FileDiff, Hunk, parse_diff};
pub use status::{BranchHead, ChangeKind, FileChange, RepositoryStatus, parse_status};

use thiserror::Error;

/// A line of git output this crate could not read as the format it expected.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ParseError {
    /// The record does not have the shape the format documents. The detail
    /// names the record type and what was missing, never file contents.
    #[error("git.parse.malformed:{0}")]
    Malformed(String),
}
