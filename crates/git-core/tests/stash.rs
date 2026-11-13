use git_core::parse_stashes;
use git_core::stash::{stash_for_branch, stash_message};

#[test]
fn reads_stashes_and_finds_the_branchs_own() {
    let output = format!(
        "stash@{{0}}\x1f{}\x1f1700000000\x1fOn feature: {}\0stash@{{1}}\x1f{}\x1f1600000000\x1fOn main: !!GitHub_Desktop<main>\0stash@{{2}}\x1fccc\x1f1\x1fWIP on main: 123 tip\0",
        "a".repeat(40),
        stash_message("feature"),
        "b".repeat(40)
    );
    let stashes = parse_stashes(output.as_bytes()).unwrap();
    assert_eq!(stashes.len(), 3);
    assert_eq!(stashes[0].branch(), Some("feature"));
    assert_eq!(stashes[1].branch(), Some("main"));
    assert_eq!(stashes[2].branch(), None);
    assert_eq!(
        stash_for_branch(&stashes, "main").unwrap().name,
        "stash@{1}"
    );
    assert!(stash_for_branch(&stashes, "other").is_none());
    assert!(parse_stashes(b"").unwrap().is_empty());
    assert!(parse_stashes(b"stash@{0}\x1fa\0").is_err());
}
