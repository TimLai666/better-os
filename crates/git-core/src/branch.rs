//! Branches: `git for-each-ref` in an explicit format, and the rules for a
//! branch name.

use crate::ParseError;

/// The `--format` that [`parse_branches`] reads, one ref per line.
pub const BRANCH_FORMAT: &str = "--format=%(refname)%1f%(objectname)%1f%(upstream)%1f%(upstream:track,nobracket)%1f%(HEAD)%1f%(committerdate:unix)%1f%(subject)";

/// One local or remote-tracking branch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Branch {
    /// `main`, or `origin/main` for a remote-tracking branch.
    pub name: String,
    /// `refs/heads/main` or `refs/remotes/origin/main`.
    pub full_ref: String,
    pub remote: bool,
    pub sha: String,
    /// The upstream's short name (`origin/main`), for a local branch that
    /// has one.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// The upstream is configured but no longer exists.
    pub upstream_gone: bool,
    /// HEAD points at it.
    pub current: bool,
    /// When its tip was committed, seconds since the epoch.
    pub date: i64,
    pub subject: String,
}

impl Branch {
    /// For a remote-tracking branch, the remote's name (`origin`).
    pub fn remote_name(&self) -> Option<&str> {
        if !self.remote {
            return None;
        }
        self.name.split_once('/').map(|(remote, _)| remote)
    }

    /// For a remote-tracking branch, the branch name on the remote (`main`).
    pub fn name_on_remote(&self) -> &str {
        if self.remote {
            self.name
                .split_once('/')
                .map(|(_, name)| name)
                .unwrap_or(&self.name)
        } else {
            &self.name
        }
    }
}

/// Reads `git for-each-ref` printed with [`BRANCH_FORMAT`] for
/// `refs/heads` and `refs/remotes`.
///
/// Expected results:
/// - One branch per line, seven fields separated by 0x1f.
/// - `refs/heads/x` is local `x`; `refs/remotes/o/x` is remote `o/x`. A
///   remote's symbolic `HEAD` (`refs/remotes/o/HEAD`) is skipped.
/// - The track field is `ahead N`, `behind N`, `ahead N, behind M`, `gone`,
///   or empty.
/// - `*` in the HEAD field marks the current branch.
/// - A line with too few fields fails with `Malformed`.
pub fn parse_branches(output: &[u8]) -> Result<Vec<Branch>, ParseError> {
    let text = String::from_utf8_lossy(output);
    let mut branches = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.splitn(7, '\u{1f}').collect();
        let [full_ref, sha, upstream, track, head, date, subject] = fields[..] else {
            return Err(ParseError::Malformed(
                "branch line has too few fields".to_string(),
            ));
        };
        let (name, remote) = if let Some(name) = full_ref.strip_prefix("refs/heads/") {
            (name, false)
        } else if let Some(name) = full_ref.strip_prefix("refs/remotes/") {
            if name.ends_with("/HEAD") {
                continue;
            }
            (name, true)
        } else {
            continue;
        };
        let (mut ahead, mut behind, mut gone) = (0, 0, false);
        for part in track.split(',').map(str::trim) {
            if let Some(n) = part.strip_prefix("ahead ") {
                ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                behind = n.parse().unwrap_or(0);
            } else if part == "gone" {
                gone = true;
            }
        }
        branches.push(Branch {
            name: name.to_string(),
            full_ref: full_ref.to_string(),
            remote,
            sha: sha.to_string(),
            upstream: upstream
                .strip_prefix("refs/remotes/")
                .map(str::to_string)
                .or_else(|| (!upstream.is_empty()).then(|| upstream.to_string())),
            ahead,
            behind,
            upstream_gone: gone,
            current: head == "*",
            date: date.parse().unwrap_or(0),
            subject: subject.to_string(),
        });
    }
    Ok(branches)
}

/// Turns what a person typed into a name git accepts, the way GitHub
/// Desktop does: spaces and runs of characters git refuses become `-`,
/// and leading `-` or `.` and trailing `.`, `/`, or `.lock` are removed.
///
/// Expected results:
/// - `my feature` becomes `my-feature`.
/// - `~^:?*[\` and control characters become `-`; `..` and `@{` too.
/// - An empty or all-invalid input becomes an empty string.
pub fn sanitize_ref_name(typed: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    let chars: Vec<char> = typed.trim().chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let bad = c.is_whitespace()
            || c.is_control()
            || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
            || (c == '.' && next == Some('.'))
            || (c == '@' && next == Some('{'));
        if bad {
            if !last_dash {
                out.push('-');
                last_dash = true;
            }
            if (c == '.' && next == Some('.')) || (c == '@' && next == Some('{')) {
                i += 1;
            }
        } else {
            out.push(c);
            last_dash = false;
        }
        i += 1;
    }
    let mut out = out.trim_start_matches(['-', '.']).to_string();
    loop {
        let before = out.len();
        out = out.trim_end_matches(['.', '/']).to_string();
        if let Some(stripped) = out.strip_suffix(".lock") {
            out = stripped.to_string();
        }
        if out.len() == before {
            break;
        }
    }
    out.replace("//", "/")
}

/// Why a branch name cannot be used, if it cannot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameProblem {
    Empty,
    /// Another branch already has it.
    Exists,
    /// It is `HEAD`, which git reserves.
    Reserved,
}

/// Checks a name that has already been through [`sanitize_ref_name`]
/// against the branches that exist.
pub fn check_branch_name(name: &str, existing: &[Branch]) -> Option<NameProblem> {
    if name.is_empty() {
        return Some(NameProblem::Empty);
    }
    if name == "HEAD" {
        return Some(NameProblem::Reserved);
    }
    if existing
        .iter()
        .any(|branch| !branch.remote && branch.name == name)
    {
        return Some(NameProblem::Exists);
    }
    None
}

/// The branch the repository treats as its default: the remote's `HEAD`
/// when known, else `main`, else `master`, else the current branch.
pub fn default_branch<'a>(branches: &'a [Branch], remote_head: Option<&str>) -> Option<&'a Branch> {
    let local = |name: &str| branches.iter().find(|b| !b.remote && b.name == name);
    if let Some(head) = remote_head {
        let short = head.split_once('/').map(|(_, n)| n).unwrap_or(head);
        if let Some(branch) = local(short) {
            return Some(branch);
        }
        if let Some(branch) = branches.iter().find(|b| b.remote && b.name == head) {
            return Some(branch);
        }
    }
    local("main")
        .or_else(|| local("master"))
        .or_else(|| branches.iter().find(|b| b.current))
}
