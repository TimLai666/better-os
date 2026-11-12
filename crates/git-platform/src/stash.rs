//! Stashes: stash everything, restore, discard, and show what one holds.

use std::path::Path;

use git_core::stash::STASH_FORMAT;
use git_core::{CommitFile, FileDiff, Stash, parse_diff, parse_name_status, parse_stashes};

use crate::{GitError, Run, empty_tree, unreadable};

pub fn list(repository: &Path) -> Result<Vec<Stash>, GitError> {
    let captured = Run::new(repository, ["stash", "list", "-z", STASH_FORMAT]).capture()?;
    if !captured.status.success() {
        // A repository with no commit has no stash ref and git says so.
        return Ok(Vec::new());
    }
    parse_stashes(&captured.stdout).map_err(unreadable)
}

/// `git stash push --include-untracked -m <message>`.
pub fn stash_all(repository: &Path, message: &str) -> Result<(), GitError> {
    Run::new(
        repository,
        ["stash", "push", "--include-untracked", "-m", message],
    )
    .without_literal_pathspecs()
    .run()
}

/// `git stash pop <name>`: the stash is dropped only when it applied
/// cleanly; with conflicts git keeps it and this answers `Conflicts`.
pub fn pop(repository: &Path, name: &str) -> Result<(), GitError> {
    Run::new(repository, ["stash", "pop", name])
        .without_literal_pathspecs()
        .run()
}

pub fn drop(repository: &Path, name: &str) -> Result<(), GitError> {
    Run::new(repository, ["stash", "drop", "-q", name]).run()
}

/// The files a stash holds, tracked changes and untracked files together.
pub fn files(repository: &Path, name: &str) -> Result<Vec<CommitFile>, GitError> {
    let out = Run::new(
        repository,
        [
            "stash",
            "show",
            "-z",
            "--name-status",
            "--include-untracked",
            "-M",
            name,
        ],
    )
    .output()?;
    parse_name_status(&out).map_err(unreadable)
}

fn has_untracked(repository: &Path, name: &str, file: &CommitFile) -> Result<bool, GitError> {
    let third = format!("{name}^3");
    let exists = Run::new(repository, ["rev-parse", "--verify", "-q", &third]).capture()?;
    if !exists.status.success() {
        return Ok(false);
    }
    let out = Run::new(repository, ["ls-tree", "--name-only", &third])
        .paths([file.path.as_path()])
        .output()?;
    Ok(!out.is_empty())
}

/// One file's diff in a stash: its tracked change against the commit the
/// stash was made on, or, for an untracked file, the whole file as added.
pub fn diff(repository: &Path, name: &str, file: &CommitFile) -> Result<FileDiff, GitError> {
    let base = ["diff", "--no-color", "--no-ext-diff", "--no-textconv", "-M"];
    let (from, to) = if has_untracked(repository, name, file)? {
        (empty_tree(repository)?, format!("{name}^3"))
    } else {
        (format!("{name}^1"), name.to_string())
    };
    let mut paths = Vec::new();
    if let Some(original) = &file.original_path {
        paths.push(original.as_path());
    }
    paths.push(file.path.as_path());
    let out = Run::new(repository, base)
        .args([from, to])
        .paths(paths)
        .output()?;
    parse_diff(&out).map_err(unreadable)
}

/// After the conflicts of a stash were resolved: the result is unstaged, as
/// a clean `stash pop` leaves it, and the stash is dropped.
pub fn finish_conflict(repository: &Path, name: &str) -> Result<(), GitError> {
    Run::new(repository, ["reset", "-q"]).run()?;
    drop(repository, name)
}

/// Undoes a stash that stopped with conflicts: `git reset --merge` puts back
/// the files it touched and keeps other changes. The stash stays.
pub fn abort_conflict(repository: &Path) -> Result<(), GitError> {
    Run::new(repository, ["reset", "--merge"]).run()
}
