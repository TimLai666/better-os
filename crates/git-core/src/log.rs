//! A branch's history: `git log` in an explicit format, and the files one
//! commit changed.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use crate::ParseError;
use crate::status::ChangeKind;

/// The `--format` that [`parse_log`] reads, with `-z` between commits. Fields
/// are separated by the unit separator (0x1f), which git never prints in a
/// hash, a name, a date, or a decoration, and which a message would have to
/// contain on purpose. The body is the last field, so a separator inside it
/// stays part of it.
pub const LOG_FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s%x1f%b";

/// One commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub parents: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    /// Seconds since the epoch.
    pub author_time: i64,
    /// The decorations git printed for it (`HEAD -> main`, `tag: v1`, ...).
    pub refs: Vec<String>,
    pub summary: String,
    pub body: String,
}

impl Commit {
    /// The first seven characters of the hash.
    pub fn short_sha(&self) -> &str {
        &self.sha[..self.sha.len().min(7)]
    }

    /// The tags among the decorations.
    pub fn tags(&self) -> Vec<&str> {
        self.refs
            .iter()
            .filter_map(|decoration| decoration.strip_prefix("tag: "))
            .collect()
    }

    /// A merge commit has more than one parent.
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// `Co-authored-by:` trailers in the body, as `(name, email)`.
    pub fn co_authors(&self) -> Vec<(String, String)> {
        self.body.lines().filter_map(parse_co_author).collect()
    }
}

/// Reads one `Co-authored-by: Name <email>` trailer line.
pub fn parse_co_author(line: &str) -> Option<(String, String)> {
    let rest = line.trim();
    let (key, value) = rest.split_once(':')?;
    if !key.trim().eq_ignore_ascii_case("co-authored-by") {
        return None;
    }
    let value = value.trim();
    let open = value.rfind('<')?;
    let close = value.rfind('>')?;
    if close < open {
        return None;
    }
    let name = value[..open].trim();
    let email = value[open + 1..close].trim();
    if name.is_empty() || email.is_empty() {
        return None;
    }
    Some((name.to_string(), email.to_string()))
}

/// Reads `git log -z` printed with [`LOG_FORMAT`].
///
/// Expected results:
/// - Commits are separated by NUL; an empty record (the trailing one) is
///   skipped. Empty input is no commits.
/// - Each record has eight fields separated by 0x1f; the body may be empty
///   and keeps any 0x1f it contains. Its trailing newline is trimmed.
/// - Parents and decorations may be empty. A root commit has no parents.
/// - A record with fewer fields, or a time that is not a number, fails with
///   `Malformed`.
pub fn parse_log(output: &[u8]) -> Result<Vec<Commit>, ParseError> {
    let mut commits = Vec::new();
    for record in output.split(|&byte| byte == 0) {
        // `git log -z` separates commits with NUL; the format itself ends
        // with a newline after the body that belongs to no field.
        let record = record.strip_prefix(b"\n").unwrap_or(record);
        if record.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(record);
        let fields: Vec<&str> = text.splitn(8, '\u{1f}').collect();
        let [sha, parents, name, email, time, refs, summary, body] = fields[..] else {
            return Err(ParseError::Malformed(
                "log record has too few fields".to_string(),
            ));
        };
        let author_time = time
            .parse()
            .map_err(|_| ParseError::Malformed("log record time is not a number".to_string()))?;
        commits.push(Commit {
            sha: sha.to_string(),
            parents: parents.split_whitespace().map(str::to_string).collect(),
            author_name: name.to_string(),
            author_email: email.to_string(),
            author_time,
            refs: refs
                .split(", ")
                .map(str::trim)
                .filter(|decoration| !decoration.is_empty())
                .map(str::to_string)
                .collect(),
            summary: summary.to_string(),
            body: body.trim_end().to_string(),
        });
    }
    Ok(commits)
}

/// One file a commit changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitFile {
    pub path: PathBuf,
    /// For a rename or copy, where it came from.
    pub original_path: Option<PathBuf>,
    pub kind: ChangeKind,
}

/// Reads `git diff-tree -r -z --no-commit-id --name-status -M --root <sha>`
/// (or `git diff --name-status -z -M` between two commits).
///
/// Expected results:
/// - Each entry is a status field and a path; `R` and `C` (with a score,
///   `R100`) are followed by the original path and then the new one.
/// - `M`, `A`, `D`, `T` map to their `ChangeKind`; `U` and anything else
///   fails with `Malformed`.
/// - Empty input is no files.
pub fn parse_name_status(output: &[u8]) -> Result<Vec<CommitFile>, ParseError> {
    let mut fields = output.split(|&byte| byte == 0).filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let status = status.strip_prefix(b"\n").unwrap_or(status);
        let path = |field: Option<&[u8]>| -> Result<PathBuf, ParseError> {
            field
                .map(|bytes| PathBuf::from(OsStr::from_bytes(bytes)))
                .ok_or_else(|| ParseError::Malformed("name-status entry has no path".to_string()))
        };
        let kind = match status.first() {
            Some(b'M') => ChangeKind::Modified,
            Some(b'A') => ChangeKind::Added,
            Some(b'D') => ChangeKind::Deleted,
            Some(b'T') => ChangeKind::TypeChanged,
            Some(b'R') => ChangeKind::Renamed,
            Some(b'C') => ChangeKind::Copied,
            _ => {
                return Err(ParseError::Malformed(
                    "name-status entry has an unknown status".to_string(),
                ));
            }
        };
        if matches!(kind, ChangeKind::Renamed | ChangeKind::Copied) {
            let original = path(fields.next())?;
            let new = path(fields.next())?;
            files.push(CommitFile {
                path: new,
                original_path: Some(original),
                kind,
            });
        } else {
            files.push(CommitFile {
                path: path(fields.next())?,
                original_path: None,
                kind,
            });
        }
    }
    Ok(files)
}
