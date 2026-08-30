//! How well a name matches, and in what order the matches come back.
//!
//! The rule is the launcher's rule, for the same reason: a person typing
//! `rep` in a folder wants `report.pdf` before `xyz-preprocessor.log`, and no
//! amount of fuzzy cleverness makes the second one a better answer. So the
//! match kind dominates the score and the tie-breaks are all deterministic —
//! shorter name first, then alphabetical — because a search whose order
//! changes between two identical runs is a search you cannot use twice.

use files_core::natural_compare;
use std::cmp::Ordering;

/// How the query was found in the name, best first.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MatchKind {
    /// Every query character appears in order, but not adjacently.
    Fuzzy,
    /// The query appears somewhere inside the name.
    Substring,
    /// The name starts with the query.
    Prefix,
    /// The whole name is the query.
    Exact,
}

impl MatchKind {
    pub fn key(self) -> &'static str {
        match self {
            MatchKind::Exact => "exact",
            MatchKind::Prefix => "prefix",
            MatchKind::Substring => "substring",
            MatchKind::Fuzzy => "fuzzy",
        }
    }
}

/// One entry's match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Score {
    pub kind: MatchKind,
    /// Where the match starts, for highlighting. `0` for a fuzzy match, whose
    /// characters are scattered.
    pub offset: usize,
    /// The length of the name that matched, used only as a tie-break.
    pub name_len: usize,
}

impl Score {
    /// Better matches sort first. Not `Ord` on `Score` itself, because "better"
    /// is the reverse of the natural ordering of these fields and a silent
    /// reversal is exactly the kind of thing that gets a sort backwards.
    pub fn better_than(&self, other: &Score) -> Ordering {
        other
            .kind
            .cmp(&self.kind)
            .then_with(|| self.offset.cmp(&other.offset))
            .then_with(|| self.name_len.cmp(&other.name_len))
    }
}

/// Turns a name and a query into a score, or decides there is no match.
///
/// A trait so ranking can be replaced without touching the provider or the UI,
/// which is the separation Issue #6 asks for.
pub trait Ranker: Send + Sync {
    fn id(&self) -> &'static str;

    /// `query` arrives already trimmed and lowercased, once per search rather
    /// than once per entry.
    fn score(&self, name: &str, query: &str) -> Option<Score>;
}

/// The ranker that ships.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultRanker;

impl Ranker for DefaultRanker {
    fn id(&self) -> &'static str {
        "default"
    }

    fn score(&self, name: &str, query: &str) -> Option<Score> {
        let name_len = name.chars().count();
        if query.is_empty() {
            // An empty query matches everything equally; the filters and the
            // caller's own sort decide the order.
            return Some(Score {
                kind: MatchKind::Exact,
                offset: 0,
                name_len,
            });
        }
        let lowered = name.to_lowercase();
        if lowered == query {
            return Some(Score {
                kind: MatchKind::Exact,
                offset: 0,
                name_len,
            });
        }
        if lowered.starts_with(query) {
            return Some(Score {
                kind: MatchKind::Prefix,
                offset: 0,
                name_len,
            });
        }
        if let Some(byte_offset) = lowered.find(query) {
            return Some(Score {
                kind: MatchKind::Substring,
                // Characters, not bytes: an offset into a name with a
                // multi-byte character is used for highlighting, and a byte
                // index would highlight the wrong thing.
                offset: lowered[..byte_offset].chars().count(),
                name_len,
            });
        }
        subsequence(&lowered, query).then_some(Score {
            kind: MatchKind::Fuzzy,
            offset: 0,
            name_len,
        })
    }
}

/// Whether every character of `query` appears in `name`, in order.
fn subsequence(name: &str, query: &str) -> bool {
    let mut haystack = name.chars();
    query
        .chars()
        .all(|wanted| haystack.any(|candidate| candidate == wanted))
}

/// Orders two matches: better score first, then by name so identical scores
/// have one answer rather than whichever arrived first.
pub fn compare_hits(
    left_score: &Score,
    left_name: &str,
    right_score: &Score,
    right_name: &str,
) -> Ordering {
    left_score
        .better_than(right_score)
        .then_with(|| natural_compare(left_name, right_name))
}
