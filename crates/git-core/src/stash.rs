//! Stash entries, and the marker that ties one to the branch it was made on.

use crate::ParseError;

/// The `--format` that [`parse_stashes`] reads, with `-z`.
pub const STASH_FORMAT: &str = "--format=%gd%x1f%H%x1f%ct%x1f%gs";

/// The message Better Git gives a stash, so it knows which branch the stash
/// belongs to. GitHub Desktop uses `!!GitHub_Desktop<branch>`; this is the
/// same idea under its own name, and Desktop's own marker is read too so a
/// stash made there is found here.
pub const STASH_MARKER: &str = "!!Better_Git";
const DESKTOP_MARKER: &str = "!!GitHub_Desktop";

/// One stash entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stash {
    /// `stash@{0}`.
    pub name: String,
    pub sha: String,
    pub time: i64,
    /// The reflog subject: `On main: message` or `WIP on main: ...`.
    pub message: String,
}

impl Stash {
    /// The branch this stash was made for, when Better Git or GitHub Desktop
    /// made it.
    pub fn branch(&self) -> Option<&str> {
        let (_, rest) = self.message.split_once(": ")?;
        for marker in [STASH_MARKER, DESKTOP_MARKER] {
            if let Some(inner) = rest.strip_prefix(marker) {
                let inner = inner.strip_prefix('<')?;
                return inner.strip_suffix('>');
            }
        }
        None
    }
}

/// The message to stash with for `branch`.
pub fn stash_message(branch: &str) -> String {
    format!("{STASH_MARKER}<{branch}>")
}

/// Reads `git stash list -z` printed with [`STASH_FORMAT`].
///
/// Expected results: one entry per NUL-separated record with four fields;
/// empty input is no stashes; a short record fails with `Malformed`.
pub fn parse_stashes(output: &[u8]) -> Result<Vec<Stash>, ParseError> {
    let mut stashes = Vec::new();
    for record in output.split(|&b| b == 0) {
        let record = record.strip_prefix(b"\n").unwrap_or(record);
        if record.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(record);
        let fields: Vec<&str> = text.splitn(4, '\u{1f}').collect();
        let [name, sha, time, message] = fields[..] else {
            return Err(ParseError::Malformed(
                "stash record has too few fields".to_string(),
            ));
        };
        stashes.push(Stash {
            name: name.to_string(),
            sha: sha.to_string(),
            time: time.trim().parse().unwrap_or(0),
            message: message.trim_end().to_string(),
        });
    }
    Ok(stashes)
}

/// The stash Better Git made for `branch`, newest first, if any.
pub fn stash_for_branch<'a>(stashes: &'a [Stash], branch: &str) -> Option<&'a Stash> {
    stashes.iter().find(|stash| stash.branch() == Some(branch))
}
