use git_core::tag::{MAX_TAG_LENGTH, TagProblem};
use git_core::{check_tag_name, parse_tags};

#[test]
fn reads_lightweight_and_annotated_tags() {
    let tags = parse_tags(b"v1\x1faaa\x1f\nv2\x1ftagobj\x1fccc\n").unwrap();
    assert_eq!(tags[0].commit, "aaa");
    assert_eq!(
        tags[1].commit, "ccc",
        "an annotated tag points at its commit"
    );
    assert!(parse_tags(b"v1\x1faaa\n").is_err());
    assert_eq!(check_tag_name("", &tags), Some(TagProblem::Empty));
    assert_eq!(check_tag_name("v1", &tags), Some(TagProblem::Exists));
    assert_eq!(
        check_tag_name(&"x".repeat(MAX_TAG_LENGTH + 1), &tags),
        Some(TagProblem::TooLong)
    );
    assert_eq!(check_tag_name("v3", &tags), None);
}
