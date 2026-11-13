//! The clone dialog's rules: a URL or `owner/name`, or a repository from
//! the account's list, and the folder it goes into.

use std::path::{Path, PathBuf};

use github_api::RepoInfo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloneProblem {
    NoUrl,
    NoFolder,
    /// The folder exists and is not empty.
    FolderInUse,
}

/// The URL git clones and the folder it makes, from what the dialog holds.
/// `parent` is the chosen parent folder; the repository's name is added.
pub fn plan(
    typed_url: &str,
    parent: &Path,
    exists_non_empty: impl Fn(&Path) -> bool,
) -> Result<(String, PathBuf), CloneProblem> {
    let url = git_core::remote::clone_url(typed_url).ok_or(CloneProblem::NoUrl)?;
    let name = git_core::remote::clone_folder_name(&url).ok_or(CloneProblem::NoUrl)?;
    if !parent.is_absolute() {
        return Err(CloneProblem::NoFolder);
    }
    let destination = parent.join(name);
    if exists_non_empty(&destination) {
        return Err(CloneProblem::FolderInUse);
    }
    Ok((url, destination))
}

/// The account's repositories filtered by a search on the full name,
/// grouped by owner as GitHub Desktop lists them.
pub fn filter<'a>(repos: &'a [RepoInfo], query: &str) -> Vec<&'a RepoInfo> {
    let query = query.trim().to_lowercase();
    let mut out: Vec<&RepoInfo> = repos
        .iter()
        .filter(|r| query.is_empty() || r.full_name.to_lowercase().contains(&query))
        .collect();
    out.sort_by(|a, b| {
        a.owner
            .login
            .to_lowercase()
            .cmp(&b.owner.login.to_lowercase())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// Whether a folder exists and holds something.
pub fn non_empty(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
}
