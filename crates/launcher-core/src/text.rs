//! Folding a searchable string into the form the matcher compares.
//!
//! Two people typing the same application name do not type the same bytes.
//! `Café` is typed `cafe`, `Übersicht` is typed `ubersicht`, and `編輯器` is
//! typed one ideograph at a time. Folding removes case and Latin diacritics so
//! those all reach the same comparison, and it records where each word begins
//! so a match on the start of the second word can be told apart from a match
//! in the middle of it.
//!
//! Folding happens once per field when the index is built, never per
//! keystroke. The query is folded once per keystroke, which is the only
//! per-keystroke allocation the matcher performs.
//!
//! Scope is deliberate: case folding is Unicode-wide through
//! [`char::to_lowercase`], diacritic folding covers Latin-1 Supplement and
//! Latin Extended-A, and every other script — CJK included — passes through
//! unchanged because there is nothing to strip.

/// A field prepared for matching: its folded characters, where its words
/// begin, and a coarse membership mask used to reject impossible queries
/// before any character comparison happens.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FoldedText {
    chars: Vec<char>,
    word_starts: Vec<bool>,
    mask: u64,
}

impl FoldedText {
    /// Folds one string for matching.
    pub fn new(input: &str) -> Self {
        let mut chars = Vec::with_capacity(input.len());
        let mut word_starts = Vec::with_capacity(input.len());
        let mut mask = 0u64;
        let mut previous: Option<char> = None;
        for source in input.chars() {
            let starts_word = is_word_start(previous, source);
            let before = chars.len();
            for lowered in source.to_lowercase() {
                match strip_diacritics(lowered) {
                    Some(replacement) => chars.extend(replacement.chars()),
                    None => chars.push(lowered),
                }
            }
            // Only the first character of an expansion begins a word: `ß`
            // folds to `ss`, and the second `s` is not a new word.
            for (position, folded) in chars.iter().enumerate().skip(before) {
                word_starts.push(starts_word && position == before);
                mask |= character_bit(*folded);
            }
            previous = Some(source);
        }
        Self {
            chars,
            word_starts,
            mask,
        }
    }

    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// Whether the character at `position` begins a word.
    pub fn starts_word(&self, position: usize) -> bool {
        self.word_starts.get(position).copied().unwrap_or(false)
    }

    /// The set of character classes this text contains. A query whose mask is
    /// not a subset cannot match, which is checked in one instruction instead
    /// of one scan per field.
    pub fn mask(&self) -> u64 {
        self.mask
    }

    /// Whether this text could possibly contain every character in `mask`.
    pub fn could_contain(&self, mask: u64) -> bool {
        self.mask & mask == mask
    }
}

/// Buckets a folded character into one of 64 classes. Letters and digits get
/// their own bit; everything else shares the remaining bits, which is enough
/// for a prefilter that is only allowed to produce false positives.
fn character_bit(character: char) -> u64 {
    let index = match character {
        'a'..='z' => character as u32 - 'a' as u32,
        '0'..='9' => 26 + (character as u32 - '0' as u32),
        other => 36 + (other as u32 % 28),
    };
    1u64 << index
}

/// Whether `current` begins a word, given the character before it.
///
/// Separators are not word starts themselves. A capital after a lowercase
/// letter or a digit starts a word, so `LibreOffice` is reachable by typing
/// `office`. Every CJK ideograph starts a word, because CJK text carries no
/// spaces and a one-character query is a legitimate word query there.
fn is_word_start(previous: Option<char>, current: char) -> bool {
    if !current.is_alphanumeric() {
        return false;
    }
    let Some(previous) = previous else {
        return true;
    };
    if !previous.is_alphanumeric() {
        return true;
    }
    if is_ideograph(current) || is_ideograph(previous) {
        return true;
    }
    (previous.is_lowercase() || previous.is_numeric()) && current.is_uppercase()
}

/// CJK ideographs and the Japanese kana, which behave the same way here: each
/// character is a word for the purposes of a search query.
fn is_ideograph(character: char) -> bool {
    matches!(character as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// The base form of a lowercase Latin letter carrying a diacritic, or `None`
/// when the character needs no folding.
fn strip_diacritics(character: char) -> Option<&'static str> {
    Some(match character {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĳ' => "ij",
        'ĵ' => "j",
        'ķ' | 'ĸ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' | 'ŋ' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folded(input: &str) -> String {
        FoldedText::new(input).chars().iter().collect()
    }

    fn word_starts(input: &str) -> Vec<usize> {
        let text = FoldedText::new(input);
        (0..text.len()).filter(|i| text.starts_word(*i)).collect()
    }

    #[test]
    fn case_is_folded_away() {
        assert_eq!(folded("Visual Studio Code"), "visual studio code");
        assert_eq!(folded("GIMP"), "gimp");
    }

    #[test]
    fn latin_diacritics_fold_to_their_base_letters() {
        assert_eq!(folded("Café"), "cafe");
        assert_eq!(folded("Übersicht"), "ubersicht");
        assert_eq!(folded("Þjóð"), "thjod");
        assert_eq!(folded("Straße"), "strasse");
        assert_eq!(folded("Ærø"), "aero");
    }

    #[test]
    fn cjk_passes_through_untouched() {
        assert_eq!(folded("文字編輯器"), "文字編輯器");
        assert_eq!(folded("ファイル"), "ファイル");
    }

    #[test]
    fn words_start_after_separators_and_at_internal_capitals() {
        assert_eq!(word_starts("Visual Studio Code"), vec![0, 7, 14]);
        assert_eq!(word_starts("LibreOffice"), vec![0, 5]);
        assert_eq!(word_starts("gnome-terminal"), vec![0, 6]);
        assert_eq!(word_starts("Krita5"), vec![0]);
    }

    #[test]
    fn every_ideograph_starts_a_word_because_cjk_text_has_no_spaces() {
        assert_eq!(word_starts("文字編輯器"), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn an_expansion_only_starts_a_word_at_its_first_character() {
        // `ß` folds to two characters; the trailing `s` is not a new word.
        assert_eq!(word_starts("Straße"), vec![0]);
    }

    #[test]
    fn the_mask_rejects_a_query_the_text_cannot_contain() {
        let text = FoldedText::new("Files");
        assert!(text.could_contain(FoldedText::new("file").mask()));
        assert!(!text.could_contain(FoldedText::new("gimp").mask()));
    }

    #[test]
    fn the_mask_is_computed_after_folding_so_diacritics_do_not_hide_a_match() {
        let text = FoldedText::new("Café");
        assert!(text.could_contain(FoldedText::new("cafe").mask()));
    }

    #[test]
    fn an_empty_string_folds_to_nothing() {
        let text = FoldedText::new("");
        assert!(text.is_empty());
        assert_eq!(text.mask(), 0);
        assert!(!text.starts_word(0));
    }
}
