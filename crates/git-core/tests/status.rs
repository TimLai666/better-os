//! Tests for `git_core::parse_status`, against the porcelain v2 `-z` output
//! git prints for `git status --porcelain=v2 -z --branch --untracked-files=all`.
//!
//! The samples were captured from git 2.43 in a temporary repository and then
//! written by hand. Records end with NUL. In a `2` record the new path comes
//! first and the original path is the next NUL-separated field.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use git_core::{BranchHead, ChangeKind, FileChange, ParseError, RepositoryStatus, parse_status};

/// A 40-character object id, as git prints for an absent object.
const ZERO_OID: &str = "0000000000000000000000000000000000000000";

/// A `1` record: an ordinary changed entry. `xy` is the two-letter status.
fn ordinary(xy: &str, path: &[u8]) -> Vec<u8> {
    let mut out = format!("1 {xy} N... 100644 100644 100644 {ZERO_OID} {ZERO_OID} ").into_bytes();
    out.extend_from_slice(path);
    out.push(0);
    out
}

/// The `# branch.head main` header, then the given records.
fn head_then(records: Vec<u8>) -> Vec<u8> {
    let mut out = b"# branch.head main\0".to_vec();
    out.extend(records);
    out
}

fn parse_ok(output: &[u8]) -> RepositoryStatus {
    match parse_status(output) {
        Ok(status) => status,
        Err(error) => panic!("expected Ok, got {error:?}"),
    }
}

/// A change with no original path, not untracked, not conflicted.
fn plain(path: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileChange {
    FileChange {
        path: PathBuf::from(path),
        original_path: None,
        staged,
        unstaged,
        untracked: false,
        conflicted: false,
    }
}

// --- branch header ---------------------------------------------------------

#[test]
fn branch_head_names_the_branch() {
    let output = b"# branch.oid 1111111111111111111111111111111111111111\0# branch.head main\0";
    let status = parse_ok(output);
    assert_eq!(status.head, BranchHead::Branch("main".to_string()));
    assert!(!status.unborn);
    assert_eq!(status.upstream, None);
    assert_eq!(status.ahead_behind, None);
    assert!(status.changes.is_empty());
}

#[test]
fn branch_head_detached_has_no_name() {
    let output =
        b"# branch.oid 1111111111111111111111111111111111111111\0# branch.head (detached)\0";
    let status = parse_ok(output);
    assert_eq!(status.head, BranchHead::Detached);
    assert!(!status.unborn);
}

#[test]
fn branch_oid_initial_marks_repository_unborn() {
    let output = b"# branch.oid (initial)\0# branch.head main\0";
    let status = parse_ok(output);
    assert!(status.unborn);
    assert_eq!(status.head, BranchHead::Branch("main".to_string()));
    assert!(status.changes.is_empty());
}

#[test]
fn upstream_and_ahead_behind_are_read() {
    let output = b"# branch.oid 1111111111111111111111111111111111111111\0\
# branch.head feature\0# branch.upstream origin/feature\0# branch.ab +2 -3\0";
    let status = parse_ok(output);
    assert_eq!(status.upstream, Some("origin/feature".to_string()));
    assert_eq!(status.ahead_behind, Some((2, 3)));
}

#[test]
fn upstream_without_ab_leaves_ahead_behind_empty() {
    let output = b"# branch.oid 1111111111111111111111111111111111111111\0\
# branch.head feature\0# branch.upstream origin/feature\0";
    let status = parse_ok(output);
    assert_eq!(status.upstream, Some("origin/feature".to_string()));
    assert_eq!(status.ahead_behind, None);
}

#[test]
fn no_upstream_leaves_both_optional_fields_empty() {
    let output = b"# branch.oid 1111111111111111111111111111111111111111\0# branch.head main\0";
    let status = parse_ok(output);
    assert_eq!(status.upstream, None);
    assert_eq!(status.ahead_behind, None);
}

// --- `1` records: XY combinations ------------------------------------------

#[test]
fn xy_modified_in_index_only_is_staged_modified() {
    let status = parse_ok(&head_then(ordinary("M.", b"a.txt")));
    assert_eq!(
        status.changes,
        vec![plain("a.txt", Some(ChangeKind::Modified), None)]
    );
}

#[test]
fn xy_modified_in_worktree_only_is_unstaged_modified() {
    let status = parse_ok(&head_then(ordinary(".M", b"a.txt")));
    assert_eq!(
        status.changes,
        vec![plain("a.txt", None, Some(ChangeKind::Modified))]
    );
}

#[test]
fn xy_modified_on_both_sides_sets_both() {
    let status = parse_ok(&head_then(ordinary("MM", b"a.txt")));
    assert_eq!(
        status.changes,
        vec![plain(
            "a.txt",
            Some(ChangeKind::Modified),
            Some(ChangeKind::Modified)
        )]
    );
}

#[test]
fn xy_added_in_index_is_staged_added() {
    let status = parse_ok(&head_then(ordinary("A.", b"new.txt")));
    assert_eq!(
        status.changes,
        vec![plain("new.txt", Some(ChangeKind::Added), None)]
    );
}

#[test]
fn xy_deleted_in_worktree_is_unstaged_deleted() {
    let status = parse_ok(&head_then(ordinary(".D", b"gone.txt")));
    assert_eq!(
        status.changes,
        vec![plain("gone.txt", None, Some(ChangeKind::Deleted))]
    );
}

#[test]
fn xy_deleted_in_index_is_staged_deleted() {
    let status = parse_ok(&head_then(ordinary("D.", b"gone.txt")));
    assert_eq!(
        status.changes,
        vec![plain("gone.txt", Some(ChangeKind::Deleted), None)]
    );
}

#[test]
fn xy_type_changed_in_index_is_staged_type_changed() {
    let status = parse_ok(&head_then(ordinary("T.", b"link")));
    assert_eq!(
        status.changes,
        vec![plain("link", Some(ChangeKind::TypeChanged), None)]
    );
}

#[test]
fn xy_type_changed_in_worktree_is_unstaged_type_changed() {
    let status = parse_ok(&head_then(ordinary(".T", b"link")));
    assert_eq!(
        status.changes,
        vec![plain("link", None, Some(ChangeKind::TypeChanged))]
    );
}

// --- `2` records: rename and copy -------------------------------------------

#[test]
fn rename_record_carries_the_original_path_from_next_field() {
    let output = b"# branch.head main\0\
2 R. N... 100644 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 R100 new.txt\0old.txt\0";
    let status = parse_ok(output);
    assert_eq!(
        status.changes,
        vec![FileChange {
            path: PathBuf::from("new.txt"),
            original_path: Some(PathBuf::from("old.txt")),
            staged: Some(ChangeKind::Renamed),
            unstaged: None,
            untracked: false,
            conflicted: false,
        }]
    );
}

#[test]
fn rename_with_spaces_in_both_paths_keeps_both_whole() {
    let output = b"# branch.head main\0\
2 R. N... 100644 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 R95 new name.txt\0old name.txt\0";
    let status = parse_ok(output);
    assert_eq!(
        status.changes,
        vec![FileChange {
            path: PathBuf::from("new name.txt"),
            original_path: Some(PathBuf::from("old name.txt")),
            staged: Some(ChangeKind::Renamed),
            unstaged: None,
            untracked: false,
            conflicted: false,
        }]
    );
}

#[test]
fn rename_staged_and_then_modified_sets_both_sides() {
    let output = b"# branch.head main\0\
2 RM N... 100644 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 R100 new.txt\0old.txt\0";
    let status = parse_ok(output);
    assert_eq!(
        status.changes,
        vec![FileChange {
            path: PathBuf::from("new.txt"),
            original_path: Some(PathBuf::from("old.txt")),
            staged: Some(ChangeKind::Renamed),
            unstaged: Some(ChangeKind::Modified),
            untracked: false,
            conflicted: false,
        }]
    );
}

#[test]
fn copy_record_is_copied_with_original_path() {
    let output = b"# branch.head main\0\
2 C. N... 100644 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 C75 copy.txt\0source.txt\0";
    let status = parse_ok(output);
    assert_eq!(
        status.changes,
        vec![FileChange {
            path: PathBuf::from("copy.txt"),
            original_path: Some(PathBuf::from("source.txt")),
            staged: Some(ChangeKind::Copied),
            unstaged: None,
            untracked: false,
            conflicted: false,
        }]
    );
}

// --- `u`, `?`, `!` records ----------------------------------------------------

#[test]
fn unmerged_record_is_conflicted_with_neither_side_set() {
    let output = b"# branch.head main\0\
u UU N... 100644 100644 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 conflict.txt\0";
    let status = parse_ok(output);
    assert_eq!(
        status.changes,
        vec![FileChange {
            path: PathBuf::from("conflict.txt"),
            original_path: None,
            staged: None,
            unstaged: None,
            untracked: false,
            conflicted: true,
        }]
    );
}

#[test]
fn untracked_record_is_untracked_with_neither_side_set() {
    let output = b"# branch.head main\0? scratch.txt\0";
    let status = parse_ok(output);
    assert_eq!(
        status.changes,
        vec![FileChange {
            path: PathBuf::from("scratch.txt"),
            original_path: None,
            staged: None,
            unstaged: None,
            untracked: true,
            conflicted: false,
        }]
    );
}

#[test]
fn ignored_record_is_skipped() {
    let output = b"# branch.head main\0! target/debug.o\0? notes.txt\0";
    let status = parse_ok(output);
    assert_eq!(
        status.changes.len(),
        1,
        "only the untracked file should remain"
    );
    assert_eq!(status.changes[0].path, PathBuf::from("notes.txt"));
    assert!(status.changes[0].untracked);
}

#[test]
fn records_keep_the_order_git_printed_them_in() {
    let mut output = b"# branch.head main\0".to_vec();
    output.extend(ordinary(".M", b"zeta.txt"));
    output.extend(b"? alpha.txt\0");
    output.extend(ordinary("A.", b"mid.txt"));
    let status = parse_ok(&output);
    let paths: Vec<_> = status.changes.iter().map(|c| c.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("zeta.txt"),
            PathBuf::from("alpha.txt"),
            PathBuf::from("mid.txt")
        ]
    );
}

// --- path bytes -----------------------------------------------------------------

#[test]
fn path_with_spaces_non_ascii_and_leading_dash_is_kept_whole() {
    let mut output = b"# branch.head main\0".to_vec();
    output.extend(ordinary(".M", "sp ace.txt".as_bytes()));
    output.extend(ordinary(".M", "中文 檔名.txt".as_bytes()));
    output.extend(ordinary("A.", b"-dash.txt"));
    let status = parse_ok(&output);
    assert_eq!(
        status.changes,
        vec![
            plain("sp ace.txt", None, Some(ChangeKind::Modified)),
            plain("中文 檔名.txt", None, Some(ChangeKind::Modified)),
            plain("-dash.txt", Some(ChangeKind::Added), None),
        ]
    );
}

#[test]
fn non_utf8_path_bytes_are_kept_raw() {
    let mut output = b"# branch.head main\0".to_vec();
    output.extend(ordinary(".M", b"bad\xff\xfe.txt"));
    let status = parse_ok(&output);
    assert_eq!(status.changes.len(), 1);
    assert_eq!(
        status.changes[0].path.as_os_str(),
        OsStr::from_bytes(b"bad\xff\xfe.txt")
    );
    assert_eq!(status.changes[0].unstaged, Some(ChangeKind::Modified));
}

// --- malformed input --------------------------------------------------------

#[test]
fn empty_input_is_malformed() {
    let result = parse_status(b"");
    assert!(
        matches!(result, Err(ParseError::Malformed(_))),
        "empty output must be Malformed, got {result:?}"
    );
}

#[test]
fn unknown_record_type_is_malformed_and_names_it() {
    let result = parse_status(b"# branch.head main\0z mystery entry\0");
    match result {
        Err(ParseError::Malformed(detail)) => {
            assert!(
                detail.contains('z'),
                "detail should name the record type, got {detail:?}"
            );
        }
        other => panic!("expected Malformed naming `z`, got {other:?}"),
    }
}

#[test]
fn ordinary_record_with_too_few_fields_is_malformed() {
    let output = [b"# branch.head main\0".as_slice(), b"1 .M N... 100644\0"].concat();
    let result = parse_status(&output);
    assert!(
        matches!(result, Err(ParseError::Malformed(_))),
        "short `1` record must be Malformed, got {result:?}"
    );
}

#[test]
fn rename_without_original_path_field_is_malformed() {
    let output = b"# branch.head main\0\
2 R. N... 100644 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 R100 new.txt\0";
    let result = parse_status(output);
    assert!(
        matches!(result, Err(ParseError::Malformed(_))),
        "a `2` record with no original path must be Malformed, got {result:?}"
    );
}

#[test]
fn unmerged_record_with_too_few_fields_is_malformed() {
    let result = parse_status(b"# branch.head main\0u UU N... 100644\0");
    assert!(
        matches!(result, Err(ParseError::Malformed(_))),
        "short `u` record must be Malformed, got {result:?}"
    );
}
