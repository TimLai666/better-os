//! Remotes: list, fetch, pull, push, delete a remote branch, change URLs,
//! and clone.

use std::fs;
use std::path::Path;

use git_core::{Remote, parse_remotes};

use crate::{GitError, Hooks, NetworkOptions, Run, io_error, refused};

pub fn list(repository: &Path) -> Result<Vec<Remote>, GitError> {
    Ok(parse_remotes(
        &Run::new(repository, ["remote", "-v"]).output()?,
    ))
}

/// `git fetch --prune <remote>`, or `--all` when no remote is named.
pub fn fetch(
    repository: &Path,
    remote: Option<&str>,
    options: &NetworkOptions,
) -> Result<(), GitError> {
    let run = Run::new(repository, ["fetch", "--progress", "--prune", "--tags"]).network(options);
    match remote {
        Some(remote) => run.arg("--end-of-options").arg(remote).run(),
        None => run.arg("--all").run(),
    }
}

/// `git pull --no-edit`, merging unless the person configured
/// `pull.rebase` themselves, as GitHub Desktop does.
pub fn pull(repository: &Path, options: &NetworkOptions) -> Result<(), GitError> {
    let configured = Run::new(repository, ["config", "--get", "pull.rebase"])
        .capture()?
        .status
        .success();
    // Hooks on: the repository is trusted, and Git LFS fetches and checks
    // out its files through `post-checkout` and `post-merge`.
    let mut run = Run::new(repository, ["pull", "--progress", "--no-edit"])
        .hooks(Hooks::On)
        .network(options)
        .env("GIT_EDITOR", "true");
    if !configured {
        run = run.arg("--no-rebase");
    }
    run.run()
}

/// A force push's expectation: the commit the person last saw on the
/// remote branch, or `None` when they saw no such branch there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lease {
    pub expected: Option<String>,
}

/// What a push sends where.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PushRequest {
    pub remote: String,
    /// The local branch.
    pub branch: String,
    /// Its name on the remote: the upstream's, or the same name for a
    /// branch that has none yet.
    pub remote_branch: String,
    /// `-u`, for a branch that has no upstream yet; an upstream the person
    /// set is never rewritten.
    pub set_upstream: bool,
    /// A force push, and what it may overwrite.
    pub force: Option<Lease>,
}

/// `git push [-u] <remote> refs/heads/<branch>:refs/heads/<remote branch>`
/// with the tags in `options.tags`. A force push is
/// `--force-with-lease=<remote ref>:<expected>`, so it overwrites only the
/// commit the person saw, even when a background fetch has moved the
/// remote-tracking branch since. (`--force-if-includes` is a no-op next to
/// an explicit expected value, git's documentation says, so it is not
/// passed.) The repository's
/// hooks run (it is trusted), so Git LFS's `pre-push` uploads the files the
/// commits point at.
pub fn push(
    repository: &Path,
    request: &PushRequest,
    options: &NetworkOptions,
) -> Result<(), GitError> {
    for name in [&request.remote, &request.branch, &request.remote_branch] {
        if name.is_empty() || name.starts_with('-') {
            return Err(refused("that is not a name git accepts"));
        }
    }
    let mut run = Run::new(repository, ["push", "--progress"])
        .hooks(Hooks::On)
        .network(options);
    if request.set_upstream {
        run = run.arg("-u");
    }
    if let Some(lease) = &request.force {
        let expected = lease.expected.as_deref().unwrap_or("");
        if !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(refused("the expected commit is not a commit id"));
        }
        run = run.arg(format!(
            "--force-with-lease=refs/heads/{}:{expected}",
            request.remote_branch
        ));
    }
    run = run
        .arg("--end-of-options")
        .arg(&request.remote)
        .arg(format!(
            "refs/heads/{}:refs/heads/{}",
            request.branch, request.remote_branch
        ));
    for tag in &options.tags {
        run = run.arg(format!("refs/tags/{tag}"));
    }
    run.run()
}

/// Deletes `branch` (its name on the remote) from the remote.
pub fn delete_branch(
    repository: &Path,
    remote: &str,
    branch: &str,
    options: &NetworkOptions,
) -> Result<(), GitError> {
    Run::new(repository, ["push", "--delete", "--end-of-options", remote])
        .arg(format!("refs/heads/{branch}"))
        .network(options)
        .run()
}

fn check_name(name: &str) -> Result<(), GitError> {
    if name.is_empty() || name.starts_with('-') || name.contains(char::is_whitespace) {
        return Err(refused("that is not a remote name git accepts"));
    }
    Ok(())
}

pub fn add(repository: &Path, name: &str, url: &str) -> Result<(), GitError> {
    check_name(name)?;
    Run::new(repository, ["remote", "add", "--", name, url]).run()
}

pub fn remove(repository: &Path, name: &str) -> Result<(), GitError> {
    Run::new(repository, ["remote", "remove", "--end-of-options", name]).run()
}

pub fn set_url(repository: &Path, name: &str, url: &str) -> Result<(), GitError> {
    Run::new(repository, ["remote", "set-url", "--", name, url]).run()
}

/// `git clone -- <url> <destination>`. The destination must not exist or
/// be an empty folder; its parent is made if needed.
pub fn clone(url: &str, destination: &Path, options: &NetworkOptions) -> Result<(), GitError> {
    if !destination.is_absolute() {
        return Err(refused("the destination must be an absolute path"));
    }
    if let Ok(mut entries) = fs::read_dir(destination)
        && entries.next().is_some()
    {
        return Err(refused("the destination folder is not empty"));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| refused("the destination has no parent folder"))?;
    fs::create_dir_all(parent).map_err(io_error)?;
    Run::new(parent, ["clone", "--progress", "--", url])
        .arg(destination)
        .network(options)
        .run()
}
