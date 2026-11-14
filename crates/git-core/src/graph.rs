//! The branch graph's commits: `git log --topo-order -z --decorate=full` in
//! [`crate::log::LOG_FORMAT`], read a record at a time as git prints them,
//! the labels each commit carries, and finding a commit in the list.

use crate::ParseError;
use crate::log::{Commit, parse_log_record};

/// What a label on a commit names.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RefKind {
    /// `HEAD` on a commit with no branch checked out (detached).
    Head,
    /// A local branch.
    Local,
    /// A remote-tracking branch (`origin/main`).
    Remote,
    Tag,
}

/// One label on a commit row.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RefLabel {
    pub kind: RefKind,
    /// `main`, `origin/main`, `v1.0`, or `HEAD`.
    pub name: String,
    /// The branch HEAD is on.
    pub current: bool,
}

/// The labels in a commit's decorations, as `--decorate=full` prints them
/// (`HEAD -> refs/heads/main`, `refs/remotes/origin/main`,
/// `tag: refs/tags/v1`, `HEAD`).
///
/// Expected results:
/// - `HEAD -> refs/heads/x` is the current local branch `x`; a bare `HEAD`
///   is a detached HEAD label.
/// - `refs/heads/x` is local `x`, `refs/remotes/o/x` remote `o/x`, and
///   `tag: refs/tags/t` tag `t`.
/// - A remote's symbolic `HEAD` (`refs/remotes/o/HEAD`), the stash, notes,
///   and any other namespace carry no label.
/// - Labels come out current branch first, then the detached HEAD, local
///   branches, remote branches, and tags, each kind in git's order.
pub fn ref_labels(refs: &[String]) -> Vec<RefLabel> {
    let mut labels = Vec::new();
    for decoration in refs {
        let decoration = decoration.trim();
        if decoration == "HEAD" {
            labels.push(RefLabel {
                kind: RefKind::Head,
                name: "HEAD".to_string(),
                current: false,
            });
            continue;
        }
        let (current, full) = match decoration.strip_prefix("HEAD -> ") {
            Some(rest) => (true, rest),
            None => (false, decoration),
        };
        let label = if let Some(tag) = full.strip_prefix("tag: ") {
            tag.strip_prefix("refs/tags/")
                .map(|name| (RefKind::Tag, name))
        } else if let Some(name) = full.strip_prefix("refs/heads/") {
            Some((RefKind::Local, name))
        } else if let Some(name) = full.strip_prefix("refs/remotes/") {
            (!name.ends_with("/HEAD")).then_some((RefKind::Remote, name))
        } else {
            None
        };
        if let Some((kind, name)) = label
            && !name.is_empty()
        {
            labels.push(RefLabel {
                kind,
                name: name.to_string(),
                current: current && kind == RefKind::Local,
            });
        }
    }
    labels.sort_by_key(|l| (!l.current, l.kind));
    labels
}

/// Splits `git log -z` output into commits as the bytes arrive, so a long
/// history is read a page at a time from a pipe rather than all at once.
#[derive(Debug, Default)]
pub struct RecordReader {
    pending: Vec<u8>,
}

impl RecordReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the next bytes git printed and returns every commit they
    /// complete. A record cut off at the end of `bytes` waits for the next
    /// call.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Commit>, ParseError> {
        let mut commits = Vec::new();
        let mut rest = bytes;
        while let Some(end) = rest.iter().position(|&b| b == 0) {
            let record = if self.pending.is_empty() {
                parse_log_record(&rest[..end])?
            } else {
                self.pending.extend_from_slice(&rest[..end]);
                let record = parse_log_record(&self.pending)?;
                self.pending.clear();
                record
            };
            commits.extend(record);
            rest = &rest[end + 1..];
        }
        self.pending.extend_from_slice(rest);
        Ok(commits)
    }

    /// git has exited: what is left is the last record (git ends every
    /// record with NUL, so this is normally nothing).
    pub fn finish(&mut self) -> Result<Option<Commit>, ParseError> {
        let rest = std::mem::take(&mut self.pending);
        parse_log_record(&rest)
    }
}

/// What "find" matches in the graph.
///
/// Expected results:
/// - An empty or blank query matches nothing.
/// - A query of four or more hexadecimal digits matches a hash that starts
///   with it.
/// - Otherwise, and also, the query matches a subject or an author name
///   that contains it, ignoring case.
pub fn matches(commit: &Commit, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return false;
    }
    if query.len() >= 4
        && query.chars().all(|c| c.is_ascii_hexdigit())
        && commit
            .sha
            .get(..query.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(query))
    {
        return true;
    }
    let lower = query.to_lowercase();
    commit.summary.to_lowercase().contains(&lower)
        || commit.author_name.to_lowercase().contains(&lower)
}

/// The next commit after `from` (or from the top when `None`) that
/// [`matches`] the query, wrapping round to the top once.
pub fn find_next(commits: &[Commit], query: &str, from: Option<usize>) -> Option<usize> {
    let start = from.map_or(0, |i| i + 1);
    (start..commits.len())
        .chain(0..start.min(commits.len()))
        .find(|&i| matches(&commits[i], query))
}

/// The previous commit before `from` that [`matches`], wrapping round to
/// the bottom once.
pub fn find_previous(commits: &[Commit], query: &str, from: Option<usize>) -> Option<usize> {
    let len = commits.len();
    let start = from.unwrap_or(len).min(len);
    (0..start)
        .rev()
        .chain((start..len).rev())
        .find(|&i| matches(&commits[i], query))
}
