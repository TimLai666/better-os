//! Partial patches built from recorded diffs. `git-platform`'s tests apply
//! the same kind of patch with real `git apply`.

use git_core::{PatchMode, RawDiff};

const MODIFIED: &[u8] = b"diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,4 @@ fn x\n one\n-two\n+TWO\n three\n-four\n+FOUR\n@@ -10,2 +10,3 @@\n ten\n+ten-and-half\n eleven\n";

#[test]
fn to_index_keeps_only_selected_changes() {
    let raw = RawDiff::parse(MODIFIED).unwrap();
    assert_eq!(raw.shape(), vec![6, 3]);
    // Select only "-two" and "+TWO" in the first hunk.
    let patch = raw
        .partial(|h, l| h == 0 && (l == 1 || l == 2), PatchMode::ToIndex)
        .unwrap();
    let text = String::from_utf8(patch).unwrap();
    assert!(text.starts_with("diff --git a/f.txt b/f.txt\n"));
    assert!(text.contains("@@ -1,4 +1,4 @@ fn x\n one\n-two\n+TWO\n three\n four\n"));
    assert!(!text.contains("FOUR"), "an unselected addition is left out");
    assert!(
        !text.contains("ten"),
        "a hunk with nothing selected is dropped"
    );
}

#[test]
fn to_index_moves_later_hunk_starts() {
    let raw = RawDiff::parse(MODIFIED).unwrap();
    // Select "-four" only (removal), and the addition in hunk 2.
    let patch = raw
        .partial(
            |h, l| (h == 0 && l == 4) || (h == 1 && l == 1),
            PatchMode::ToIndex,
        )
        .unwrap();
    let text = String::from_utf8(patch).unwrap();
    assert!(text.contains("@@ -1,4 +1,3 @@ fn x\n one\n two\n three\n-four\n"));
    assert!(text.contains("@@ -10,2 +9,3 @@\n ten\n+ten-and-half\n eleven\n"));
}

#[test]
fn discard_keeps_unselected_additions_as_context() {
    let raw = RawDiff::parse(MODIFIED).unwrap();
    let patch = raw
        .partial(|h, l| h == 0 && l == 2, PatchMode::Discard)
        .unwrap();
    let text = String::from_utf8(patch).unwrap();
    // "+TWO" selected; "-two" and "-four" unselected are left out; "+FOUR"
    // stays as context, because it is in the file.
    assert!(
        text.contains("@@ -1,3 +1,4 @@ fn x\n one\n+TWO\n three\n FOUR\n"),
        "{text}"
    );
}

#[test]
fn nothing_selected_is_none() {
    let raw = RawDiff::parse(MODIFIED).unwrap();
    assert!(raw.partial(|_, _| false, PatchMode::ToIndex).is_none());
    // Context lines alone are not a change.
    assert!(raw.partial(|_, l| l == 0, PatchMode::ToIndex).is_none());
}

#[test]
fn partial_deletion_becomes_a_modification() {
    let deleted = b"diff --git a/d.txt b/d.txt\ndeleted file mode 100644\nindex 1111111..0000000\n--- a/d.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n-b\n";
    let raw = RawDiff::parse(deleted).unwrap();
    let text = String::from_utf8(raw.partial(|_, l| l == 0, PatchMode::ToIndex).unwrap()).unwrap();
    assert!(!text.contains("deleted file mode"));
    assert!(text.contains("+++ b/d.txt"));
    assert!(text.contains("@@ -1,2 +1,1 @@\n-a\n b\n"), "{text}");
    let whole = String::from_utf8(raw.partial(|_, _| true, PatchMode::ToIndex).unwrap()).unwrap();
    assert!(whole.contains("deleted file mode") && whole.contains("+++ /dev/null"));
}

#[test]
fn partial_new_file_for_index_and_discard() {
    let added = b"diff --git a/n.txt b/n.txt\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,3 @@\n+a\n+b\n+c\n\\ No newline at end of file\n";
    let raw = RawDiff::parse(added).unwrap();
    let index = String::from_utf8(raw.partial(|_, l| l != 1, PatchMode::ToIndex).unwrap()).unwrap();
    assert!(index.contains("new file mode"));
    assert!(
        index.contains("@@ -0,0 +1,2 @@\n+a\n+c\n\\ No newline at end of file\n"),
        "{index}"
    );
    let discard =
        String::from_utf8(raw.partial(|_, l| l == 1, PatchMode::Discard).unwrap()).unwrap();
    assert!(!discard.contains("new file mode"));
    assert!(discard.contains("--- a/n.txt"));
    assert!(
        discard.contains("@@ -1,2 +1,3 @@\n a\n+b\n c\n"),
        "{discard}"
    );
}

#[test]
fn bad_header_fails() {
    assert!(RawDiff::parse(b"@@ nonsense @@\n").is_err());
}
