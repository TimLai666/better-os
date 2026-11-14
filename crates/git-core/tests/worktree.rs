use std::path::PathBuf;

use git_core::parse_worktrees;

#[test]
fn reads_worktrees() {
    let output = b"worktree /repo\0HEAD aaa\0branch refs/heads/main\0\0worktree /repo-wt\0HEAD bbb\0detached\0locked\0\0worktree /gone\0HEAD ccc\0branch refs/heads/old\0prunable gitdir file points to non-existent location\0\0";
    let worktrees = parse_worktrees(output).unwrap();
    assert_eq!(worktrees.len(), 3);
    assert!(worktrees[0].main && !worktrees[1].main);
    assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
    assert_eq!(worktrees[1].path, PathBuf::from("/repo-wt"));
    assert!(worktrees[1].detached && worktrees[1].locked);
    assert_eq!(worktrees[1].lock_reason, None, "locked with no reason");
    let reasoned =
        parse_worktrees(b"worktree /r\0HEAD a\0\0worktree /w\0HEAD b\0locked on a USB stick\0\0")
            .unwrap();
    assert!(reasoned[1].locked);
    assert_eq!(reasoned[1].lock_reason.as_deref(), Some("on a USB stick"));
    assert!(worktrees[2].prunable);
    assert!(parse_worktrees(b"HEAD aaa\0").is_err());
    assert!(parse_worktrees(b"").unwrap().is_empty());
}
