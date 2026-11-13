//! Worktrees: `git worktree list --porcelain -z`.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use crate::ParseError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    /// None for a bare entry.
    pub head: Option<String>,
    /// `main`, without `refs/heads/`; None when detached.
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub locked: bool,
    /// git would remove it on `worktree prune` (its folder is gone).
    pub prunable: bool,
    /// The first entry is the main working tree, which cannot be removed.
    pub main: bool,
}

/// Reads `git worktree list --porcelain -z`.
///
/// Expected results: attributes are NUL-separated, an empty attribute ends an
/// entry; `worktree <path>` starts one; `HEAD`, `branch`, `bare`, `detached`,
/// `locked [reason]`, and `prunable [reason]` set their fields; unknown
/// attributes are ignored; an attribute before any `worktree` fails with
/// `Malformed`.
pub fn parse_worktrees(output: &[u8]) -> Result<Vec<Worktree>, ParseError> {
    let mut worktrees: Vec<Worktree> = Vec::new();
    let mut current: Option<Worktree> = None;
    for field in output.split(|&b| b == 0) {
        if field.is_empty() {
            worktrees.extend(current.take());
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            worktrees.extend(current.take());
            current = Some(Worktree {
                path: PathBuf::from(OsStr::from_bytes(path)),
                head: None,
                branch: None,
                bare: false,
                detached: false,
                locked: false,
                prunable: false,
                main: worktrees.is_empty(),
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            return Err(ParseError::Malformed(
                "worktree attribute before `worktree`".to_string(),
            ));
        };
        let text = String::from_utf8_lossy(field);
        let (key, value) = text.split_once(' ').unwrap_or((&text, ""));
        match key {
            "HEAD" => entry.head = Some(value.to_string()),
            "branch" => {
                entry.branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_string(),
                )
            }
            "bare" => entry.bare = true,
            "detached" => entry.detached = true,
            "locked" => entry.locked = true,
            "prunable" => entry.prunable = true,
            _ => {}
        }
    }
    worktrees.extend(current.take());
    Ok(worktrees)
}
