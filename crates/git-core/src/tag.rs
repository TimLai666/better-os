//! Tags: `git for-each-ref refs/tags`, and the rules for a tag name.

use crate::ParseError;

/// The `--format` that [`parse_tags`] reads. `%(*objectname)` is the commit
/// an annotated tag points at; it is empty for a lightweight tag, whose own
/// object is the commit.
pub const TAG_FORMAT: &str = "--format=%(refname:strip=2)%1f%(objectname)%1f%(*objectname)";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tag {
    pub name: String,
    /// The commit the tag points at.
    pub commit: String,
}

/// Reads `git for-each-ref refs/tags` printed with [`TAG_FORMAT`].
pub fn parse_tags(output: &[u8]) -> Result<Vec<Tag>, ParseError> {
    let text = String::from_utf8_lossy(output);
    let mut tags = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\u{1f}').collect();
        let [name, object, peeled] = fields[..] else {
            return Err(ParseError::Malformed(
                "tag line has the wrong number of fields".to_string(),
            ));
        };
        tags.push(Tag {
            name: name.to_string(),
            commit: if peeled.is_empty() { object } else { peeled }.to_string(),
        });
    }
    Ok(tags)
}

/// Why a tag name cannot be used, if it cannot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TagProblem {
    Empty,
    Exists,
    /// Longer than GitHub Desktop allows (245 bytes).
    TooLong,
}

pub const MAX_TAG_LENGTH: usize = 245;

/// Checks a sanitized tag name against the tags that exist.
pub fn check_tag_name(name: &str, existing: &[Tag]) -> Option<TagProblem> {
    if name.is_empty() {
        Some(TagProblem::Empty)
    } else if name.len() > MAX_TAG_LENGTH {
        Some(TagProblem::TooLong)
    } else if existing.iter().any(|tag| tag.name == name) {
        Some(TagProblem::Exists)
    } else {
        None
    }
}
