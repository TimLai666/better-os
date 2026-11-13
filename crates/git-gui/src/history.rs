//! The history list's rules: which commits may be rewritten, and what
//! dragging one commit onto another does (reorder, squash), turned into the
//! todo list `git rebase -i` runs.

use git_core::Commit;
use git_platform::{HistoryEdit, TodoAction};

/// Whether the commits from the newest down to `index` can be rewritten:
/// none of them may be a merge, and when the branch has an upstream they
/// must not be on it yet (GitHub Desktop refuses to rewrite pushed
/// commits). `ahead` is how many commits the branch has that its upstream
/// does not; `None` when it has no upstream.
pub fn can_rewrite(commits: &[Commit], index: usize, ahead: Option<u32>) -> bool {
    if index >= commits.len() {
        return false;
    }
    if commits[..=index].iter().any(Commit::is_merge) {
        return false;
    }
    match ahead {
        Some(ahead) => index < ahead as usize,
        None => true,
    }
}

/// The base (the parent of the oldest touched commit) and the commits from
/// it to the newest, oldest first. The list's first commit is where HEAD
/// was when it was read; the rewrite carries it so git refuses a list that
/// has gone stale.
fn range(commits: &[Commit], oldest: usize) -> (Option<String>, Vec<String>) {
    let base = commits[oldest].parents.first().cloned();
    let todo = commits[..=oldest]
        .iter()
        .rev()
        .map(|c| c.sha.clone())
        .collect();
    (base, todo)
}

/// Moving the commit at `from` so it sits just before (newer than) the
/// commit at `to` in the list. Newest is index 0.
pub fn reorder(commits: &[Commit], from: usize, to: usize) -> Option<HistoryEdit> {
    if from == to || from >= commits.len() || to >= commits.len() {
        return None;
    }
    let oldest = from.max(to);
    let (base, mut order) = range(commits, oldest);
    // `order` is oldest first; convert indexes.
    let n = order.len();
    let from_pos = n - 1 - from;
    let sha = order.remove(from_pos);
    let to_sha = &commits[to].sha;
    let target = order.iter().position(|s| s == to_sha)?;
    // Newer than `to` means after it in oldest-first order.
    order.insert(target + 1, sha);
    Some(HistoryEdit::Rewrite {
        base,
        todo: order.into_iter().map(|s| (TodoAction::Pick, s)).collect(),
        message: None,
        head: commits[0].sha.clone(),
    })
}

/// Squashing the commit at `from` into the commit at `to`: the result sits
/// where `to` is, with `message`.
pub fn squash(commits: &[Commit], from: usize, to: usize, message: &str) -> Option<HistoryEdit> {
    if from == to || from >= commits.len() || to >= commits.len() {
        return None;
    }
    let oldest = from.max(to);
    let (base, order) = range(commits, oldest);
    let from_sha = commits[from].sha.clone();
    let to_sha = commits[to].sha.clone();
    let mut todo = Vec::new();
    for sha in order {
        if sha == from_sha {
            continue;
        }
        let is_target = sha == to_sha;
        todo.push((TodoAction::Pick, sha));
        if is_target {
            todo.push((TodoAction::Squash, from_sha.clone()));
        }
    }
    Some(HistoryEdit::Rewrite {
        base,
        todo,
        message: Some(message.to_string()),
        head: commits[0].sha.clone(),
    })
}

/// The message a squash starts with: both summaries and bodies, the target
/// first, as GitHub Desktop pre-fills it.
pub fn squash_message(target: &Commit, folded: &Commit) -> String {
    let mut text = target.summary.clone();
    for part in [&target.body, &folded.summary, &folded.body] {
        if !part.trim().is_empty() {
            text.push_str("\n\n");
            text.push_str(part.trim());
        }
    }
    text
}

/// Relative time for a commit row, in whole units.
pub fn age(now: i64, then: i64) -> (u64, &'static str) {
    let seconds = (now - then).max(0) as u64;
    match seconds {
        s if s < 60 => (s, "s"),
        s if s < 3600 => (s / 60, "m"),
        s if s < 86_400 => (s / 3600, "h"),
        s if s < 86_400 * 30 => (s / 86_400, "d"),
        s if s < 86_400 * 365 => (s / (86_400 * 30), "mo"),
        s => (s / (86_400 * 365), "y"),
    }
}
