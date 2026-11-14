//! What Better Git needs to keep its window current without being asked:
//! whether Git knows who is committing, where the repository's own
//! directories are, and which directories Git ignores (ticket 103).

use std::path::{Path, PathBuf};

use crate::{GitError, Run};

/// Whether Git can name the author and committer of a commit made here,
/// as `git commit` would: `git var` applies the same strict rules. An
/// unknown identity is `Ok(false)`, not an error.
pub fn identity_known(repository: &Path) -> Result<bool, GitError> {
    for variable in ["GIT_AUTHOR_IDENT", "GIT_COMMITTER_IDENT"] {
        let captured = Run::new(repository, ["var", variable]).capture()?;
        if captured.code() != Some(0) {
            let error = crate::failed(&captured);
            if let GitError::Failed { message, .. } = &error {
                let message = message.to_ascii_lowercase();
                if message.contains("ident")
                    || message.contains("email")
                    || message.contains("no name was given")
                {
                    return Ok(false);
                }
            }
            return Err(error);
        }
    }
    Ok(true)
}

/// This worktree's git directory and the directory every worktree of the
/// repository shares (the same folder for a repository's main worktree).
pub fn git_dirs(repository: &Path) -> Result<(PathBuf, PathBuf), GitError> {
    let out = Run::new(
        repository,
        [
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ],
    )
    .output()?;
    let text = String::from_utf8(out).map_err(|e| GitError::UnreadableOutput(e.to_string()))?;
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    match (lines.next(), lines.next()) {
        (Some(git_dir), Some(common)) => Ok((PathBuf::from(git_dir), PathBuf::from(common))),
        _ => Err(GitError::UnreadableOutput(text)),
    }
}

/// The directories in the working tree that Git ignores, each once at its
/// top (`node_modules/`, not every folder inside it), relative to the
/// working tree's root.
pub fn ignored_directories(repository: &Path) -> Result<Vec<PathBuf>, GitError> {
    let out = Run::new(
        repository,
        [
            "ls-files",
            "-z",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "--no-empty-directory",
        ],
    )
    .output()?;
    Ok(out
        .split(|b| *b == 0)
        .filter_map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            entry
                .strip_suffix('/')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
        })
        .collect())
}

/// Which of `paths` Git ignores, in one `git check-ignore --stdin` for the
/// whole batch. A path Git tracks is never ignored, whatever the patterns
/// say.
pub fn ignored_paths(repository: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>, GitError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut input = Vec::new();
    for path in paths {
        input.extend_from_slice(path.as_os_str().as_encoded_bytes());
        input.push(0);
    }
    // `check-ignore` refuses `--literal-pathspecs`; it reads plain paths
    // from its input, never patterns, so nothing is lost.
    let captured = Run::new(repository, ["check-ignore", "-z", "--stdin"])
        .without_literal_pathspecs()
        .stdin(input)
        .capture()?;
    match captured.code() {
        // 1: none of them is ignored.
        Some(0) | Some(1) => Ok(captured
            .stdout
            .split(|b| *b == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| PathBuf::from(String::from_utf8_lossy(entry).into_owned()))
            .collect()),
        _ => Err(crate::failed(&captured)),
    }
}
