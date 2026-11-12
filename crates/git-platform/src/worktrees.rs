//! Worktrees: list, add, rename (move), and remove.

use std::path::Path;

use git_core::{Worktree, parse_worktrees};

use crate::{GitError, Run, refused, unreadable};

pub fn list(repository: &Path) -> Result<Vec<Worktree>, GitError> {
    let out = Run::new(repository, ["worktree", "list", "--porcelain", "-z"]).output()?;
    parse_worktrees(&out).map_err(unreadable)
}

fn absolute(path: &Path) -> Result<&Path, GitError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(refused("a worktree's folder must be an absolute path"))
    }
}

/// `git worktree add <path> <branch>`, or `-b <branch>` to make the branch.
pub fn add(repository: &Path, path: &Path, branch: &str, create: bool) -> Result<(), GitError> {
    let path = absolute(path)?;
    let run = Run::new(repository, ["worktree", "add"]);
    if create {
        run.args(["-b", branch, "--end-of-options"]).arg(path).run()
    } else {
        run.arg("--end-of-options").arg(path).arg(branch).run()
    }
}

/// `git worktree move`: a worktree's folder is its name.
pub fn rename(repository: &Path, from: &Path, to: &Path) -> Result<(), GitError> {
    Run::new(repository, ["worktree", "move"])
        .arg(absolute(from)?)
        .arg(absolute(to)?)
        .run()
}

pub fn remove(repository: &Path, path: &Path, force: bool) -> Result<(), GitError> {
    let mut run = Run::new(repository, ["worktree", "remove"]);
    if force {
        run = run.arg("--force");
    }
    run.arg(absolute(path)?).run()
}
