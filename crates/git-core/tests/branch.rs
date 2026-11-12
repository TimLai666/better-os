//! `parse_branches` and the branch-name rules.

use git_core::branch::default_branch;
use git_core::{NameProblem, check_branch_name, parse_branches, sanitize_ref_name};

const SAMPLE: &str = "refs/heads/main\x1faaa\x1frefs/remotes/origin/main\x1fahead 2, behind 1\x1f*\x1f1700000000\x1fTip\n\
refs/heads/feature\x1fbbb\x1frefs/remotes/origin/feature\x1fgone\x1f \x1f1600000000\x1fWork\n\
refs/heads/local\x1fccc\x1f\x1f\x1f \x1f1500000000\x1fLocal only\n\
refs/remotes/origin/HEAD\x1faaa\x1f\x1f\x1f \x1f1700000000\x1fTip\n\
refs/remotes/origin/main\x1faaa\x1f\x1f\x1f \x1f1700000000\x1fTip\n";

#[test]
fn reads_local_and_remote_branches() {
    let branches = parse_branches(SAMPLE.as_bytes()).unwrap();
    assert_eq!(branches.len(), 4, "origin/HEAD is skipped");
    let main = &branches[0];
    assert!(main.current && !main.remote);
    assert_eq!((main.ahead, main.behind), (2, 1));
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert!(branches[1].upstream_gone);
    assert_eq!(branches[2].upstream, None);
    let remote = &branches[3];
    assert!(remote.remote);
    assert_eq!(remote.remote_name(), Some("origin"));
    assert_eq!(remote.name_on_remote(), "main");
    assert!(parse_branches(b"refs/heads/x\x1fa\n").is_err());
}

#[test]
fn sanitizes_names_like_desktop() {
    assert_eq!(sanitize_ref_name("my feature"), "my-feature");
    assert_eq!(sanitize_ref_name("  a  b  "), "a-b");
    assert_eq!(sanitize_ref_name("fix:bug?*"), "fix-bug-");
    assert_eq!(sanitize_ref_name("a..b"), "a-b");
    assert_eq!(sanitize_ref_name("-lead"), "lead");
    assert_eq!(sanitize_ref_name("name.lock"), "name");
    assert_eq!(sanitize_ref_name("dir/"), "dir");
    assert_eq!(sanitize_ref_name("x@{y"), "x-y");
    assert_eq!(sanitize_ref_name("功能/新"), "功能/新");
    assert_eq!(sanitize_ref_name("..."), "");
}

#[test]
fn checks_names_against_existing() {
    let branches = parse_branches(SAMPLE.as_bytes()).unwrap();
    assert_eq!(check_branch_name("", &branches), Some(NameProblem::Empty));
    assert_eq!(
        check_branch_name("HEAD", &branches),
        Some(NameProblem::Reserved)
    );
    assert_eq!(
        check_branch_name("main", &branches),
        Some(NameProblem::Exists)
    );
    assert_eq!(check_branch_name("new", &branches), None);
}

#[test]
fn picks_the_default_branch() {
    let branches = parse_branches(SAMPLE.as_bytes()).unwrap();
    assert_eq!(
        default_branch(&branches, Some("origin/feature"))
            .unwrap()
            .name,
        "feature"
    );
    assert_eq!(default_branch(&branches, None).unwrap().name, "main");
}
