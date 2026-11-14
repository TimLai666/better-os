//! Worktrees in the repository list and the branch list.
//!
//! A repository can have linked worktrees: other folders with their own
//! branch checked out, sharing one history. Git will not check a branch out
//! in two worktrees at once, so a branch that is checked out in another
//! worktree cannot be switched to here; choosing it opens that worktree
//! instead. The repository list shows each repository's linked worktrees
//! under it, so they are found where repositories are found.

use std::path::{Path, PathBuf};

use git_core::Worktree;

/// One linked worktree under its repository in the repository list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkedRow {
    pub path: PathBuf,
    /// The branch checked out there, or `None` when it is detached.
    pub branch: Option<String>,
    /// It is the folder open in the window.
    pub current: bool,
    /// Its folder is gone; git would prune it.
    pub missing: bool,
}

/// Whether two paths name the same folder, through symbolic links.
pub fn same_folder(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The linked worktrees of a repository, in git's order: every worktree but
/// the main one and bare entries.
pub fn linked_rows(worktrees: &[Worktree], current: Option<&Path>) -> Vec<LinkedRow> {
    worktrees
        .iter()
        .filter(|w| !w.main && !w.bare)
        .map(|w| LinkedRow {
            path: w.path.clone(),
            branch: w.branch.clone(),
            current: current.is_some_and(|c| same_folder(c, &w.path)),
            missing: w.prunable,
        })
        .collect()
}

/// The worktree other than `here` that has `branch` checked out, if any.
pub fn checked_out_elsewhere(branch: &str, worktrees: &[Worktree], here: &Path) -> Option<PathBuf> {
    worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some(branch) && !w.bare && !same_folder(&w.path, here))
        .map(|w| w.path.clone())
}
