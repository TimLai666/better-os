//! `git status --porcelain=v2 -z --branch`, read into a changes list.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use crate::ParseError;

/// What happened to one path on one side (the index, or the working tree).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    /// The file's type changed, for example a file became a symbolic link.
    TypeChanged,
}

/// One changed path.
///
/// A path can be changed on both sides at once: staged, and then edited again.
/// It is one entry here with both sides set, the way git reports it, and the
/// window shows it in both the staged and the unstaged list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileChange {
    /// The path relative to the repository root, as bytes git printed. Not
    /// required to be UTF-8.
    pub path: PathBuf,
    /// For a rename or a copy, the path it came from.
    pub original_path: Option<PathBuf>,
    /// The change recorded in the index, if any.
    pub staged: Option<ChangeKind>,
    /// The change in the working tree that is not in the index, if any.
    pub unstaged: Option<ChangeKind>,
    /// Not tracked by git at all. `staged` and `unstaged` are both `None`.
    pub untracked: bool,
    /// An unmerged path. `staged` and `unstaged` are both `None`.
    pub conflicted: bool,
    /// For an unmerged path, which sides changed it.
    pub conflict: Option<ConflictKind>,
}

/// The `XY` of an unmerged (`u`) record: what each side did to the path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictKind {
    /// `UU`: both sides changed the file.
    BothModified,
    /// `AA`: both sides added a file at this path.
    BothAdded,
    /// `DD`: both sides deleted it.
    BothDeleted,
    /// `AU`: added by us.
    AddedByUs,
    /// `UA`: added by them.
    AddedByThem,
    /// `DU`: deleted by us, changed by them.
    DeletedByUs,
    /// `UD`: changed by us, deleted by them.
    DeletedByThem,
}

impl ConflictKind {
    /// Reads the two letters of a `u` record.
    pub fn from_xy(xy: &[u8]) -> Option<ConflictKind> {
        Some(match xy {
            b"UU" => ConflictKind::BothModified,
            b"AA" => ConflictKind::BothAdded,
            b"DD" => ConflictKind::BothDeleted,
            b"AU" => ConflictKind::AddedByUs,
            b"UA" => ConflictKind::AddedByThem,
            b"DU" => ConflictKind::DeletedByUs,
            b"UD" => ConflictKind::DeletedByThem,
            _ => return None,
        })
    }

    /// True when the working-tree file holds git's conflict markers, so the
    /// person edits it; false when one side deleted it and the choice is
    /// keep or delete.
    pub fn has_markers(self) -> bool {
        matches!(self, ConflictKind::BothModified | ConflictKind::BothAdded)
    }
}

/// Where HEAD is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BranchHead {
    /// On a branch. The name is what `# branch.head` printed.
    Branch(String),
    /// Detached. git prints `(detached)` and this carries no name.
    Detached,
}

/// Everything one status call reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryStatus {
    pub head: BranchHead,
    /// True when the repository has no commit yet (`# branch.oid (initial)`).
    pub unborn: bool,
    /// The commit HEAD points at (`# branch.oid`); `None` before the first
    /// commit.
    pub head_oid: Option<String>,
    /// The upstream branch name, when one is configured.
    pub upstream: Option<String>,
    /// Commits ahead of and behind the upstream, when git reported them.
    pub ahead_behind: Option<(u32, u32)>,
    /// Changed paths in the order git printed them.
    pub changes: Vec<FileChange>,
    /// How many stash entries exist (`# stash N`, from `--show-stash`).
    pub stash_count: u32,
}

/// Reads the NUL-separated output of
/// `git status --porcelain=v2 -z --branch --untracked-files=all`.
///
/// Expected results:
/// - `# branch.head <name>` sets `head`; `(detached)` is `BranchHead::Detached`.
/// - `# branch.oid (initial)` sets `unborn`.
/// - `# branch.upstream <name>` and `# branch.ab +A -B` set `upstream` and
///   `ahead_behind`. Both are optional.
/// - A `1` record sets `staged` from X and `unstaged` from Y of its `XY`
///   field; `.` means no change on that side. `M`, `A`, `D`, `T` map to
///   Modified, Added, Deleted, TypeChanged.
/// - A `2` record is a rename (`R`) or a copy (`C`); the path is in the
///   record and the original path is the next NUL-separated field.
/// - A `u` record is conflicted. A `?` record is untracked. `!` records
///   (ignored) are skipped.
/// - A path is everything after the fixed fields of its record, taken as raw
///   bytes: spaces, non-ASCII bytes, and a leading `-` are part of the path.
/// - Empty input is a repository with nothing to report: it fails with
///   `Malformed`, because `--branch` always prints `# branch.head`.
/// - A record type this function does not know, or a record with too few
///   fields, fails with `Malformed` naming the record type.
pub fn parse_status(output: &[u8]) -> Result<RepositoryStatus, ParseError> {
    let mut fields: Vec<&[u8]> = output.split(|&byte| byte == 0).collect();
    if fields.last().is_some_and(|last| last.is_empty()) {
        fields.pop();
    }
    let mut fields = fields.into_iter();

    let mut status = RepositoryStatus {
        head: BranchHead::Detached,
        unborn: false,
        head_oid: None,
        upstream: None,
        ahead_behind: None,
        changes: Vec::new(),
        stash_count: 0,
    };
    let mut saw_head = false;

    while let Some(record) = fields.next() {
        match record.first().copied() {
            None => return Err(malformed("empty record")),
            Some(b'#') => saw_head |= read_header(record, &mut status)?,
            Some(b'1') => {
                let parts: Vec<&[u8]> = record.splitn(9, |&byte| byte == b' ').collect();
                let (Some(xy), Some(path)) = (parts.get(1), parts.get(8)) else {
                    return Err(malformed("`1` record has too few fields"));
                };
                let (staged, unstaged) = sides(xy)?;
                status.changes.push(FileChange {
                    path: path_of(path),
                    original_path: None,
                    staged,
                    unstaged,
                    untracked: false,
                    conflicted: false,
                    conflict: None,
                });
            }
            Some(b'2') => {
                let parts: Vec<&[u8]> = record.splitn(10, |&byte| byte == b' ').collect();
                let (Some(xy), Some(path)) = (parts.get(1), parts.get(9)) else {
                    return Err(malformed("`2` record has too few fields"));
                };
                let Some(original) = fields.next() else {
                    return Err(malformed("`2` record has no original path field"));
                };
                let (staged, unstaged) = sides(xy)?;
                status.changes.push(FileChange {
                    path: path_of(path),
                    original_path: Some(path_of(original)),
                    staged,
                    unstaged,
                    untracked: false,
                    conflicted: false,
                    conflict: None,
                });
            }
            Some(b'u') => {
                let parts: Vec<&[u8]> = record.splitn(11, |&byte| byte == b' ').collect();
                let (Some(xy), Some(path)) = (parts.get(1), parts.get(10)) else {
                    return Err(malformed("`u` record has too few fields"));
                };
                let kind = ConflictKind::from_xy(xy)
                    .ok_or_else(|| malformed("`u` record has an unknown `XY`"))?;
                status.changes.push(FileChange {
                    path: path_of(path),
                    original_path: None,
                    staged: None,
                    unstaged: None,
                    untracked: false,
                    conflicted: true,
                    conflict: Some(kind),
                });
            }
            Some(b'?') => {
                let Some(path) = record.strip_prefix(b"? ") else {
                    return Err(malformed("`?` record has no path"));
                };
                status.changes.push(FileChange {
                    path: path_of(path),
                    original_path: None,
                    staged: None,
                    unstaged: None,
                    untracked: true,
                    conflicted: false,
                    conflict: None,
                });
            }
            Some(b'!') => {}
            Some(other) => {
                return Err(malformed(format!(
                    "unknown record type `{}`",
                    char::from(other)
                )));
            }
        }
    }

    if !saw_head {
        return Err(malformed("`# branch.head` header is missing"));
    }
    Ok(status)
}

fn malformed(detail: impl Into<String>) -> ParseError {
    ParseError::Malformed(detail.into())
}

fn path_of(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(bytes))
}

/// Reads one `# ` header. Returns true when it was `branch.head`.
fn read_header(record: &[u8], status: &mut RepositoryStatus) -> Result<bool, ParseError> {
    let text = String::from_utf8_lossy(record);
    let body = text
        .strip_prefix("# ")
        .ok_or_else(|| malformed("header record does not start with `# `"))?;
    let (key, value) = body.split_once(' ').unwrap_or((body, ""));
    match key {
        "branch.oid" => {
            status.unborn = value == "(initial)";
            status.head_oid = (!status.unborn).then(|| value.to_string());
        }
        "branch.head" => {
            status.head = if value == "(detached)" {
                BranchHead::Detached
            } else {
                BranchHead::Branch(value.to_string())
            };
            return Ok(true);
        }
        "branch.upstream" => status.upstream = Some(value.to_string()),
        "branch.ab" => status.ahead_behind = Some(parse_ahead_behind(value)?),
        "stash" => status.stash_count = value.parse().unwrap_or(0),
        _ => {}
    }
    Ok(false)
}

/// Reads `+A -B`.
fn parse_ahead_behind(value: &str) -> Result<(u32, u32), ParseError> {
    let mut parts = value.split(' ');
    let ahead: Option<u32> = parts.next().and_then(|p| p.strip_prefix('+')?.parse().ok());
    let behind: Option<u32> = parts.next().and_then(|p| p.strip_prefix('-')?.parse().ok());
    match (ahead, behind, parts.next()) {
        (Some(ahead), Some(behind), None) => Ok((ahead, behind)),
        _ => Err(malformed("`branch.ab` header is not `+A -B`")),
    }
}

/// Reads the two status letters of a `1` or `2` record: X is the index, Y the
/// working tree.
fn sides(xy: &[u8]) -> Result<(Option<ChangeKind>, Option<ChangeKind>), ParseError> {
    match xy {
        [x, y] => Ok((change_kind(*x)?, change_kind(*y)?)),
        _ => Err(malformed("change status field is not two characters")),
    }
}

fn change_kind(code: u8) -> Result<Option<ChangeKind>, ParseError> {
    Ok(match code {
        b'.' => None,
        b'M' => Some(ChangeKind::Modified),
        b'A' => Some(ChangeKind::Added),
        b'D' => Some(ChangeKind::Deleted),
        b'T' => Some(ChangeKind::TypeChanged),
        b'R' => Some(ChangeKind::Renamed),
        b'C' => Some(ChangeKind::Copied),
        other => {
            return Err(malformed(format!(
                "unknown change code `{}`",
                char::from(other)
            )));
        }
    })
}
