//! `git status --porcelain=v2 -z --branch`, read into a changes list.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::ParseError;

/// What happened to one path on one side (the index, or the working tree).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    /// The file's type changed, for example a file became a symbolic link.
    TypeChanged,
}

/// One changed path.
///
/// A path can be changed on both sides at once: staged, and then edited again.
/// It is one entry here with both sides set, the way git reports it, and the
/// window shows it in both the staged and the unstaged list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileChange {
    /// The path relative to the repository root, as bytes git printed. Not
    /// required to be UTF-8.
    pub path: PathBuf,
    /// For a rename or a copy, the path it came from.
    pub original_path: Option<PathBuf>,
    /// The change recorded in the index, if any.
    pub staged: Option<ChangeKind>,
    /// The change in the working tree that is not in the index, if any.
    pub unstaged: Option<ChangeKind>,
    /// Not tracked by git at all. `staged` and `unstaged` are both `None`.
    pub untracked: bool,
    /// An unmerged path. `staged` and `unstaged` are both `None`.
    pub conflicted: bool,
}

/// Where HEAD is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BranchHead {
    /// On a branch. The name is what `# branch.head` printed.
    Branch(String),
    /// Detached. git prints `(detached)` and this carries no name.
    Detached,
}

/// Everything one status call reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryStatus {
    pub head: BranchHead,
    /// True when the repository has no commit yet (`# branch.oid (initial)`).
    pub unborn: bool,
    /// The upstream branch name, when one is configured.
    pub upstream: Option<String>,
    /// Commits ahead of and behind the upstream, when git reported them.
    pub ahead_behind: Option<(u32, u32)>,
    /// Changed paths in the order git printed them.
    pub changes: Vec<FileChange>,
}

/// Reads the NUL-separated output of
/// `git status --porcelain=v2 -z --branch --untracked-files=all`.
///
/// Expected results:
/// - `# branch.head <name>` sets `head`; `(detached)` is `BranchHead::Detached`.
/// - `# branch.oid (initial)` sets `unborn`.
/// - `# branch.upstream <name>` and `# branch.ab +A -B` set `upstream` and
///   `ahead_behind`. Both are optional.
/// - A `1` record sets `staged` from X and `unstaged` from Y of its `XY`
///   field; `.` means no change on that side. `M`, `A`, `D`, `T` map to
///   Modified, Added, Deleted, TypeChanged.
/// - A `2` record is a rename (`R`) or a copy (`C`); the path is in the
///   record and the original path is the next NUL-separated field.
/// - A `u` record is conflicted. A `?` record is untracked. `!` records
///   (ignored) are skipped.
/// - A path is everything after the fixed fields of its record, taken as raw
///   bytes: spaces, non-ASCII bytes, and a leading `-` are part of the path.
/// - Empty input is a repository with nothing to report: it fails with
///   `Malformed`, because `--branch` always prints `# branch.head`.
/// - A record type this function does not know, or a record with too few
///   fields, fails with `Malformed` naming the record type.
pub fn parse_status(output: &[u8]) -> Result<RepositoryStatus, ParseError> {
    let _ = (output, OsString::new());
    todo!("ticket 77: parse_status")
}
