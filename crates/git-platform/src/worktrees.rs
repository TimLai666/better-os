//! Worktrees: list, add, rename (move), remove, lock, unlock, and prune.

use std::path::{Path, PathBuf};

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

/// `git worktree remove`, with one `--force` at most: that lets go of a
/// worktree with changes, never of a locked one (Git wants it twice for
/// that, and Better Git never asks twice; it is unlocked first).
pub fn remove(repository: &Path, path: &Path, force: bool) -> Result<(), GitError> {
    let mut run = Run::new(repository, ["worktree", "remove"]);
    if force {
        run = run.arg("--force");
    }
    run.arg(absolute(path)?).run()
}

/// `git worktree lock`, with `--reason` when one is given: Git then will
/// not move, remove, or prune it.
pub fn lock(repository: &Path, path: &Path, reason: Option<&str>) -> Result<(), GitError> {
    let mut run = Run::new(repository, ["worktree", "lock"]);
    if let Some(reason) = reason.map(str::trim).filter(|r| !r.is_empty()) {
        run = run.args(["--reason", reason]);
    }
    run.arg("--end-of-options").arg(absolute(path)?).run()
}

pub fn unlock(repository: &Path, path: &Path) -> Result<(), GitError> {
    Run::new(repository, ["worktree", "unlock", "--end-of-options"])
        .arg(absolute(path)?)
        .run()
}

/// `git worktree prune`: Git forgets every worktree whose folder is gone
/// and is not locked. It takes no folder, so it cannot be narrowed to one;
/// the caller names every worktree it would clear before asking.
pub fn prune(repository: &Path) -> Result<(), GitError> {
    Run::new(repository, ["worktree", "prune"]).run()
}

/// `core.worktree` in a git directory: where its main worktree is, as Git
/// records it for a submodule (relative to the git directory) or a
/// repository whose working tree was moved. `None` when it is not set.
pub fn core_worktree(git_dir: &Path) -> Result<Option<PathBuf>, GitError> {
    match Run::new(git_dir, ["config", "--get", "core.worktree"]).output() {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out);
            let value = text.trim_end_matches(['\n', '\r']);
            Ok((!value.is_empty()).then(|| git_dir.join(value)))
        }
        // `git config --get` exits 1 when the key is not set.
        Err(GitError::Failed {
            exit_code: Some(1), ..
        }) => Ok(None),
        Err(error) => Err(error),
    }
}
