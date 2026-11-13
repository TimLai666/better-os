//! Branches: list, create, switch with or without the changes, rename,
//! delete, merge, rebase, and compare.

use std::path::Path;

use git_core::branch::BRANCH_FORMAT;
use git_core::log::LOG_FORMAT;
use git_core::stash::stash_message;
use git_core::{Branch, Commit, parse_branches, parse_log};

use crate::{GitError, Hooks, Run, unreadable};

/// What happens to uncommitted changes when switching branches, the choice
/// GitHub Desktop offers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwitchChanges {
    /// Plain `git switch`: git carries the changes when it can and refuses
    /// when they would be overwritten.
    Carry,
    /// Leave them on the current branch: stash them under its name first.
    Leave,
    /// Bring them along: stash, switch, and pop the stash on the new branch.
    Bring,
}

/// `git for-each-ref` over local and remote-tracking branches.
pub fn list(repository: &Path) -> Result<Vec<Branch>, GitError> {
    let out = Run::new(
        repository,
        [
            "for-each-ref",
            BRANCH_FORMAT,
            "--sort=-committerdate",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .output()?;
    parse_branches(&out).map_err(unreadable)
}

/// `origin/HEAD`'s target as `origin/main`, when the remote said.
pub fn remote_head(repository: &Path) -> Option<String> {
    let out = Run::new(
        repository,
        ["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"],
    )
    .output()
    .ok()?;
    let text = String::from_utf8_lossy(&out).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The current branch's name, or `None` when HEAD is detached.
pub fn current(repository: &Path) -> Result<Option<String>, GitError> {
    let captured = Run::new(repository, ["symbolic-ref", "-q", "--short", "HEAD"]).capture()?;
    Ok(captured
        .status
        .success()
        .then(|| String::from_utf8_lossy(&captured.stdout).trim().to_string()))
}

/// `git branch <name> [<start>]`, then `git switch <name>` when asked. A
/// start that is a remote-tracking branch sets it as the upstream.
pub fn create(
    repository: &Path,
    name: &str,
    start: Option<&str>,
    checkout: bool,
) -> Result<(), GitError> {
    if name.starts_with('-') {
        return Err(crate::refused("a branch name cannot start with -"));
    }
    let mut run = Run::new(repository, ["branch", "--end-of-options", name]);
    if let Some(start) = start {
        run = run.arg(start);
    }
    run.run()?;
    if checkout {
        switch_to(repository, &["--end-of-options", name])?;
    }
    Ok(())
}

/// `git switch`, with the repository's hooks (it is trusted; Git LFS checks
/// out its files from `post-checkout`).
fn switch_to(repository: &Path, args: &[&str]) -> Result<(), GitError> {
    Run::new(repository, ["switch"])
        .args(args)
        .hooks(Hooks::On)
        .run()
}

/// The commit `refs/stash` points at, or `None` when there is no stash.
fn stash_tip(repository: &Path) -> Result<Option<String>, GitError> {
    let captured = Run::new(repository, ["rev-parse", "--verify", "-q", "refs/stash"]).capture()?;
    Ok(captured
        .status
        .success()
        .then(|| String::from_utf8_lossy(&captured.stdout).trim().to_string()))
}

/// The `stash@{n}` name of the stash whose commit is `sha`, if it is still
/// in the list.
fn stash_named(repository: &Path, sha: &str) -> Result<Option<String>, GitError> {
    let out = Run::new(repository, ["stash", "list", "--format=%H %gd"]).output()?;
    Ok(String::from_utf8_lossy(&out).lines().find_map(|line| {
        let (commit, name) = line.split_once(' ')?;
        (commit == sha).then(|| name.to_string())
    }))
}

/// Pops the stash made for this switch, found by its commit: `stash@{0}`
/// may be someone else's by then, or an older one when nothing was stashed.
fn pop_made(repository: &Path, sha: &str) -> Result<(), GitError> {
    match stash_named(repository, sha)? {
        Some(name) => crate::stash::pop(repository, &name),
        None => Ok(()),
    }
}

fn ref_exists(repository: &Path, full: &str) -> Result<bool, GitError> {
    let captured = Run::new(repository, ["show-ref", "--verify", "-q", full]).capture()?;
    Ok(captured.status.success())
}

/// Switches to `target`: a local branch; a remote-tracking branch (`origin/x`
/// with no local `x`), which makes a local `x` that tracks it; or anything
/// else git can resolve, as a detached HEAD.
pub fn switch(repository: &Path, target: &str, changes: SwitchChanges) -> Result<(), GitError> {
    // The stash made here, by its commit: `git stash push` makes none for
    // changes it cannot stash (a submodule's own changes), and then
    // `stash@{0}` is an older stash that must not be popped.
    let stashed = match changes {
        SwitchChanges::Carry => None,
        SwitchChanges::Leave | SwitchChanges::Bring => {
            let branch = current(repository)?.unwrap_or_else(|| "HEAD".to_string());
            let has_changes = !Run::new(repository, ["status", "--porcelain", "-z"])
                .output()?
                .is_empty();
            if has_changes {
                let before = stash_tip(repository)?;
                crate::stash::stash_all(repository, &stash_message(&branch))?;
                stash_tip(repository)?.filter(|after| Some(after) != before.as_ref())
            } else {
                None
            }
        }
    };
    let result = if ref_exists(repository, &format!("refs/heads/{target}"))? {
        switch_to(repository, &["--end-of-options", target])
    } else if ref_exists(repository, &format!("refs/remotes/{target}"))? {
        let local = target.split_once('/').map_or(target, |(_, name)| name);
        if ref_exists(repository, &format!("refs/heads/{local}"))? {
            switch_to(repository, &["--end-of-options", local])
        } else {
            switch_to(
                repository,
                &["-c", local, "--track", "--end-of-options", target],
            )
        }
    } else {
        switch_to(repository, &["--detach", "--end-of-options", target])
    };
    if let Err(error) = result {
        if let Some(sha) = &stashed {
            // Put the changes back where they were.
            let _ = pop_made(repository, sha);
        }
        return Err(error);
    }
    if let Some(sha) = stashed.filter(|_| changes == SwitchChanges::Bring) {
        pop_made(repository, &sha)?;
    }
    Ok(())
}

pub fn rename(repository: &Path, from: &str, to: &str) -> Result<(), GitError> {
    if to.starts_with('-') {
        return Err(crate::refused("a branch name cannot start with -"));
    }
    Run::new(repository, ["branch", "-m", "--end-of-options", from, to]).run()
}

/// `git branch -d`, or `-D` to delete a branch that is not merged.
pub fn delete(repository: &Path, name: &str, force: bool) -> Result<(), GitError> {
    Run::new(
        repository,
        [
            "branch",
            if force { "-D" } else { "-d" },
            "--end-of-options",
            name,
        ],
    )
    .run()
}

/// `git merge --no-edit <branch>`, or `--squash` then a commit with git's
/// prepared message. A squash merge that stops on conflicts leaves
/// `SQUASH_MSG`, which [`crate::conflicts::operation`] reads as a squash
/// merge in progress. The repository's hooks run: these make commits.
pub fn merge(repository: &Path, branch: &str, squash: bool) -> Result<(), GitError> {
    if squash {
        Run::new(
            repository,
            ["merge", "--squash", "--end-of-options", branch],
        )
        .hooks(Hooks::On)
        .run()?;
        Run::new(repository, ["commit", "--no-edit"])
            .hooks(Hooks::On)
            .env("GIT_EDITOR", "true")
            .run()
    } else {
        Run::new(
            repository,
            ["merge", "--no-edit", "--end-of-options", branch],
        )
        .hooks(Hooks::On)
        .env("GIT_EDITOR", "true")
        .run()
    }
}

/// `git rebase <onto>`.
pub fn rebase(repository: &Path, onto: &str) -> Result<(), GitError> {
    Run::new(repository, ["rebase", "--end-of-options", onto])
        .hooks(Hooks::On)
        .env("GIT_EDITOR", "true")
        .run()
}

/// The commits in `head` that are not in `base`, newest first, and how many
/// commits `base` has that `head` does not.
pub fn compare(
    repository: &Path,
    base: &str,
    head: &str,
) -> Result<(Vec<Commit>, usize), GitError> {
    let out = Run::new(repository, ["log", "-z", LOG_FORMAT, "--max-count=500"])
        .arg("--end-of-options")
        .arg(format!("{base}..{head}"))
        .arg("--")
        .output()?;
    let ahead = parse_log(&out).map_err(unreadable)?;
    let count = Run::new(repository, ["rev-list", "--count", "--end-of-options"])
        .arg(format!("{head}..{base}"))
        .arg("--")
        .output()?;
    let behind = String::from_utf8_lossy(&count).trim().parse().unwrap_or(0);
    Ok((ahead, behind))
}
