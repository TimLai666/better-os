//! The conflicts list shown while a merge, rebase, cherry-pick, revert, or
//! stash restore is stopped.

use std::path::PathBuf;

use git_core::{ConflictKind, RepositoryStatus};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub path: PathBuf,
    pub kind: ConflictKind,
    /// Conflict regions still in the file; `None` when it was not read (a
    /// side deleted it).
    pub markers: Option<usize>,
}

/// The conflicted files and, for each that has them, how many marker
/// regions remain, as counted with the last snapshot (`markers`).
pub fn rows(status: &RepositoryStatus, markers: &[(PathBuf, Option<usize>)]) -> Vec<Row> {
    status
        .changes
        .iter()
        .filter_map(|c| {
            let kind = c.conflict?;
            let markers = if kind.has_markers() {
                markers
                    .iter()
                    .find(|(path, _)| *path == c.path)
                    .and_then(|(_, count)| *count)
            } else {
                None
            };
            Some(Row {
                path: c.path.clone(),
                kind,
                markers,
            })
        })
        .collect()
}

/// Continue is offered once nothing is conflicted any more.
pub fn can_continue(status: &RepositoryStatus) -> bool {
    !status.changes.iter().any(|c| c.conflicted)
}

/// Whether "Mark as resolved" needs a confirmation: markers are still in
/// the file.
pub fn needs_confirmation(row: &Row) -> bool {
    row.markers.is_some_and(|m| m > 0)
}
