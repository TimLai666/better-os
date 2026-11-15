//! Where a wrapped diff line breaks, as columns of the code font.
//!
//! A diff row keeps a fixed height, so wrapping is not the text system's
//! automatic wrap: the session lays a line out as several rows of equal
//! height, the first with the line's numbers and mark and the rest indented
//! under it. That makes the layout a decision this module can state and a test
//! can assert, and it keeps the diff a `uniform_list` — only the rows on
//! screen are built, however long the file.
//!
//! The code font is the platform monospace one, so a character is one column
//! wide and one advance unit costs the same at any position on the row. Wide
//! scripts (CJK, fullwidth forms) are two columns, as they are in the font.
//! The estimate is deliberately the font's own advance rather than a
//! character-count guess: the window measures one row of the code font and
//! passes the advance in ([`columns`]).

use std::ops::Range;

/// A tab is drawn as four spaces, as [`crate::render`] always has, so the
/// layout counts it that way too.
pub const TAB_SPACES: usize = 4;

/// The most rows one line may take. A minified file can be one line of
/// hundreds of kilobytes; laying all of it out would make a list row per
/// screenful of a line nobody reads in a diff. Past this the last row runs to
/// the end of the line and is clipped, and the line's own actions (staging it,
/// ticking it) are unaffected because they work from the line's number and
/// text, not from the rows.
pub const MAX_ROWS: usize = 4096;

/// The line as it is drawn: tabs as spaces.
pub fn expand(text: &str) -> String {
    text.replace('\t', &" ".repeat(TAB_SPACES))
}

/// How many columns one character takes: a wide character is two.
pub fn char_columns(character: char) -> usize {
    let wide = matches!(
        character as u32,
        0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
    );
    if wide { 2 } else { 1 }
}

/// How many columns a string takes.
pub fn columns_of(text: &str) -> usize {
    text.chars().map(char_columns).sum()
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

/// The visual rows of one line, as byte ranges into the expanded line.
///
/// A row takes `columns` columns at most, excepting a single wide character
/// where not even that fits. A row ends at the last space in its second half
/// when there is one, so words and code stay together the way a browser's
/// `pre-wrap` would break them, and breaks mid-token only when a token is
/// longer than a row. The last row runs to the end of the line.
pub fn rows(text: &str, columns: usize) -> Vec<Range<usize>> {
    rows_capped(text, columns, MAX_ROWS)
}

/// [`rows`] with the cap given, for the test that the cap holds.
pub fn rows_capped(text: &str, columns: usize, max_rows: usize) -> Vec<Range<usize>> {
    let columns = columns.max(1);
    if text.is_empty() {
        return std::iter::once(0..0).collect();
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < text.len() {
        if out.len() + 1 >= max_rows {
            out.push(start..text.len());
            return out;
        }
        let mut used = 0;
        let mut end = start;
        let mut last_space = None;
        for (offset, character) in text[start..].char_indices() {
            let width = char_columns(character);
            if used + width > columns {
                break;
            }
            if character == ' ' {
                last_space = Some(start + offset);
            }
            used += width;
            end = start + offset + character.len_utf8();
        }
        if end == start {
            // One character wider than the whole row: it still gets a row of
            // its own rather than looping for ever.
            let character = text[start..].chars().next().expect("not empty");
            end = start + character.len_utf8();
        } else if end < text.len() {
            // A break that splits a word is the last resort: prefer a space
            // in the second half of the row, and keep it at the row's end.
            if let Some(space) = last_space.filter(|space| *space > start + (end - start) / 2) {
                end = space + 1;
            }
        }
        out.push(start..end);
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str, columns: usize) -> Vec<&str> {
        rows(text, columns)
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
        for range in rows("héllo→wörld", 3) {
            assert!("héllo→wörld".is_char_boundary(range.start));
            assert!("héllo→wörld".is_char_boundary(range.end));
        }
    }

    #[test]
    fn tabs_are_four_spaces_and_the_cap_holds() {
        assert_eq!(expand("\tx"), "    x");
        assert_eq!(columns_of(&expand("\t")), 4);
        let text = "a".repeat(100);
        let capped = rows_capped(&text, 2, 3);
        assert_eq!(capped.len(), 3);
        assert_eq!(capped[0], 0..2);
        assert_eq!(capped[1], 2..4);
        // The last row runs to the end, so no cell is silently dropped from
        // the line the row's actions apply to.
        assert_eq!(capped[2], 4..100);
    }
}
