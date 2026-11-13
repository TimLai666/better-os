use git_core::{ConflictKind, Operation, count_conflict_markers, parse_status};

#[test]
fn operation_from_markers_prefers_rebase() {
    assert_eq!(
        Operation::from_markers(true, false, true, false),
        Some(Operation::Rebase)
    );
    assert_eq!(
        Operation::from_markers(false, true, false, false),
        Some(Operation::Merge)
    );
    assert_eq!(
        Operation::from_markers(false, false, true, false),
        Some(Operation::CherryPick)
    );
    assert_eq!(
        Operation::from_markers(false, false, false, true),
        Some(Operation::Revert)
    );
    assert_eq!(Operation::from_markers(false, false, false, false), None);
    assert_eq!(Operation::CherryPick.command(), "cherry-pick");
}

#[test]
fn counts_markers() {
    let text = b"a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\nb\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> b\n";
    assert_eq!(count_conflict_markers(text), 2);
    assert_eq!(count_conflict_markers(b"plain\n"), 0);
    assert_eq!(count_conflict_markers(b"<<<<<<< HEAD\nx\n=======\n"), 0);
    assert_eq!(
        count_conflict_markers(b"  <<<<<<< not at start\n=======\n>>>>>>>\n"),
        0
    );
}

#[test]
fn status_reads_conflict_kinds() {
    let zero = "0".repeat(40);
    let record = |xy: &str, path: &str| {
        format!("u {xy} N... 100644 100644 100644 100644 {zero} {zero} {zero} {path}\0")
    };
    let output = format!(
        "# branch.oid {zero}\0# branch.head main\0{}{}{}",
        record("UU", "both.txt"),
        record("DU", "deleted-by-us.txt"),
        record("UD", "deleted-by-them.txt")
    );
    let status = parse_status(output.as_bytes()).unwrap();
    let kinds: Vec<_> = status.changes.iter().map(|c| c.conflict).collect();
    assert_eq!(
        kinds,
        vec![
            Some(ConflictKind::BothModified),
            Some(ConflictKind::DeletedByUs),
            Some(ConflictKind::DeletedByThem)
        ]
    );
    assert!(ConflictKind::BothModified.has_markers());
    assert!(!ConflictKind::DeletedByUs.has_markers());
    let bad = format!("# branch.head main\0{}", record("XX", "x"));
    assert!(parse_status(bad.as_bytes()).is_err());
}

#[test]
fn status_reads_stash_count() {
    let status = parse_status(b"# branch.oid (initial)\0# branch.head main\0# stash 3\0").unwrap();
    assert_eq!(status.stash_count, 3);
}
