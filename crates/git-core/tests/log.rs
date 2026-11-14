//! `parse_log` and `parse_name_status` against git's formats.

use std::path::PathBuf;

use git_core::{ChangeKind, parse_log, parse_name_status};

fn record(fields: &[&str]) -> Vec<u8> {
    let mut out = fields.join("\u{1f}").into_bytes();
    out.push(b'\n');
    out.push(0);
    out
}

#[test]
fn reads_commits_with_parents_refs_and_body() {
    let mut output = record(&[
        "a".repeat(40).as_str(),
        &format!("{} {}", "b".repeat(40), "c".repeat(40)),
        "Ada",
        "ada@example.com",
        "1700000000",
        "HEAD -> main, tag: v1.0, origin/main",
        "Merge things",
        "Body line\n\nCo-authored-by: Bob <bob@example.com>\n",
    ]);
    output.extend(record(&[
        "d".repeat(40).as_str(),
        "",
        "Root",
        "root@example.com",
        "1600000000",
        "",
        "First",
        "",
    ]));
    let commits = parse_log(&output).unwrap();
    assert_eq!(commits.len(), 2);
    let merge = &commits[0];
    assert!(merge.is_merge());
    assert_eq!(merge.short_sha(), "aaaaaaa");
    assert_eq!(merge.tags(), vec!["v1.0"]);
    assert_eq!(merge.refs.len(), 3);
    assert_eq!(merge.summary, "Merge things");
    assert_eq!(
        merge.co_authors(),
        vec![("Bob".to_string(), "bob@example.com".to_string())]
    );
    assert!(commits[1].parents.is_empty());
    assert_eq!(commits[1].body, "");
    assert_eq!(commits[1].author_time, 1_600_000_000);
}

#[test]
fn empty_log_is_no_commits_and_short_record_fails() {
    assert!(parse_log(b"").unwrap().is_empty());
    assert!(parse_log(b"abc\x1fdef\0").is_err());
    let bad_time = record(&["a", "", "n", "e", "soon", "", "s", ""]);
    assert!(parse_log(&bad_time).is_err());
    // git prints no time at all for a date it cannot read.
    let no_time = record(&["a", "", "n", "e", "", "", "s", ""]);
    assert_eq!(parse_log(&no_time).unwrap()[0].author_time, 0);
}

#[test]
fn body_keeps_a_separator_it_contains() {
    let output = record(&["a", "", "n", "e", "1", "", "s", "one\u{1f}two"]);
    assert_eq!(parse_log(&output).unwrap()[0].body, "one\u{1f}two");
}

#[test]
fn name_status_reads_renames_and_odd_names() {
    let output = b"M\0src/a.rs\0R100\0old name.txt\0new name.txt\0A\0-dash\0D\0\xe4\xb8\xad.txt\0";
    let files = parse_name_status(output).unwrap();
    assert_eq!(files.len(), 4);
    assert_eq!(files[0].kind, ChangeKind::Modified);
    assert_eq!(files[1].kind, ChangeKind::Renamed);
    assert_eq!(files[1].path, PathBuf::from("new name.txt"));
    assert_eq!(files[1].original_path, Some(PathBuf::from("old name.txt")));
    assert_eq!(files[2].path, PathBuf::from("-dash"));
    assert_eq!(files[3].path, PathBuf::from("中.txt"));
    assert!(parse_name_status(b"").unwrap().is_empty());
    assert!(parse_name_status(b"X\0a\0").is_err());
    assert!(parse_name_status(b"M\0").is_err());
}

#[test]
fn co_author_trailer_parsing() {
    use git_core::log::parse_co_author;
    assert_eq!(
        parse_co_author("co-authored-by: A B <a@b.c>"),
        Some(("A B".into(), "a@b.c".into()))
    );
    assert_eq!(parse_co_author("Signed-off-by: A <a@b.c>"), None);
    assert_eq!(parse_co_author("Co-authored-by: <a@b.c>"), None);
    assert_eq!(parse_co_author("Co-authored-by: A"), None);
}
