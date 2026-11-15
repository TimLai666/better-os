//! Where a wrapped diff line breaks, as columns of the code font.
//!
//! A diff row keeps a fixed height, so wrapping is not the text system's
//! automatic wrap: the window lays a line out as several rows of equal
//! height, the first with the line's numbers and mark and the rest indented
//! under it. That makes the layout a decision this module can state and a test
//! can assert, and it keeps the diff a `uniform_list`: only the rows on
//! screen are built, however long the file.
//!
//! The code font is the platform monospace one, so a narrow character is one
//! column and a wide one (CJK, fullwidth forms, emoji) two, as the font draws
//! them; combining marks, joiners and variation selectors take none
//! (`unicode-width`'s East Asian Width tables). The window measures one
//! column of the code font and passes the advance in ([`columns`]).

use std::ops::Range;

use unicode_width::UnicodeWidthChar;

/// A tab is drawn as four spaces, as [`crate::render`] always has, so the
/// layout counts it that way too.
pub const TAB_SPACES: usize = 4;

/// The most rows one line may take. A minified file can be one line of
/// hundreds of kilobytes; laying all of it out would make a list row per
/// screenful of a line nobody reads in a diff. The last row ends where it
/// fills and says how much of the line is not shown ([`Rows::hidden`]); the
/// line's own actions (staging it, ticking it) are unaffected because they
/// work from the line's number and text, not from the rows.
pub const MAX_ROWS: usize = 4096;

/// Zero width joiner: the characters either side of it are one glyph.
const ZWJ: char = '\u{200D}';
/// Emoji presentation selector: the character before it is drawn as a
/// two-column emoji.
const VS16: char = '\u{FE0F}';

/// The line as it is drawn: tabs as spaces.
pub fn expand(text: &str) -> String {
    text.replace('\t', &" ".repeat(TAB_SPACES))
}

/// How many columns one character takes on its own: two for a wide one,
/// none for a combining mark, a joiner, a variation selector or a control.
pub fn char_columns(character: char) -> usize {
    character.width().unwrap_or(0)
}

/// How many columns a string takes, a narrow character followed by the
/// emoji presentation selector counting as the two-column emoji it is.
pub fn columns_of(text: &str) -> usize {
    let mut total = 0;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        total += step(character, chars.peek().copied());
    }
    total
}

/// The columns `character` adds when `next` follows it.
fn step(character: char, next: Option<char>) -> usize {
    let own = char_columns(character);
    if own == 1 && next == Some(VS16) {
        2
    } else {
        own
    }
}

/// How many columns fit in `width` pixels when one column advances `advance`
/// pixels. Always at least one, so a narrow window wraps rather than drawing
/// nothing.
pub fn columns(width: f32, advance: f32) -> usize {
    if !(width.is_finite() && advance.is_finite()) || advance <= 0.0 {
        return 1;
    }
    ((width / advance).floor() as usize).max(1)
}

/// A line's visual rows, and how many characters past the last one are not
/// drawn because the line reached [`MAX_ROWS`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rows {
    /// Byte ranges into the expanded line.
    pub ranges: Vec<Range<usize>>,
    /// Characters after the last row, not shown; zero for every line under
    /// the cap.
    pub hidden: usize,
}

/// The visual rows of one line.
///
/// A row takes `columns` columns at most, excepting a single wide character
/// where not even that fits. A row ends at the last space in its second half
/// when there is one, so words and code stay together the way a browser's
/// `pre-wrap` would break them, and breaks mid-token only when a token is
/// longer than a row. A row never ends inside a joined emoji or before a
/// combining mark: zero-width characters stay with the one before them.
pub fn rows(text: &str, columns: usize) -> Rows {
    rows_capped(text, columns, MAX_ROWS)
}

/// [`rows`] with the cap given, for the test that the cap holds.
pub fn rows_capped(text: &str, columns: usize, max_rows: usize) -> Rows {
    let columns = columns.max(1);
    let mut ranges = Vec::new();
    if text.is_empty() {
        ranges.push(0..0);
        return Rows { ranges, hidden: 0 };
    }
    let mut start = 0;
    while start < text.len() {
        let last = ranges.len() + 1 >= max_rows;
        let mut used = 0;
        let mut end = start;
        let mut last_space = None;
        let mut chars = text[start..].char_indices().peekable();
        let mut after_joiner = false;
        while let Some((offset, character)) = chars.next() {
            let width = step(character, chars.peek().map(|(_, c)| *c));
            // Marks, selectors and joiners draw on the glyph before them, and
            // what a joiner joins is the same glyph: none of them starts a
            // row.
            let attached = width == 0 || after_joiner;
            if !attached && used + width > columns && end > start {
                break;
            }
            if character == ' ' {
                last_space = Some(start + offset);
            }
            used += width;
            end = start + offset + character.len_utf8();
            after_joiner = character == ZWJ;
        }
        if end < text.len() && !last {
            // A break that splits a word is the last resort: prefer a space
            // in the second half of the row, and keep it at the row's end.
            if let Some(space) = last_space.filter(|space| *space > start + (end - start) / 2) {
                end = space + 1;
            }
        }
        ranges.push(start..end);
        if last {
            return Rows {
                ranges,
                hidden: text[end..].chars().count(),
            };
        }
        start = end;
    }
    Rows { ranges, hidden: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str, columns: usize) -> Vec<&str> {
        rows(text, columns)
            .ranges
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn a_line_breaks_at_the_column_count() {
        assert_eq!(texts("abcdef", 3), ["abc", "def"]);
        assert_eq!(texts("abcdef", 4), ["abcd", "ef"]);
        // Exactly the width is one row, not a trailing empty one.
        assert_eq!(texts("abcd", 4), ["abcd"]);
        // Nothing is a row of nothing, not no row at all.
        assert_eq!(texts("", 10), [""]);
        // One column always fits, whatever the window.
        assert_eq!(columns(0.0, 7.8), 1);
        assert_eq!(columns(f32::NAN, 7.8), 1);
        assert_eq!(columns(78.0, 0.0), 1);
        assert_eq!(columns(780.0, 7.8), 100);
    }

    #[test]
    fn a_break_prefers_a_space_in_the_second_half() {
        // "hello wo" is eight columns; the space at six is in the second
        // half, so the row ends there.
        assert_eq!(texts("hello world", 8), ["hello ", "world"]);
        // The space at two is not: breaking there would waste the row.
        assert_eq!(texts("a bbbbbbbbbb", 8), ["a bbbbbb", "bbbb"]);
        // A token longer than a row breaks mid-token.
        assert_eq!(texts("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn wide_characters_take_two_columns_and_never_split() {
        assert_eq!(columns_of("日本語"), 6);
        assert_eq!(columns_of("abc"), 3);
        assert_eq!(texts("日本語", 4), ["日本", "語"]);
        // A wide character in a one-column row gets a row of its own.
        assert_eq!(texts("日", 1), ["日"]);
        // Multi-byte characters are never cut in half.
        for range in rows("héllo→wörld", 3).ranges {
            assert!("héllo→wörld".is_char_boundary(range.start));
            assert!("héllo→wörld".is_char_boundary(range.end));
        }
    }

    #[test]
    fn emoji_marks_joiners_selectors_and_extension_b_are_counted_as_drawn() {
        // Emoji are two columns.
        assert_eq!(columns_of("😀"), 2);
        assert_eq!(texts("😀😀😀", 4), ["😀😀", "😀"]);
        // A combining mark takes none and stays with its letter.
        assert_eq!(columns_of("e\u{301}"), 1);
        assert_eq!(texts("abe\u{301}cd", 3), ["abe\u{301}", "cd"]);
        // A joined family is one glyph: never broken at the joiner.
        let family = "👨\u{200D}👩\u{200D}👧";
        assert_eq!(char_columns('\u{200D}'), 0);
        assert_eq!(
            texts(&format!("ab{family}cd"), 4),
            [&format!("ab{family}") as &str, "cd"]
        );
        // The emoji presentation selector makes a narrow symbol two columns
        // and stays with it.
        assert_eq!(columns_of("\u{2764}\u{FE0F}"), 2);
        assert_eq!(texts("a\u{2764}\u{FE0F}b", 3), ["a\u{2764}\u{FE0F}", "b"]);
        // CJK Extension B (outside the Basic Multilingual Plane) is wide.
        assert_eq!(columns_of("\u{20000}\u{2A6DF}"), 4);
        assert_eq!(
            texts("\u{20000}\u{20001}\u{20002}", 4),
            ["\u{20000}\u{20001}", "\u{20002}"]
        );
    }

    #[test]
    fn tabs_are_four_spaces_and_the_cap_says_what_it_hides() {
        assert_eq!(expand("\tx"), "    x");
        assert_eq!(columns_of(&expand("\t")), 4);
        let text = "a".repeat(100);
        let capped = rows_capped(&text, 2, 3);
        assert_eq!(capped.ranges, [0..2, 2..4, 4..6]);
        // The rest of the line is counted, for the "truncated" marker.
        assert_eq!(capped.hidden, 94);
        assert_eq!(rows(&text, 10).hidden, 0);
    }
}
