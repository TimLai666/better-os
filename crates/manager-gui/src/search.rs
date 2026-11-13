//! "Find a setting": the index every page contributes to, and how a query
//! matches it (spec section 1, "Search").
//!
//! A query matches a settings card when it is the start of any word of the
//! card's header or description, ignoring case for Latin text; a query with
//! more than one word matches where it starts at a word. Chinese has no
//! spaces between words, so a query with CJK text in it matches anywhere in
//! the text instead. Cards whose header matched come before cards whose
//! description did, each in the order the pages registered them, and the
//! list stops at eight.

use crate::navigation::Route;

/// The most suggestions the search box shows.
pub(crate) const MAX_RESULTS: usize = 8;

/// One settings card, as the search knows it. Built from the same copy table
/// the page draws the card from, so the two cannot disagree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SearchEntry {
    /// The card's header.
    pub(crate) header: &'static str,
    /// The card's description, or empty.
    pub(crate) description: &'static str,
    /// The page's title, in the current language.
    pub(crate) page: &'static str,
    /// The card's group header, when the page groups its cards.
    pub(crate) group: Option<&'static str>,
    /// The pane item the card lives under.
    pub(crate) route: Route,
    /// The card's id on its page, which the page scrolls to and focuses.
    pub(crate) card: &'static str,
}

impl SearchEntry {
    /// The line under the header in the suggestion list:
    /// `{page} › {group}`, or the page alone.
    pub(crate) fn location(&self) -> String {
        match self.group {
            Some(group) => format!("{} › {group}", self.page),
            None => self.page.to_string(),
        }
    }
}

/// Whether a character is written without spaces between words: Han,
/// kana, Hangul, and the CJK punctuation and full-width forms.
pub(crate) fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x2E80..=0x2FDF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3100..=0x312F
            | 0x3130..=0x318F
            | 0x31A0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FA1F
    )
}

/// Whether `query`, already lower-cased and trimmed, matches `text`.
fn matches_text(text: &str, query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    let text = text.to_lowercase();
    if query.chars().any(is_cjk) {
        return text.contains(query);
    }
    let mut at_word_start = true;
    for (index, character) in text.char_indices() {
        if at_word_start && text[index..].starts_with(query) {
            return true;
        }
        // A CJK character is a word of its own, so the Latin word after it
        // (`Better OS` in `安裝 Better OS`) still starts a word.
        at_word_start = !character.is_alphanumeric() || is_cjk(character);
    }
    false
}

/// How well an entry matches: by its header, by its description, or not.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Match {
    Header,
    Description,
}

fn match_of(entry: &SearchEntry, query: &str) -> Option<Match> {
    if matches_text(entry.header, query) {
        Some(Match::Header)
    } else if matches_text(entry.description, query) {
        Some(Match::Description)
    } else {
        None
    }
}

/// The entries a query finds, best first, at most [`MAX_RESULTS`].
pub(crate) fn find<'a>(entries: &'a [SearchEntry], query: &str) -> Vec<&'a SearchEntry> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(Match, usize, &SearchEntry)> = entries
        .iter()
        .enumerate()
        .filter_map(|(order, entry)| match_of(entry, &query).map(|m| (m, order, entry)))
        .collect();
    found.sort_by_key(|(how, order, _)| (*how, *order));
    found
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_, _, entry)| entry)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The charge-limit card ticket 86 registers, in both languages, and a
    /// few neighbours, standing in until that page exists.
    fn fixture() -> Vec<SearchEntry> {
        let entry = |header, description, page, group, card| SearchEntry {
            header,
            description,
            page,
            group,
            route: Route::System,
            card,
        };
        vec![
            entry(
                "Power mode",
                "Balance battery life against performance.",
                "System",
                Some("Power"),
                "power-mode",
            ),
            entry(
                "Battery charge limit",
                "Stopping short of full extends battery life when you're mostly plugged in.",
                "System",
                Some("Power"),
                "charge-limit",
            ),
            entry(
                "電池充電上限",
                "長時間接著電源時，充到較低的電量可以延長電池壽命。",
                "系統",
                Some("電源"),
                "charge-limit",
            ),
            entry(
                "電源模式",
                "在電池續航力和效能之間取捨。",
                "系統",
                Some("電源"),
                "power-mode",
            ),
            entry(
                "Brightness",
                "Change the brightness of the built-in display.",
                "System",
                Some("Display"),
                "brightness",
            ),
        ]
    }

    fn cards(found: &[&SearchEntry]) -> Vec<&'static str> {
        found.iter().map(|entry| entry.card).collect()
    }

    #[test]
    fn charge_finds_the_charge_limit_card_in_english() {
        let entries = fixture();
        let found = find(&entries, "charge");
        assert_eq!(cards(&found), ["charge-limit"]);
        assert_eq!(found[0].header, "Battery charge limit");
        assert_eq!(found[0].location(), "System › Power");
    }

    #[test]
    fn a_chinese_query_matches_inside_words_with_no_spaces() {
        let entries = fixture();
        let found = find(&entries, "充電");
        assert_eq!(cards(&found), ["charge-limit"]);
        assert_eq!(found[0].header, "電池充電上限");
        assert_eq!(found[0].location(), "系統 › 電源");
        // 電池 is in both cards' text: the header match comes first.
        assert_eq!(
            cards(&find(&entries, "電池")),
            ["charge-limit", "power-mode"]
        );
    }

    #[test]
    fn latin_text_matches_the_start_of_a_word_whatever_its_case() {
        let entries = fixture();
        assert_eq!(cards(&find(&entries, "CHARGE")), ["charge-limit"]);
        assert_eq!(cards(&find(&entries, "bri")), ["brightness"]);
        // The middle of a word is not a match.
        assert!(find(&entries, "arge").is_empty());
        assert!(find(&entries, "ightness").is_empty());
    }

    #[test]
    fn a_header_match_outranks_a_description_match() {
        let entries = fixture();
        // "battery" is Battery charge limit's header and Power mode's
        // description.
        assert_eq!(
            cards(&find(&entries, "battery")),
            ["charge-limit", "power-mode"]
        );
    }

    #[test]
    fn several_words_match_where_they_start_at_a_word() {
        let entries = fixture();
        assert_eq!(cards(&find(&entries, "charge li")), ["charge-limit"]);
        assert_eq!(
            cards(&find(&entries, "  battery charge  ")),
            ["charge-limit"]
        );
        assert!(find(&entries, "charge power").is_empty());
    }

    #[test]
    fn an_empty_query_finds_nothing_and_a_miss_finds_nothing() {
        let entries = fixture();
        assert!(find(&entries, "").is_empty());
        assert!(find(&entries, "   ").is_empty());
        assert!(find(&entries, "bluetooth").is_empty());
        assert!(find(&entries, "藍牙").is_empty());
        assert!(find(&[], "charge").is_empty());
    }

    #[test]
    fn at_most_eight_suggestions_are_shown() {
        let many: Vec<SearchEntry> = (0..20)
            .map(|_| SearchEntry {
                header: "Setting",
                description: "",
                page: "Settings",
                group: None,
                route: Route::Settings,
                card: "setting",
            })
            .collect();
        assert_eq!(find(&many, "set").len(), MAX_RESULTS);
        assert_eq!(MAX_RESULTS, 8);
    }

    #[test]
    fn a_latin_word_after_chinese_text_still_starts_a_word() {
        assert!(matches_text("安裝 Better OS 元件", "better"));
        assert!(matches_text("Better OS預設為深色", "better"));
        assert!(matches_text("與Better", "better"));
        assert!(!is_cjk('a'));
        assert!(is_cjk('電'));
        assert!(is_cjk('，'));
    }
}
