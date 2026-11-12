//! A unified diff for one file, read into hunks and lines.

use crate::ParseError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

/// One line of a hunk, without its leading ` `, `+`, or `-`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    /// The line's bytes decoded lossily, without the trailing newline.
    pub text: String,
    /// Line number in the old file, for context and removed lines.
    pub old_line: Option<u32>,
    /// Line number in the new file, for context and added lines.
    pub new_line: Option<u32>,
    /// True when git followed this line with `\ No newline at end of file`.
    pub no_newline_at_end: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@` header line as git printed it.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// What one file's diff holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileDiff {
    /// A text diff. No hunks means the content did not change (a mode change,
    /// or an empty new file).
    Text { hunks: Vec<Hunk> },
    /// git reported `Binary files ... differ`.
    Binary,
}

/// Reads the output of `git diff --no-color --no-ext-diff` (or the same with
/// `--cached`) for exactly one path.
///
/// Expected results:
/// - Lines before the first `@@` (the `diff --git`, `index`, `---`, `+++`,
///   mode, and rename headers) are skipped.
/// - Each `@@ -a[,b] +c[,d] @@` starts a hunk; a missing count is 1. Lines in
///   the hunk get `old_line` and `new_line` counted from `a` and `c`.
/// - A line starting with ` ` is context, `+` added, `-` removed. The prefix
///   is not part of `text`.
/// - `\ No newline at end of file` sets `no_newline_at_end` on the line
///   before it and is not a line of its own.
/// - A line `Binary files ... differ` anywhere before a hunk yields
///   `FileDiff::Binary`.
/// - A second `diff --git` header after a hunk ends that hunk; the hunks of
///   both diffs are returned in order. git prints a type change this way.
/// - Empty input is `FileDiff::Text { hunks: vec![] }`.
/// - Bytes that are not UTF-8 are decoded lossily; they never fail the parse.
/// - A hunk header that cannot be read fails with `Malformed`.
pub fn parse_diff(output: &[u8]) -> Result<FileDiff, ParseError> {
    let mut lines: Vec<&[u8]> = output.split(|&byte| byte == b'\n').collect();
    if lines.last().is_some_and(|last| last.is_empty()) {
        lines.pop();
    }

    let mut hunks = Vec::new();
    let mut hunk: Option<Hunk> = None;
    let (mut old_no, mut new_no) = (0u32, 0u32);

    for raw in lines {
        if raw.starts_with(b"@@") {
            hunks.extend(hunk.take());
            let header = String::from_utf8_lossy(raw).into_owned();
            (old_no, new_no) = parse_hunk_header(&header)?;
            hunk = Some(Hunk {
                header,
                lines: Vec::new(),
            });
            continue;
        }

        // A change of type (a file that became a symlink) is printed as two
        // diffs for the one path: a deletion, then an addition. No line of a
        // hunk can start with `d`, so this is the next diff's header.
        if hunk.is_some() && raw.starts_with(b"diff --git ") {
            hunks.extend(hunk.take());
            continue;
        }

        let Some(current) = hunk.as_mut() else {
            // Before the first hunk: file headers, which are skipped, or the
            // notice that a binary file differs.
            if raw.starts_with(b"Binary files ") && raw.ends_with(b" differ") {
                return Ok(FileDiff::Binary);
            }
            continue;
        };

        if raw.starts_with(b"\\") {
            if let Some(last) = current.lines.last_mut() {
                last.no_newline_at_end = true;
            }
            continue;
        }

        let (kind, body) = match raw.first().copied() {
            Some(b'+') => (DiffLineKind::Added, &raw[1..]),
            Some(b'-') => (DiffLineKind::Removed, &raw[1..]),
            Some(b' ') => (DiffLineKind::Context, &raw[1..]),
            None => (DiffLineKind::Context, raw),
            Some(_) => return Err(malformed("unexpected line inside a hunk")),
        };
        let (old_line, new_line) = match kind {
            DiffLineKind::Context => {
                let numbers = (Some(old_no), Some(new_no));
                old_no = old_no.saturating_add(1);
                new_no = new_no.saturating_add(1);
                numbers
            }
            DiffLineKind::Removed => {
                let number = (Some(old_no), None);
                old_no = old_no.saturating_add(1);
                number
            }
            DiffLineKind::Added => {
                let number = (None, Some(new_no));
                new_no = new_no.saturating_add(1);
                number
            }
        };
        current.lines.push(DiffLine {
            kind,
            text: String::from_utf8_lossy(body).into_owned(),
            old_line,
            new_line,
            no_newline_at_end: false,
        });
    }

    hunks.extend(hunk.take());
    Ok(FileDiff::Text { hunks })
}

fn malformed(detail: impl Into<String>) -> ParseError {
    ParseError::Malformed(detail.into())
}

/// Reads `@@ -a[,b] +c[,d] @@` and returns the first line numbers `(a, c)`.
/// A missing count means one line and is not checked further.
fn parse_hunk_header(header: &str) -> Result<(u32, u32), ParseError> {
    let rest = header.strip_prefix("@@ -").ok_or_else(bad_hunk_header)?;
    let (old_part, rest) = rest.split_once(" +").ok_or_else(bad_hunk_header)?;
    let (new_part, _) = rest.split_once(" @@").ok_or_else(bad_hunk_header)?;
    let old_start = range_start(old_part).ok_or_else(bad_hunk_header)?;
    let new_start = range_start(new_part).ok_or_else(bad_hunk_header)?;
    Ok((old_start, new_start))
}

fn bad_hunk_header() -> ParseError {
    malformed("hunk header is not `@@ -a[,b] +c[,d] @@`")
}

/// Reads `a` or `a,b` and returns `a`.
fn range_start(part: &str) -> Option<u32> {
    let mut fields = part.splitn(2, ',');
    let start: u32 = fields.next()?.parse().ok()?;
    if let Some(count) = fields.next() {
        let _: u32 = count.parse().ok()?;
    }
    Some(start)
}

/// One row of the side-by-side diff: the line shown on the left (the old
/// file) and on the right (the new file), as indexes into the hunk's lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitRow {
    pub left: Option<usize>,
    pub right: Option<usize>,
}

/// Lays a hunk out side by side, the way GitHub Desktop's split view does.
///
/// Expected results: a context line is on both sides; a run of removed lines
/// followed by a run of added lines is paired row by row, and the longer run
/// leaves the other side empty; an added line with no removal before it has
/// an empty left side.
pub fn split_rows(hunk: &Hunk) -> Vec<SplitRow> {
    let mut rows = Vec::new();
    let mut removed: Vec<usize> = Vec::new();
    let mut added: Vec<usize> = Vec::new();
    let flush = |rows: &mut Vec<SplitRow>, removed: &mut Vec<usize>, added: &mut Vec<usize>| {
        for i in 0..removed.len().max(added.len()) {
            rows.push(SplitRow {
                left: removed.get(i).copied(),
                right: added.get(i).copied(),
            });
        }
        removed.clear();
        added.clear();
    };
    for (index, line) in hunk.lines.iter().enumerate() {
        match line.kind {
            DiffLineKind::Removed => {
                if !added.is_empty() {
                    flush(&mut rows, &mut removed, &mut added);
                }
                removed.push(index);
            }
            DiffLineKind::Added => added.push(index),
            DiffLineKind::Context => {
                flush(&mut rows, &mut removed, &mut added);
                rows.push(SplitRow {
                    left: Some(index),
                    right: Some(index),
                });
            }
        }
    }
    flush(&mut rows, &mut removed, &mut added);
    rows
}

/// A diff larger than this many bytes is not shown until the person asks,
/// as GitHub Desktop does: drawing it would stall the window.
pub const LARGE_DIFF_BYTES: usize = 3 * 1024 * 1024;

/// A diff with more lines than this is not shown until the person asks.
pub const LARGE_DIFF_LINES: usize = 20_000;

/// Whether a diff is too large to show without asking.
pub fn is_large(diff: &FileDiff) -> bool {
    match diff {
        FileDiff::Binary => false,
        FileDiff::Text { hunks } => {
            let lines: usize = hunks.iter().map(|h| h.lines.len()).sum();
            let bytes: usize = hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .map(|l| l.text.len() + 1)
                .sum();
            lines > LARGE_DIFF_LINES || bytes > LARGE_DIFF_BYTES
        }
    }
}

/// What identifies a changed line however much context the diff around it
/// shows: its number in the old file (a removal) or the new one (an
/// addition). Context lines have both.
pub type LineKey = (Option<u32>, Option<u32>);

pub fn line_key(line: &DiffLine) -> LineKey {
    (line.old_line, line.new_line)
}

/// A fingerprint of every change in a diff: each changed line's kind,
/// numbers, and text, and whether git said it ends without a newline. Two
/// diffs of one file read with different amounts of context (three lines,
/// or the whole file) have the same fingerprint; any change to the file or
/// to HEAD that changes what would be committed changes it. A choice of
/// lines made on one diff is applied only to a diff with the same
/// fingerprint.
pub fn change_fingerprint(diff: &FileDiff) -> u64 {
    // FNV-1a, 64 bits: stable across runs and builds.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    match diff {
        FileDiff::Binary => feed(b"binary"),
        FileDiff::Text { hunks } => {
            for line in hunks.iter().flat_map(|h| &h.lines) {
                let kind = match line.kind {
                    DiffLineKind::Context => continue,
                    DiffLineKind::Added => b'+',
                    DiffLineKind::Removed => b'-',
                };
                feed(&[kind, u8::from(line.no_newline_at_end)]);
                feed(&line.old_line.unwrap_or(0).to_le_bytes());
                feed(&line.new_line.unwrap_or(0).to_le_bytes());
                feed(&(line.text.len() as u64).to_le_bytes());
                feed(line.text.as_bytes());
            }
        }
    }
    hash
}
