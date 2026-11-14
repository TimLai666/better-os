//! The branch graph's records, labels, and search, against git's formats.

use git_core::graph::{find_next, find_previous, matches};
use git_core::{Commit, RecordReader, RefKind, RefLabel, ref_labels};

fn record(sha: &str, parents: &str, refs: &str, summary: &str) -> Vec<u8> {
    let mut out = [
        sha,
        parents,
        "Ada",
        "ada@example.com",
        "1700000000",
        refs,
        summary,
        "",
    ]
    .join("\u{1f}")
    .into_bytes();
    out.push(b'\n');
    out.push(0);
    out
}

fn labels(refs: &[&str]) -> Vec<RefLabel> {
    ref_labels(&refs.iter().map(|r| r.to_string()).collect::<Vec<_>>())
}

fn label(kind: RefKind, name: &str, current: bool) -> RefLabel {
    RefLabel {
        kind,
        name: name.to_string(),
        current,
    }
}

#[test]
fn full_decorations_become_labels_current_branch_first() {
    assert_eq!(
        labels(&[
            "tag: refs/tags/v1.0",
            "refs/remotes/origin/main",
            "HEAD -> refs/heads/main",
            "refs/heads/release/1",
            "refs/remotes/origin/HEAD",
        ]),
        vec![
            label(RefKind::Local, "main", true),
            label(RefKind::Local, "release/1", false),
            label(RefKind::Remote, "origin/main", false),
            label(RefKind::Tag, "v1.0", false),
        ]
    );
}

#[test]
fn a_detached_head_is_its_own_label_and_other_namespaces_carry_none() {
    assert_eq!(
        labels(&[
            "HEAD",
            "refs/stash",
            "refs/notes/commits",
            "refs/pull/1/head",
            "refs/heads/x"
        ]),
        vec![
            label(RefKind::Head, "HEAD", false),
            label(RefKind::Local, "x", false),
        ]
    );
    assert!(labels(&[]).is_empty());
}

#[test]
fn a_branch_named_like_a_remote_is_still_local() {
    // Short decorations could not tell these apart; full ones can.
    assert_eq!(
        labels(&["refs/heads/origin/main"]),
        vec![label(RefKind::Local, "origin/main", false)]
    );
}

#[test]
fn records_split_across_reads_come_out_whole_and_in_order() {
    let a = "a".repeat(40);
    let b = "b".repeat(40);
    let mut bytes = record(&a, &b, "HEAD -> refs/heads/main", "Second");
    bytes.extend(record(&b, "", "", "First"));
    // Every split point, including inside the separator and the NUL.
    for cut in 0..bytes.len() {
        let mut reader = RecordReader::new();
        let mut commits = reader.feed(&bytes[..cut]).unwrap();
        commits.extend(reader.feed(&bytes[cut..]).unwrap());
        assert!(reader.finish().unwrap().is_none());
        let shas: Vec<&str> = commits.iter().map(|c| c.sha.as_str()).collect();
        assert_eq!(shas, [a.as_str(), b.as_str()], "cut at {cut}");
        assert_eq!(commits[0].parents, std::slice::from_ref(&b));
        assert_eq!(commits[0].refs, ["HEAD -> refs/heads/main"]);
    }
}

#[test]
fn a_record_left_without_its_nul_is_read_at_the_end() {
    let mut bytes = record("c", "", "", "Last");
    bytes.pop();
    let mut reader = RecordReader::new();
    assert!(reader.feed(&bytes).unwrap().is_empty());
    assert_eq!(reader.finish().unwrap().unwrap().summary, "Last");
}

#[test]
fn a_malformed_record_is_an_error() {
    let mut reader = RecordReader::new();
    assert!(reader.feed(b"only\x1ftwo\0").is_err());
}

fn commit(sha: &str, summary: &str, author: &str) -> Commit {
    Commit {
        sha: sha.to_string(),
        parents: vec![],
        author_name: author.to_string(),
        author_email: String::new(),
        author_time: 0,
        refs: vec![],
        summary: summary.to_string(),
        body: String::new(),
    }
}

#[test]
fn search_matches_subject_author_and_hash_prefix() {
    let c = commit("abcdef0123", "Fix the Parser", "Grace Hopper");
    assert!(matches(&c, "parser"));
    assert!(matches(&c, "  PARSER "));
    assert!(matches(&c, "hopper"));
    assert!(matches(&c, "abcd"));
    assert!(matches(&c, "ABCDEF"));
    assert!(!matches(&c, "abc"), "three digits are too short for a hash");
    assert!(!matches(&c, "bcde"), "a hash matches from its start only");
    assert!(!matches(&c, ""));
    assert!(!matches(&c, "   "));
}

#[test]
fn find_steps_forward_and_back_and_wraps_once() {
    let list = vec![
        commit("1111", "alpha", "x"),
        commit("2222", "beta", "x"),
        commit("3333", "alpha two", "x"),
        commit("4444", "gamma", "x"),
    ];
    assert_eq!(find_next(&list, "alpha", None), Some(0));
    assert_eq!(find_next(&list, "alpha", Some(0)), Some(2));
    assert_eq!(
        find_next(&list, "alpha", Some(2)),
        Some(0),
        "wraps to the top"
    );
    assert_eq!(find_previous(&list, "alpha", Some(2)), Some(0));
    assert_eq!(
        find_previous(&list, "alpha", Some(0)),
        Some(2),
        "wraps to the bottom"
    );
    assert_eq!(find_previous(&list, "gamma", None), Some(3));
    assert_eq!(find_next(&list, "delta", None), None);
    assert_eq!(find_next(&[], "alpha", None), None);
}
