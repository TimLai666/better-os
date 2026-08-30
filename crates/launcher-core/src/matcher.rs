//! How one query is compared against one field, and what the comparison is
//! worth.
//!
//! The ranking rules Issue #2 requires are encoded as two ordered enums and
//! one integer, in that precedence:
//!
//! 1. [`MatchKind`] — how the query met the text. An exact match outranks a
//!    prefix, which outranks a match at the start of a later word, which
//!    outranks a match buried inside a word, which outranks a fuzzy
//!    subsequence.
//! 2. [`FieldKind`] — where it matched. The application's name outranks its
//!    translations, which outrank the generic name, which outranks keywords
//!    and the executable name.
//! 3. A detail score, which orders matches that agree on both of the above.
//!
//! The precedence between the first two is a decision, not an accident: an
//! exact keyword match beats a fuzzy name match. "Application name ranks above
//! secondary metadata" settles ties between equally strong matches, and does
//! not promote a weak name match over a strong keyword one — otherwise typing
//! an application's exact keyword would rank it below every application whose
//! name happens to contain those letters in order.
//!
//! There is no scoring dependency here on anything outside the two strings, so
//! the same query against the same field is worth the same number forever.

use crate::text::FoldedText;

/// How a query met a field, worst to best. The ordering is the ranking rule.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MatchKind {
    /// Every query character appears in order, with other characters between
    /// them.
    Fuzzy,
    /// The query appears as a run of characters inside a word.
    Substring,
    /// The query appears as a run of characters starting at a word boundary.
    WordPrefix,
    /// The field begins with the query.
    Prefix,
    /// The field is the query.
    Exact,
}

/// Which field matched, worst to best. The ordering is the "name above
/// secondary metadata" rule.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FieldKind {
    /// The program name from `Exec`, `TryExec`, or the resolved executable.
    Executable,
    /// One of the entry's `Keywords`.
    Keyword,
    /// `GenericName`, in any locale the entry carries.
    GenericName,
    /// `Name` in a locale other than the active one, including the
    /// untranslated value when a translation is active.
    AlternateName,
    /// `Name` as the active locale resolves it.
    Name,
}

/// The largest detail score, and therefore the width of one field bucket.
pub const MAX_DETAIL: u32 = 9_999;

const FIELD_STRIDE: u32 = MAX_DETAIL + 1;
const KIND_STRIDE: u32 = FIELD_STRIDE * 100;

/// One field's answer for one query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldMatch {
    pub kind: MatchKind,
    pub field: FieldKind,
    pub detail: u32,
}

impl FieldMatch {
    /// The single number results are sorted by. Composed so the match kind
    /// dominates the field, and the field dominates the detail score: no
    /// detail score can promote a fuzzy match past a substring one, and no
    /// keyword match can be worth more than a name match of the same kind.
    pub fn score(&self) -> u32 {
        self.kind as u32 * KIND_STRIDE
            + self.field as u32 * FIELD_STRIDE
            + self.detail.min(MAX_DETAIL)
    }

    /// The same score with a bounded adjustment added to the detail term. The
    /// clamp is what keeps a usage signal from ever moving a result out of the
    /// bucket its match earned.
    pub fn adjusted_score(&self, detail_bonus: u32) -> u32 {
        Self {
            detail: self.detail.saturating_add(detail_bonus).min(MAX_DETAIL),
            ..*self
        }
        .score()
    }

    /// Whether this match should replace `other` as a record's best.
    fn beats(&self, other: &FieldMatch) -> bool {
        (self.kind, self.field, self.detail) > (other.kind, other.field, other.detail)
    }
}

/// Keeps the best match seen for one record.
#[derive(Clone, Copy, Debug, Default)]
pub struct BestMatch(Option<FieldMatch>);

impl BestMatch {
    pub fn consider(&mut self, candidate: FieldMatch) {
        match &self.0 {
            Some(current) if !candidate.beats(current) => {}
            _ => self.0 = Some(candidate),
        }
    }

    pub fn get(self) -> Option<FieldMatch> {
        self.0
    }

    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

/// Compares a query against a field without considering subsequences.
///
/// This is separated from [`fuzzy_match`] because it is far cheaper and,
/// whenever it finds anything at all, its result outranks every possible fuzzy
/// match. Searching runs this over every field first and only falls back to
/// fuzzy matching for records it found nothing for, which is what keeps the
/// per-keystroke cost flat as the catalog grows.
pub fn literal_match(text: &FoldedText, query: &[char], field: FieldKind) -> Option<FieldMatch> {
    let haystack = text.chars();
    if query.is_empty() || query.len() > haystack.len() {
        return None;
    }
    if haystack.len() == query.len() {
        return (haystack == query).then_some(FieldMatch {
            kind: MatchKind::Exact,
            field,
            detail: MAX_DETAIL,
        });
    }

    let mut word_prefix: Option<usize> = None;
    let mut substring: Option<usize> = None;
    for start in 0..=haystack.len() - query.len() {
        if &haystack[start..start + query.len()] != query {
            continue;
        }
        if start == 0 {
            return Some(FieldMatch {
                kind: MatchKind::Prefix,
                field,
                detail: run_detail(0, haystack.len(), query.len()),
            });
        }
        if text.starts_word(start) {
            word_prefix = Some(start);
            break;
        }
        substring.get_or_insert(start);
    }

    let (kind, start) = match (word_prefix, substring) {
        (Some(start), _) => (MatchKind::WordPrefix, start),
        (None, Some(start)) => (MatchKind::Substring, start),
        (None, None) => return None,
    };
    Some(FieldMatch {
        kind,
        field,
        detail: run_detail(start, haystack.len(), query.len()),
    })
}

/// Orders matches of the same kind: earlier in the field is better, and less
/// unmatched text left over is better. `fil` prefers `Files` over
/// `FileZilla Transfer Manager`.
fn run_detail(start: usize, text_len: usize, query_len: usize) -> u32 {
    let leftover = text_len - query_len;
    let penalty = (start as u32)
        .saturating_mul(20)
        .saturating_add(leftover as u32);
    MAX_DETAIL.saturating_sub(penalty.min(MAX_DETAIL - 1))
}

/// Stands in for "no match reaches here". Far enough below any real score that
/// arithmetic on it stays negative, and far enough above `i32::MIN` that it
/// cannot overflow.
const UNREACHABLE: i32 = -1_000_000;

/// What matching one more query character is worth.
const CHARACTER_VALUE: i32 = 10;
/// Landing on the first character of a word, which is what makes an acronym
/// like `gimp` find `GNU Image Manipulation Program`.
const WORD_START_BONUS: i32 = 40;
/// Landing on the first character of the field at all.
const FIELD_START_BONUS: i32 = 20;
/// Continuing directly from the previous matched character. Worth more than a
/// word start so a typed run stays ahead of the same letters scattered across
/// several words.
const ADJACENCY_BONUS: i32 = 50;
/// Charged for each character skipped between two matches.
const GAP_PENALTY: i32 = 3;

/// Scores the best subsequence match of `query` inside `text`, or `None` when
/// the characters do not appear in order at all.
///
/// The score rewards matching at word starts and matching consecutively and
/// charges for the characters skipped between matches. This is a heuristic,
/// not a distance metric: what it guarantees is that the same pair of strings
/// always produces the same number, and that the weights above are the only
/// thing that decides the order.
///
/// The dynamic program is linear in text length per query character. The
/// running maximum works because the gap penalty is linear, so the best
/// predecessor for one position is either the character immediately before it
/// or the best predecessor of the position before it, one gap step cheaper.
pub fn fuzzy_match(text: &FoldedText, query: &[char], field: FieldKind) -> Option<FieldMatch> {
    let haystack = text.chars();
    if query.is_empty() || query.len() > haystack.len() {
        return None;
    }
    // A forward greedy walk answers "is this even a subsequence?" in one pass.
    // Only text that survives it is worth scoring.
    let mut cursor = 0usize;
    for wanted in query {
        let offset = haystack[cursor..]
            .iter()
            .position(|character| character == wanted)?;
        cursor += offset + 1;
    }

    // `ending_here[position]` is the best score for the query characters
    // matched so far, ending exactly at `position`.
    let mut ending_here = vec![UNREACHABLE; haystack.len()];
    let mut previous = vec![UNREACHABLE; haystack.len()];
    for (depth, wanted) in query.iter().enumerate() {
        std::mem::swap(&mut ending_here, &mut previous);
        // The best non-adjacent predecessor strictly before the current
        // position, already charged for the gap it leaves.
        let mut carried = UNREACHABLE;
        for position in 0..haystack.len() {
            if position > 0 {
                let decayed = if carried <= UNREACHABLE {
                    UNREACHABLE
                } else {
                    carried - GAP_PENALTY
                };
                carried = previous[position - 1].max(decayed);
            }
            if haystack[position] != *wanted {
                ending_here[position] = UNREACHABLE;
                continue;
            }
            let bonus = CHARACTER_VALUE
                + if text.starts_word(position) {
                    WORD_START_BONUS
                } else {
                    0
                }
                + if position == 0 { FIELD_START_BONUS } else { 0 };
            ending_here[position] = if depth == 0 {
                bonus
            } else {
                let adjacent = match position.checked_sub(1) {
                    Some(before) if previous[before] > UNREACHABLE => {
                        previous[before] + ADJACENCY_BONUS
                    }
                    _ => UNREACHABLE,
                };
                match adjacent.max(carried) {
                    best if best <= UNREACHABLE => UNREACHABLE,
                    best => best + bonus,
                }
            };
        }
    }

    let raw = ending_here.iter().copied().max()?;
    if raw <= UNREACHABLE {
        return None;
    }
    Some(FieldMatch {
        kind: MatchKind::Fuzzy,
        field,
        detail: raw.clamp(0, MAX_DETAIL as i32) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(value: &str) -> Vec<char> {
        FoldedText::new(value).chars().to_vec()
    }

    fn literal(text: &str, value: &str) -> Option<FieldMatch> {
        literal_match(&FoldedText::new(text), &query(value), FieldKind::Name)
    }

    fn fuzzy(text: &str, value: &str) -> Option<FieldMatch> {
        fuzzy_match(&FoldedText::new(text), &query(value), FieldKind::Name)
    }

    fn kind(text: &str, value: &str) -> Option<MatchKind> {
        literal(text, value)
            .or_else(|| fuzzy(text, value))
            .map(|matched| matched.kind)
    }

    #[test]
    fn the_four_match_kinds_are_told_apart() {
        assert_eq!(kind("Files", "files"), Some(MatchKind::Exact));
        assert_eq!(kind("Files", "fil"), Some(MatchKind::Prefix));
        assert_eq!(
            kind("Disk Usage Analyzer", "usage"),
            Some(MatchKind::WordPrefix)
        );
        assert_eq!(
            kind("Disk Usage Analyzer", "sage"),
            Some(MatchKind::Substring)
        );
        assert_eq!(kind("Disk Usage Analyzer", "dua"), Some(MatchKind::Fuzzy));
        assert_eq!(kind("Disk Usage Analyzer", "qqq"), None);
    }

    #[test]
    fn a_stronger_kind_always_outscores_a_weaker_one_whatever_the_detail() {
        let fuzzy_best = FieldMatch {
            kind: MatchKind::Fuzzy,
            field: FieldKind::Name,
            detail: MAX_DETAIL,
        };
        let substring_worst = FieldMatch {
            kind: MatchKind::Substring,
            field: FieldKind::Executable,
            detail: 0,
        };
        assert!(substring_worst.score() > fuzzy_best.score());
    }

    #[test]
    fn a_name_outscores_secondary_metadata_at_the_same_kind() {
        let name = FieldMatch {
            kind: MatchKind::WordPrefix,
            field: FieldKind::Name,
            detail: 0,
        };
        let keyword = FieldMatch {
            kind: MatchKind::WordPrefix,
            field: FieldKind::Keyword,
            detail: MAX_DETAIL,
        };
        assert!(name.score() > keyword.score());
    }

    #[test]
    fn a_shorter_field_wins_a_prefix_match() {
        let short = literal("Files", "fil").expect("prefix");
        let long = literal("FileZilla Transfer Manager", "fil").expect("prefix");
        assert!(short.detail > long.detail);
    }

    #[test]
    fn an_earlier_word_wins_a_word_prefix_match() {
        let early = literal("Text Editor Extra", "editor").expect("word prefix");
        let late = literal("Extra Text Editor", "editor").expect("word prefix");
        assert!(early.detail > late.detail);
    }

    #[test]
    fn a_query_longer_than_the_field_matches_nothing() {
        assert!(literal("gimp", "gimpgimp").is_none());
        assert!(fuzzy("gimp", "gimpgimp").is_none());
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(literal("gimp", "").is_none());
        assert!(fuzzy("gimp", "").is_none());
    }

    #[test]
    fn fuzzy_prefers_a_match_that_lands_on_word_starts() {
        // Same length, same first character position: only where the second
        // character lands differs.
        let word_starts = fuzzy("ab cd", "ac").expect("subsequence");
        let inside_a_word = fuzzy("axcd ", "ac").expect("subsequence");
        assert!(word_starts.detail > inside_a_word.detail);
    }

    #[test]
    fn fuzzy_prefers_a_contiguous_run_over_a_scattered_one() {
        // Same length, same first character position: only the gap differs.
        let contiguous = fuzzy("acxxx", "ac").expect("subsequence");
        let scattered = fuzzy("axxxc", "ac").expect("subsequence");
        assert!(contiguous.detail > scattered.detail);
    }

    #[test]
    fn an_acronym_finds_the_words_it_abbreviates() {
        assert!(fuzzy("GNU Image Manipulation Program", "gimp").is_some());
    }

    #[test]
    fn fuzzy_refuses_characters_that_are_present_but_out_of_order() {
        assert!(fuzzy("pmig", "gimp").is_none());
    }

    #[test]
    fn fuzzy_matches_cjk_by_subsequence_too() {
        assert!(fuzzy("文字編輯器", "文器").is_some());
        assert!(fuzzy("文字編輯器", "器文").is_none());
    }

    #[test]
    fn a_usage_bonus_can_never_leave_the_bucket_the_match_earned() {
        let weak = FieldMatch {
            kind: MatchKind::Substring,
            field: FieldKind::Keyword,
            detail: MAX_DETAIL,
        };
        let strong = FieldMatch {
            kind: MatchKind::Substring,
            field: FieldKind::GenericName,
            detail: 0,
        };
        assert!(weak.adjusted_score(u32::MAX) < strong.score());
    }

    #[test]
    fn the_best_match_is_the_strongest_one_offered() {
        let mut best = BestMatch::default();
        assert!(best.is_none());
        best.consider(FieldMatch {
            kind: MatchKind::Fuzzy,
            field: FieldKind::Name,
            detail: 500,
        });
        best.consider(FieldMatch {
            kind: MatchKind::Substring,
            field: FieldKind::Keyword,
            detail: 0,
        });
        best.consider(FieldMatch {
            kind: MatchKind::Fuzzy,
            field: FieldKind::Name,
            detail: MAX_DETAIL,
        });
        assert_eq!(best.get().expect("a match").kind, MatchKind::Substring);
    }
}
