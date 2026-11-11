//! Running the system `git` for Better Git.
//!
//! Every operation is one `git` process started with an argument vector and
//! `-C <repository>`. Nothing goes through a shell, every path argument follows
//! `--` so a file named `-x` can never be read as an option, and `LC_ALL=C` is
//! set so that what little prose git prints is in one language. The person's
//! own configuration, credential helper, and SSH keys are left as they are:
//! this crate adds no sign-in of its own.
//!
//! The parsing lives in `git-core`. This crate only decides which command to
//! run and hands the bytes over.

use std::path::{Path, PathBuf};

use git_core::{FileDiff, RepositoryStatus};
use thiserror::Error;

/// Why an operation did not happen.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum GitError {
    /// No `git` program could be started on this machine.
    #[error("git.error.not_installed")]
    NotInstalled,
    /// The path is not inside a git working tree.
    #[error("git.error.not_a_repository:{0}")]
    NotARepository(PathBuf),
    /// git ran and refused. `message` is its own stderr, trimmed, for the
    /// person to read; nothing decides behaviour from it.
    #[error("git.error.failed:{message}")]
    Failed {
        exit_code: Option<i32>,
        message: String,
    },
    /// git printed something `git-core` could not read.
    #[error("git.error.unreadable_output:{0}")]
    UnreadableOutput(String),
}

/// Which side of a file's change to show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffSide {
    /// Working tree against the index.
    Unstaged,
    /// Index against HEAD (or against nothing, in a repository with no commit).
    Staged,
}

/// What a commit needs from the person.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitMessage {
    pub summary: String,
    /// Optional; an empty string means no description.
    pub description: String,
}

/// The operations the first Better Git window uses. A test gives the window a
/// fake of this trait; `SystemGit` is the real one.
pub trait Git: Send + Sync {
    /// The root of the working tree that contains `path`.
    fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError>;
    fn status(&self, repository: &Path) -> Result<RepositoryStatus, GitError>;
    /// The diff of one path, relative to the repository root.
    fn diff(&self, repository: &Path, path: &Path, side: DiffSide) -> Result<FileDiff, GitError>;
    /// Stages the paths as they are in the working tree, deletions included.
    fn stage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError>;
    /// Takes the paths back out of the index and leaves the working tree alone.
    fn unstage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError>;
    /// Commits what is staged.
    fn commit(&self, repository: &Path, message: &CommitMessage) -> Result<(), GitError>;
}

/// Runs the `git` found on `PATH`.
#[derive(Clone, Debug, Default)]
pub struct SystemGit;

/// Expected results, for every method:
/// - `git` cannot be started → `GitError::NotInstalled`.
/// - git exits non-zero → `GitError::Failed` with its trimmed stderr, or its
///   trimmed stdout when stderr is empty (`git commit` reports "nothing to
///   commit" on stdout), except where a method below says otherwise.
/// - Arguments are passed as a vector; paths are passed as `OsStr`, never
///   converted through a string, and always after `--`.
impl Git for SystemGit {
    /// `git -C <path> rev-parse --show-toplevel`. A path that is not in a
    /// working tree (git exits 128) is `NotARepository(path)`. A path that
    /// does not exist is `NotARepository(path)` too.
    fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError> {
        let _ = path;
        todo!("ticket 77: SystemGit::repository_root")
    }

    /// `git status --porcelain=v2 -z --branch --untracked-files=all`, parsed
    /// by `git_core::parse_status`. A parse failure is `UnreadableOutput`.
    fn status(&self, repository: &Path) -> Result<RepositoryStatus, GitError> {
        let _ = repository;
        todo!("ticket 77: SystemGit::status")
    }

    /// `git diff --no-color --no-ext-diff [--cached] -- <path>`, parsed by
    /// `git_core::parse_diff`. An untracked file has no diff from git, so for
    /// `Unstaged` on an untracked path this shows the whole file as added:
    /// `git diff --no-color --no-ext-diff --no-index -- /dev/null <path>`,
    /// whose exit code 1 means "differences found" and is not a failure.
    fn diff(&self, repository: &Path, path: &Path, side: DiffSide) -> Result<FileDiff, GitError> {
        let _ = (repository, path, side);
        todo!("ticket 77: SystemGit::diff")
    }

    /// `git add -A -- <paths>`. No paths is a no-op that runs nothing.
    fn stage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError> {
        let _ = (repository, paths);
        todo!("ticket 77: SystemGit::stage")
    }

    /// `git restore --staged -- <paths>`; in a repository with no commit yet,
    /// `git rm --cached -r -q -- <paths>`, because there is no HEAD to restore
    /// from. No paths is a no-op that runs nothing.
    fn unstage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError> {
        let _ = (repository, paths);
        todo!("ticket 77: SystemGit::unstage")
    }

    /// `git commit -F -` with the message on stdin: the summary, and when the
    /// description is not empty, a blank line and the description. An empty or
    /// whitespace-only summary is refused with `Failed` before git runs.
    fn commit(&self, repository: &Path, message: &CommitMessage) -> Result<(), GitError> {
        let _ = (repository, message);
        todo!("ticket 77: SystemGit::commit")
    }
}
