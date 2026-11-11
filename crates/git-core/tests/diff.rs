//! Tests for `git_core::parse_diff`, against the output of
//! `git diff --no-color --no-ext-diff` for one path. The samples follow git's
//! unified diff format, the same as git 2.43 prints it.

use git_core::{DiffLine, DiffLineKind, FileDiff, Hunk, ParseError, parse_diff};

fn text_hunks(output: &[u8]) -> Vec<Hunk> {
    match parse_diff(output) {
        Ok(FileDiff::Text { hunks }) => hunks,
        other => panic!("expected Ok(Text), got {other:?}"),
    }
}

fn line(kind: DiffLineKind, text: &str, old_line: Option<u32>, new_line: Option<u32>) -> DiffLine {
    DiffLine {
        kind,
        text: text.to_string(),
        old_line,
        new_line,
        no_newline_at_end: false,
    }
}

const HEADERS: &[u8] =
    b"diff --git a/x.txt b/x.txt\nindex 1111111..2222222 100644\n--- a/x.txt\n+++ b/x.txt\n";

// --- headers and hunks -----------------------------------------------------

#[test]
fn file_headers_before_the_first_hunk_are_skipped() {
    let mut output = HEADERS.to_vec();
    output.extend(b"@@ -1,3 +1,3 @@\n keep\n-old\n+new\n end\n");
    let hunks = text_hunks(&output);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].header, "@@ -1,3 +1,3 @@");
    let texts: Vec<_> = hunks[0].lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        texts,
        vec!["keep", "old", "new", "end"],
        "no header line may appear as a hunk line"
    );
}

#[test]
fn single_hunk_counts_old_and_new_line_numbers() {
    let mut output = HEADERS.to_vec();
    output.extend(b"@@ -1,3 +1,3 @@\n keep\n-old\n+new\n end\n");
    let hunks = text_hunks(&output);
    assert_eq!(
        hunks[0].lines,
        vec![
            line(DiffLineKind::Context, "keep", Some(1), Some(1)),
            line(DiffLineKind::Removed, "old", Some(2), None),
            line(DiffLineKind::Added, "new", None, Some(2)),
            line(DiffLineKind::Context, "end", Some(3), Some(3)),
        ]
    );
}

#[test]
fn multiple_hunks_each_start_counting_from_their_own_header() {
    let mut output = HEADERS.to_vec();
    output.extend(b"@@ -1,2 +1,2 @@\n-a\n+A\n b\n");
    output.extend(b"@@ -10,2 +12,3 @@\n c\n+inserted\n d\n");
    let hunks = text_hunks(&output);
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[1].header, "@@ -10,2 +12,3 @@");
    assert_eq!(
        hunks[1].lines,
        vec![
            line(DiffLineKind::Context, "c", Some(10), Some(12)),
            line(DiffLineKind::Added, "inserted", None, Some(13)),
            line(DiffLineKind::Context, "d", Some(11), Some(14)),
        ]
    );
    assert_eq!(
        hunks[0].lines[0],
        line(DiffLineKind::Removed, "a", Some(1), None)
    );
}

#[test]
fn hunk_header_without_counts_means_one_line() {
    let output = b"@@ -1 +1 @@\n-old\n+new\n";
    let hunks = text_hunks(output);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].header, "@@ -1 +1 @@");
    assert_eq!(
        hunks[0].lines,
        vec![
            line(DiffLineKind::Removed, "old", Some(1), None),
            line(DiffLineKind::Added, "new", None, Some(1)),
        ]
    );
}

#[test]
fn hunk_header_with_one_missing_count_uses_default_for_that_side_only() {
    let output = b"@@ -5 +7,2 @@\n-x\n+y\n+z\n";
    let hunks = text_hunks(output);
    assert_eq!(
        hunks[0].lines,
        vec![
            line(DiffLineKind::Removed, "x", Some(5), None),
            line(DiffLineKind::Added, "y", None, Some(7)),
            line(DiffLineKind::Added, "z", None, Some(8)),
        ]
    );
}

// --- line text ---------------------------------------------------------------

#[test]
fn prefixes_are_removed_and_only_one_character_is_taken() {
    let output = b"@@ -1,3 +1,3 @@\n  indented\n-  was\n+  now\n \n";
    let hunks = text_hunks(output);
    assert_eq!(
        hunks[0].lines[0].text, " indented",
        "only the one-character prefix goes"
    );
    assert_eq!(hunks[0].lines[1].text, "  was");
    assert_eq!(hunks[0].lines[2].text, "  now");
    assert_eq!(
        hunks[0].lines[3].text, "",
        "a context line for an empty line has no text"
    );
    assert_eq!(hunks[0].lines[3].kind, DiffLineKind::Context);
}

#[test]
fn no_newline_marker_sets_flag_on_previous_line_and_is_not_a_line() {
    let output =
        b"@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
    let hunks = text_hunks(output);
    assert_eq!(
        hunks[0].lines.len(),
        2,
        "the marker is not a line of its own"
    );
    assert!(hunks[0].lines[0].no_newline_at_end);
    assert_eq!(hunks[0].lines[0].text, "old");
    assert!(hunks[0].lines[1].no_newline_at_end);
    assert_eq!(hunks[0].lines[1].text, "new");
}

#[test]
fn no_newline_marker_after_context_line_sets_only_that_line() {
    let output = b"@@ -1,2 +1,2 @@\n keep\n-tail\n\\ No newline at end of file\n+tail2\n";
    let hunks = text_hunks(output);
    assert_eq!(hunks[0].lines.len(), 3);
    assert!(!hunks[0].lines[0].no_newline_at_end);
    assert!(hunks[0].lines[1].no_newline_at_end);
    assert!(!hunks[0].lines[2].no_newline_at_end);
}

// --- binary -------------------------------------------------------------------

#[test]
fn binary_files_line_yields_binary() {
    let mut output = b"diff --git a/x.bin b/x.bin\nindex 1111111..2222222 100644\n".to_vec();
    output.extend(b"Binary files a/x.bin and b/x.bin differ\n");
    assert_eq!(parse_diff(&output), Ok(FileDiff::Binary));
}

// --- empty and odd input ----------------------------------------------------

#[test]
fn empty_input_is_text_with_no_hunks() {
    assert_eq!(parse_diff(b""), Ok(FileDiff::Text { hunks: vec![] }));
}

#[test]
fn non_utf8_bytes_are_decoded_lossily_and_do_not_fail() {
    let output = b"@@ -1 +1 @@\n-caf\xe9\n+ok\n";
    let result = parse_diff(output);
    let hunks = match result {
        Ok(FileDiff::Text { hunks }) => hunks,
        other => panic!("non-UTF-8 bytes must not fail the parse, got {other:?}"),
    };
    assert_eq!(hunks[0].lines.len(), 2);
    assert_eq!(hunks[0].lines[0].kind, DiffLineKind::Removed);
    assert!(
        hunks[0].lines[0].text.starts_with("caf") && hunks[0].lines[0].text.contains('\u{FFFD}'),
        "invalid byte should become U+FFFD, got {:?}",
        hunks[0].lines[0].text
    );
    assert_eq!(
        hunks[0].lines[1],
        line(DiffLineKind::Added, "ok", None, Some(1))
    );
}

#[test]
fn unreadable_hunk_header_is_malformed() {
    for bad in [
        b"@@ garbage @@\n".as_slice(),
        b"@@ -x +1 @@\n".as_slice(),
        b"@@ -1,2 @@\n".as_slice(),
    ] {
        let result = parse_diff(bad);
        assert!(
            matches!(result, Err(ParseError::Malformed(_))),
            "header {:?} must be Malformed, got {result:?}",
            String::from_utf8_lossy(bad)
        );
    }
}

#[test]
fn mode_change_only_is_text_with_no_hunks() {
    let output = b"diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n";
    assert_eq!(parse_diff(output), Ok(FileDiff::Text { hunks: vec![] }));
}

#[test]
fn a_type_change_is_two_diffs_and_both_hunks_are_read() {
    // What git prints for a file that became a symlink: a deletion and an
    // addition of the same path, one after the other.
    let output = b"diff --git a/f b/f\ndeleted file mode 100644\nindex 1111111..0000000\n--- a/f\n+++ /dev/null\n@@ -1 +0,0 @@\n-content\ndiff --git a/f b/f\nnew file mode 120000\nindex 0000000..2222222\n--- /dev/null\n+++ b/f\n@@ -0,0 +1 @@\n+target\n\\ No newline at end of file\n";
    let hunks = text_hunks(output);
    assert_eq!(hunks.len(), 2);
    assert_eq!(
        hunks[0].lines,
        vec![line(DiffLineKind::Removed, "content", Some(1), None)]
    );
    let mut added = line(DiffLineKind::Added, "target", None, Some(1));
    added.no_newline_at_end = true;
    assert_eq!(hunks[1].lines, vec![added]);
}
