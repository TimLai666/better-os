//! The branch list: filtered, grouped as GitHub Desktop groups it (the
//! default branch, recent branches, then the rest), and remote-only
//! branches shown once beside the local ones.

use git_core::Branch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Group {
    Default,
    Recent,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub name: String,
    pub group: Group,
    pub current: bool,
    /// Only on the remote; switching makes a local branch that tracks it.
    pub remote_only: bool,
    pub date: i64,
}

/// How many branches the recent group holds.
pub const RECENT: usize = 5;

/// The rows for a search. `recent` is the branches switched to lately,
/// newest first.
pub fn rows(
    branches: &[Branch],
    default: Option<&str>,
    recent: &[String],
    query: &str,
) -> Vec<Row> {
    let query = query.trim().to_lowercase();
    let local: Vec<&Branch> = branches.iter().filter(|b| !b.remote).collect();
    let mut rows: Vec<Row> = Vec::new();
    for branch in &local {
        rows.push(Row {
            name: branch.name.clone(),
            group: Group::Other,
            current: branch.current,
            remote_only: false,
            date: branch.date,
        });
    }
    for branch in branches.iter().filter(|b| b.remote) {
        let short = branch.name_on_remote();
        if local.iter().any(|l| l.name == short) || rows.iter().any(|r| r.name == branch.name) {
            continue;
        }
        if branches
            .iter()
            .filter(|b| b.remote && b.name_on_remote() == short)
            .position(|b| b.name == branch.name)
            != Some(0)
        {
            continue;
        }
        rows.push(Row {
            name: branch.name.clone(),
            group: Group::Other,
            current: false,
            remote_only: true,
            date: branch.date,
        });
    }
    for row in &mut rows {
        if Some(row.name.as_str()) == default {
            row.group = Group::Default;
        } else if recent.iter().take(RECENT).any(|r| *r == row.name) {
            row.group = Group::Recent;
        }
    }
    rows.retain(|r| query.is_empty() || r.name.to_lowercase().contains(&query));
    rows.sort_by(|a, b| {
        let order = |g: Group| match g {
            Group::Default => 0,
            Group::Recent => 1,
            Group::Other => 2,
        };
        order(a.group)
            .cmp(&order(b.group))
            .then_with(|| match a.group {
                Group::Recent => {
                    let pos = |n: &str| recent.iter().position(|r| r == n);
                    pos(&a.name).cmp(&pos(&b.name))
                }
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            })
    });
    rows
}

/// The recent list after switching to `branch`.
pub fn remember(recent: &[String], branch: &str) -> Vec<String> {
    let mut next: Vec<String> = recent.iter().filter(|b| *b != branch).cloned().collect();
    next.insert(0, branch.to_string());
    next.truncate(10);
    next
}
