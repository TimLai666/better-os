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
