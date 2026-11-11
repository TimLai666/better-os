//! Tests for `git_platform::SystemGit` against a real `git`.
//!
//! Every test builds its own repository in a temporary directory. The `git`
//! calls that build test data, and the `SystemGit` calls under test, all run
//! with `HOME` pointed at the temporary directory, the global and system
//! configuration switched off, and a fixed commit identity. The environment is
//! process-wide, so the tests take `SERIAL` and run one at a time.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use git_core::{BranchHead, ChangeKind, DiffLineKind, FileChange, FileDiff, RepositoryStatus};
use git_platform::{CommitMessage, DiffSide, Git, GitError, SystemGit};
use tempfile::TempDir;

static SERIAL: Mutex<()> = Mutex::new(());

const AUTHOR_NAME: &str = "Better Git Test";
const AUTHOR_EMAIL: &str = "test@example.invalid";

/// A temporary home, a repository in it on branch `main`, and a plain folder
/// that is not a repository.
struct Sandbox {
    _serial: MutexGuard<'static, ()>,
    _tmp: TempDir,
    home: PathBuf,
    repo: PathBuf,
    plain: PathBuf,
}

fn sandbox() -> Sandbox {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let tmp = TempDir::new().expect("create temporary directory");
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    let plain = tmp.path().join("plain");
    for dir in [&home, &repo, &plain] {
        fs::create_dir_all(dir).expect("create sandbox directory");
    }
    isolate_environment(&home);
    let sandbox = Sandbox {
        _serial: serial,
        _tmp: tmp,
        home,
        repo,
        plain,
    };
    git(&sandbox, &["init", "-q", "-b", "main"]);
    sandbox
}

/// Points this process's `HOME` and git configuration away from the person's
/// own, and sets the commit identity, so `SystemGit` commits with test values.
fn isolate_environment(home: &Path) {
    // SAFETY: every test in this file holds `SERIAL` while it runs, so no other
    // thread reads or writes the environment at the same time.
    unsafe {
        std::env::set_var("HOME", home);
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_AUTHOR_NAME", AUTHOR_NAME);
        std::env::set_var("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL);
        std::env::set_var("GIT_COMMITTER_NAME", AUTHOR_NAME);
        std::env::set_var("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL);
    }
}

/// A `git -C <repo>` command with the same isolation set on the command itself.
fn git_command(sandbox: &Sandbox, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(&sandbox.repo)
        .args(args)
        .env("HOME", &sandbox.home)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL)
        .env("GIT_COMMITTER_NAME", AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL);
    command
}

/// Runs git to build test data and returns its stdout. Fails the test if git does.
fn git(sandbox: &Sandbox, args: &[&str]) -> String {
    let output = git_command(sandbox, args).output().expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Writes a file under the repository, creating folders as needed.
fn write(sandbox: &Sandbox, relative: &str, contents: &[u8]) {
    let path = sandbox.repo.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent folder");
    }
    fs::write(path, contents).expect("write test file");
}

/// Stages the given files with git and makes the first commit.
fn commit_baseline(sandbox: &Sandbox, files: &[&str]) {
    let mut args = vec!["add", "--"];
    args.extend_from_slice(files);
    git(sandbox, &args);
    git(sandbox, &["commit", "-q", "-m", "baseline"]);
}

/// The files every status and diff test starts from, committed on `main`.
/// Their names cover spaces, non-ASCII characters and a leading dash.
const BASELINE: &[&str] = &[
    "modify.txt",
    "delete.txt",
    "staged-delete.txt",
    "rename-src.txt",
    "other.txt",
    "sp ace.txt",
    "中文.txt",
    "-dash.txt",
];

fn commit_standard_baseline(sandbox: &Sandbox) {
    for name in BASELINE {
        write(sandbox, name, format!("base {name}\n").as_bytes());
    }
    commit_baseline(sandbox, BASELINE);
}

fn status_of(sandbox: &Sandbox) -> RepositoryStatus {
    SystemGit
        .status(&sandbox.repo)
        .expect("status of the sandbox repository")
}

fn diff_of(sandbox: &Sandbox, path: &str, side: DiffSide) -> FileDiff {
    SystemGit
        .diff(&sandbox.repo, Path::new(path), side)
        .expect("diff of the sandbox repository")
}

/// The (kind, text) of every line in every hunk, in order. Panics on a binary diff.
fn lines_of(diff: &FileDiff) -> Vec<(DiffLineKind, String)> {
    match diff {
        FileDiff::Text { hunks } => hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter().map(|line| (line.kind, line.text.clone())))
            .collect(),
        FileDiff::Binary => panic!("expected a text diff, got Binary"),
    }
}

fn by_path(mut changes: Vec<FileChange>) -> Vec<FileChange> {
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

fn change(path: &str, staged: Option<ChangeKind>, unstaged: Option<ChangeKind>) -> FileChange {
    FileChange {
        path: PathBuf::from(path),
        original_path: None,
        staged,
        unstaged,
        untracked: false,
        conflicted: false,
    }
}

fn untracked(path: &str) -> FileChange {
    FileChange {
        path: PathBuf::from(path),
        original_path: None,
        staged: None,
        unstaged: None,
        untracked: true,
        conflicted: false,
    }
}

fn commit_count(sandbox: &Sandbox) -> usize {
    git(sandbox, &["rev-list", "--all", "--count"])
        .trim()
        .parse()
        .expect("commit count is a number")
}

/// `git log -1 --format=<format>` with the one newline git always adds removed.
fn last_commit(sandbox: &Sandbox, format: &str) -> String {
    let format_arg = format!("--format={format}");
    let text = git(sandbox, &["log", "-1", format_arg.as_str()]);
    // git ends a stored message with a newline and `log` adds its own.
    text.trim_end_matches('\n').to_string()
}

// --- repository_root ----------------------------------------------------------

#[test]
fn repository_root_from_a_subdirectory_is_the_top_of_the_tree() {
    let sandbox = sandbox();
    let deep = sandbox.repo.join("a").join("b");
    fs::create_dir_all(&deep).expect("create nested folder");

    let from_subdirectory = SystemGit
        .repository_root(&deep)
        .expect("a subdirectory of a repository has a root");
    let from_root = SystemGit
        .repository_root(&sandbox.repo)
        .expect("the repository root has a root");

    let expected = sandbox.repo.canonicalize().expect("canonicalize repo");
    assert_eq!(
        from_subdirectory
            .canonicalize()
            .expect("canonicalize result"),
        expected
    );
    assert_eq!(
        from_root.canonicalize().expect("canonicalize result"),
        expected
    );
}

#[test]
fn repository_root_of_a_plain_folder_is_not_a_repository() {
    let sandbox = sandbox();
    let result = SystemGit.repository_root(&sandbox.plain);
    assert_eq!(result, Err(GitError::NotARepository(sandbox.plain.clone())));
}

#[test]
fn repository_root_of_a_missing_path_is_not_a_repository() {
    let sandbox = sandbox();
    let missing = sandbox.plain.join("does-not-exist");
    let result = SystemGit.repository_root(&missing);
    assert_eq!(result, Err(GitError::NotARepository(missing)));
}

#[test]
fn missing_git_is_reported_as_not_installed() {
    let sandbox = sandbox();
    let empty_bin = sandbox.plain.join("no-git-here");
    fs::create_dir_all(&empty_bin).expect("create empty PATH folder");

    let previous = std::env::var_os("PATH");
    // SAFETY: this test holds `SERIAL`; PATH is restored before the lock is released.
    unsafe { std::env::set_var("PATH", &empty_bin) };
    // Catch a panic so PATH is restored even when the call panics.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        SystemGit.repository_root(&sandbox.repo)
    }));
    // SAFETY: as above.
    unsafe {
        match &previous {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
    let result = outcome.unwrap_or_else(|panic| resume_unwind(panic));

    assert_eq!(result, Err(GitError::NotInstalled));
}

// --- status ----------------------------------------------------------------------

#[test]
fn status_lists_each_kind_of_change_and_the_branch() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);

    // Modified in the working tree only.
    write(&sandbox, "modify.txt", b"changed\n");
    // Added and staged.
    write(&sandbox, "added.txt", b"new\n");
    git(&sandbox, &["add", "--", "added.txt"]);
    // Deleted in the working tree only.
    fs::remove_file(sandbox.repo.join("delete.txt")).expect("remove file");
    // Deleted and staged.
    git(&sandbox, &["rm", "-q", "--", "staged-delete.txt"]);
    // Renamed and staged.
    git(&sandbox, &["mv", "--", "rename-src.txt", "rename-dst.txt"]);
    // Untracked.
    write(&sandbox, "untracked.txt", b"loose\n");

    let status = status_of(&sandbox);
    assert_eq!(status.head, BranchHead::Branch("main".to_string()));
    assert!(!status.unborn);
    assert_eq!(
        by_path(status.changes),
        vec![
            change("added.txt", Some(ChangeKind::Added), None),
            change("delete.txt", None, Some(ChangeKind::Deleted)),
            change("modify.txt", None, Some(ChangeKind::Modified)),
            FileChange {
                path: PathBuf::from("rename-dst.txt"),
                original_path: Some(PathBuf::from("rename-src.txt")),
                staged: Some(ChangeKind::Renamed),
                unstaged: None,
                untracked: false,
                conflicted: false,
            },
            change("staged-delete.txt", Some(ChangeKind::Deleted), None),
            untracked("untracked.txt"),
        ]
    );
}

#[test]
fn status_file_staged_then_edited_again_is_on_both_sides() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);

    write(&sandbox, "other.txt", b"staged version\n");
    git(&sandbox, &["add", "--", "other.txt"]);
    write(&sandbox, "other.txt", b"edited after staging\n");

    assert_eq!(
        status_of(&sandbox).changes,
        vec![change(
            "other.txt",
            Some(ChangeKind::Modified),
            Some(ChangeKind::Modified)
        )]
    );
}

#[test]
fn status_keeps_names_with_spaces_non_ascii_and_leading_dash_whole() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);

    write(&sandbox, "sp ace.txt", b"edited\n");
    write(&sandbox, "中文.txt", b"edited\n");
    write(&sandbox, "-dash.txt", b"edited\n");

    assert_eq!(
        by_path(status_of(&sandbox).changes),
        vec![
            change("-dash.txt", None, Some(ChangeKind::Modified)),
            change("sp ace.txt", None, Some(ChangeKind::Modified)),
            change("中文.txt", None, Some(ChangeKind::Modified)),
        ]
    );
}

#[test]
fn status_reports_a_conflicted_file() {
    let sandbox = sandbox();
    write(&sandbox, "conflict.txt", b"base\n");
    commit_baseline(&sandbox, &["conflict.txt"]);

    git(&sandbox, &["checkout", "-q", "-b", "side"]);
    write(&sandbox, "conflict.txt", b"side\n");
    git(&sandbox, &["commit", "-q", "-am", "side"]);
    git(&sandbox, &["checkout", "-q", "main"]);
    write(&sandbox, "conflict.txt", b"main\n");
    git(&sandbox, &["commit", "-q", "-am", "main"]);

    // The merge is expected to stop with a conflict, so its exit code is not checked.
    let merge = git_command(&sandbox, &["merge", "--no-edit", "side"])
        .output()
        .expect("run git merge");
    assert!(!merge.status.success(), "the merge should conflict");

    assert_eq!(
        status_of(&sandbox).changes,
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

// --- diff -----------------------------------------------------------------------

#[test]
fn unstaged_diff_shows_the_working_tree_and_not_the_index() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);

    write(&sandbox, "modify.txt", b"base modify.txt\nnew line\n");

    let unstaged = diff_of(&sandbox, "modify.txt", DiffSide::Unstaged);
    assert_eq!(
        lines_of(&unstaged),
        vec![
            (DiffLineKind::Context, "base modify.txt".to_string()),
            (DiffLineKind::Added, "new line".to_string()),
        ]
    );
    let staged = diff_of(&sandbox, "modify.txt", DiffSide::Staged);
    assert_eq!(staged, FileDiff::Text { hunks: vec![] });
}

#[test]
fn staged_diff_shows_the_index_and_unstaged_shows_only_later_edits() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);

    write(&sandbox, "modify.txt", b"base modify.txt\nfirst\n");
    git(&sandbox, &["add", "--", "modify.txt"]);
    write(&sandbox, "modify.txt", b"base modify.txt\nfirst\nsecond\n");

    assert_eq!(
        lines_of(&diff_of(&sandbox, "modify.txt", DiffSide::Staged)),
        vec![
            (DiffLineKind::Context, "base modify.txt".to_string()),
            (DiffLineKind::Added, "first".to_string()),
        ]
    );
    assert_eq!(
        lines_of(&diff_of(&sandbox, "modify.txt", DiffSide::Unstaged)),
        vec![
            (DiffLineKind::Context, "base modify.txt".to_string()),
            (DiffLineKind::Context, "first".to_string()),
            (DiffLineKind::Added, "second".to_string()),
        ]
    );
}

#[test]
fn unstaged_diff_of_an_untracked_file_is_the_whole_file_as_added() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "untracked.txt", b"one\ntwo\n");

    let diff = diff_of(&sandbox, "untracked.txt", DiffSide::Unstaged);
    let FileDiff::Text { hunks } = &diff else {
        panic!("an untracked text file is a text diff, got {diff:?}");
    };
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].header, "@@ -0,0 +1,2 @@");
    assert_eq!(
        hunks[0].lines,
        vec![
            git_core::DiffLine {
                kind: DiffLineKind::Added,
                text: "one".to_string(),
                old_line: None,
                new_line: Some(1),
                no_newline_at_end: false,
            },
            git_core::DiffLine {
                kind: DiffLineKind::Added,
                text: "two".to_string(),
                old_line: None,
                new_line: Some(2),
                no_newline_at_end: false,
            },
        ]
    );
}

#[test]
fn binary_file_diff_is_binary_on_both_sides() {
    let sandbox = sandbox();
    write(&sandbox, "blob.bin", b"\x00\x01first\x02");
    commit_baseline(&sandbox, &["blob.bin"]);

    write(&sandbox, "blob.bin", b"\x00\x01second\x02");
    assert_eq!(
        diff_of(&sandbox, "blob.bin", DiffSide::Unstaged),
        FileDiff::Binary
    );

    git(&sandbox, &["add", "--", "blob.bin"]);
    assert_eq!(
        diff_of(&sandbox, "blob.bin", DiffSide::Staged),
        FileDiff::Binary
    );
}

#[test]
fn diff_of_a_path_that_looks_like_an_option_reads_that_file() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "-dash.txt", b"base -dash.txt\nadded\n");

    assert_eq!(
        lines_of(&diff_of(&sandbox, "-dash.txt", DiffSide::Unstaged)),
        vec![
            (DiffLineKind::Context, "base -dash.txt".to_string()),
            (DiffLineKind::Added, "added".to_string()),
        ]
    );
}

// --- stage and unstage --------------------------------------------------------------

#[test]
fn stage_changes_only_the_chosen_file() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "modify.txt", b"changed a\n");
    write(&sandbox, "other.txt", b"changed b\n");

    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("modify.txt")])
        .expect("stage one file");

    assert_eq!(
        by_path(status_of(&sandbox).changes),
        vec![
            change("modify.txt", Some(ChangeKind::Modified), None),
            change("other.txt", None, Some(ChangeKind::Modified)),
        ]
    );
}

#[test]
fn stage_of_a_deleted_file_stages_the_deletion() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    fs::remove_file(sandbox.repo.join("delete.txt")).expect("remove file");

    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("delete.txt")])
        .expect("stage the deletion");

    assert_eq!(
        status_of(&sandbox).changes,
        vec![change("delete.txt", Some(ChangeKind::Deleted), None)]
    );
}

#[test]
fn stage_and_unstage_with_no_paths_do_nothing() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "modify.txt", b"changed\n");
    let before = status_of(&sandbox);

    SystemGit
        .stage(&sandbox.repo, &[])
        .expect("staging nothing is not an error");
    assert_eq!(
        status_of(&sandbox),
        before,
        "staging nothing changed the status"
    );

    SystemGit
        .unstage(&sandbox.repo, &[])
        .expect("unstaging nothing is not an error");
    assert_eq!(
        status_of(&sandbox),
        before,
        "unstaging nothing changed the status"
    );
}

#[test]
fn unstage_takes_the_file_out_of_the_index_and_keeps_its_content() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "modify.txt", b"my edit\n");
    git(&sandbox, &["add", "--", "modify.txt"]);
    write(&sandbox, "added.txt", b"new\n");
    git(&sandbox, &["add", "--", "added.txt"]);

    SystemGit
        .unstage(
            &sandbox.repo,
            &[PathBuf::from("modify.txt"), PathBuf::from("added.txt")],
        )
        .expect("unstage both files");

    assert_eq!(
        by_path(status_of(&sandbox).changes),
        vec![
            untracked("added.txt"),
            change("modify.txt", None, Some(ChangeKind::Modified)),
        ]
    );
    assert_eq!(
        fs::read(sandbox.repo.join("modify.txt")).expect("read file"),
        b"my edit\n",
        "unstaging must not touch the working tree"
    );
}

#[test]
fn stage_and_unstage_accept_a_path_that_starts_with_a_dash() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "-dash.txt", b"edited\n");

    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("-dash.txt")])
        .expect("stage a path that starts with a dash");
    assert_eq!(
        status_of(&sandbox).changes,
        vec![change("-dash.txt", Some(ChangeKind::Modified), None)]
    );

    SystemGit
        .unstage(&sandbox.repo, &[PathBuf::from("-dash.txt")])
        .expect("unstage a path that starts with a dash");
    assert_eq!(
        status_of(&sandbox).changes,
        vec![change("-dash.txt", None, Some(ChangeKind::Modified))]
    );
}

#[test]
fn stage_of_a_missing_path_fails_with_git_message() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);

    let result = SystemGit.stage(&sandbox.repo, &[PathBuf::from("no-such-file.txt")]);
    match result {
        Err(GitError::Failed { message, .. }) => {
            assert!(!message.is_empty(), "git's own message should be shown");
            assert_eq!(message, message.trim(), "the message is trimmed");
        }
        other => panic!("expected Failed with git's message, got {other:?}"),
    }
}

#[test]
fn stage_and_unstage_work_in_a_repository_with_no_commit() {
    let sandbox = sandbox();
    write(&sandbox, "first.txt", b"first\n");

    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("first.txt")])
        .expect("stage in an unborn repository");
    let staged = status_of(&sandbox);
    assert!(staged.unborn);
    assert_eq!(
        staged.changes,
        vec![change("first.txt", Some(ChangeKind::Added), None)]
    );

    SystemGit
        .unstage(&sandbox.repo, &[PathBuf::from("first.txt")])
        .expect("unstage in an unborn repository");
    let unstaged = status_of(&sandbox);
    assert!(unstaged.unborn);
    assert_eq!(unstaged.changes, vec![untracked("first.txt")]);
    assert_eq!(
        fs::read(sandbox.repo.join("first.txt")).expect("read file"),
        b"first\n"
    );
}

// --- commit -----------------------------------------------------------------------

#[test]
fn commit_records_summary_and_description_and_clears_the_changes() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "other.txt", b"changed\n");
    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("other.txt")])
        .expect("stage");

    SystemGit
        .commit(
            &sandbox.repo,
            &CommitMessage {
                summary: "Change other".to_string(),
                description: "Explain why.\nSecond line.".to_string(),
            },
        )
        .expect("commit the staged file");

    assert_eq!(
        last_commit(&sandbox, "%s%n%b"),
        "Change other\nExplain why.\nSecond line."
    );
    let status = status_of(&sandbox);
    assert!(
        status.changes.is_empty(),
        "nothing is left to commit: {:?}",
        status.changes
    );
}

#[test]
fn commit_with_an_empty_description_writes_no_body() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "other.txt", b"changed\n");
    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("other.txt")])
        .expect("stage");

    SystemGit
        .commit(
            &sandbox.repo,
            &CommitMessage {
                summary: "Summary only".to_string(),
                description: String::new(),
            },
        )
        .expect("commit with no description");

    assert_eq!(last_commit(&sandbox, "%s"), "Summary only");
    assert_eq!(last_commit(&sandbox, "%b"), "");
}

#[test]
fn commit_keeps_non_ascii_summary_and_description() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "other.txt", b"changed\n");
    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("other.txt")])
        .expect("stage");

    SystemGit
        .commit(
            &sandbox.repo,
            &CommitMessage {
                summary: "中文標題".to_string(),
                description: "第二行說明".to_string(),
            },
        )
        .expect("commit non-ASCII text");

    assert_eq!(last_commit(&sandbox, "%s%n%b"), "中文標題\n第二行說明");
}

#[test]
fn commit_with_a_blank_summary_is_refused_and_makes_no_commit() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "other.txt", b"changed\n");
    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("other.txt")])
        .expect("stage");
    let before = commit_count(&sandbox);

    for summary in ["", "   \t "] {
        let result = SystemGit.commit(
            &sandbox.repo,
            &CommitMessage {
                summary: summary.to_string(),
                description: "a description".to_string(),
            },
        );
        assert!(
            matches!(result, Err(GitError::Failed { .. })),
            "blank summary {summary:?} must be Failed, got {result:?}"
        );
    }
    assert_eq!(
        commit_count(&sandbox),
        before,
        "a refused commit must not create a commit"
    );
}

#[test]
fn commit_with_nothing_staged_fails_and_makes_no_commit() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "other.txt", b"changed but not staged\n");
    let before = commit_count(&sandbox);

    let result = SystemGit.commit(
        &sandbox.repo,
        &CommitMessage {
            summary: "Nothing here".to_string(),
            description: String::new(),
        },
    );

    assert!(
        matches!(result, Err(GitError::Failed { .. })),
        "a commit with nothing staged must be Failed, got {result:?}"
    );
    assert_eq!(commit_count(&sandbox), before);
}

// --- repository with no commit yet -------------------------------------------------

#[test]
fn unborn_repository_reports_unborn_with_its_files_untracked() {
    let sandbox = sandbox();
    write(&sandbox, "first.txt", b"first\n");

    let status = status_of(&sandbox);
    assert!(status.unborn);
    assert_eq!(status.head, BranchHead::Branch("main".to_string()));
    assert_eq!(status.changes, vec![untracked("first.txt")]);
}

#[test]
fn unborn_repository_staged_diff_shows_the_whole_file_as_added() {
    let sandbox = sandbox();
    write(&sandbox, "first.txt", b"one\ntwo\n");
    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("first.txt")])
        .expect("stage");

    let diff = diff_of(&sandbox, "first.txt", DiffSide::Staged);
    assert_eq!(
        lines_of(&diff),
        vec![
            (DiffLineKind::Added, "one".to_string()),
            (DiffLineKind::Added, "two".to_string()),
        ]
    );
}

#[test]
fn unborn_repository_commit_creates_the_first_commit() {
    let sandbox = sandbox();
    write(&sandbox, "first.txt", b"first\n");
    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("first.txt")])
        .expect("stage");

    SystemGit
        .commit(
            &sandbox.repo,
            &CommitMessage {
                summary: "First commit".to_string(),
                description: String::new(),
            },
        )
        .expect("the first commit in an unborn repository");

    assert_eq!(commit_count(&sandbox), 1);
    assert_eq!(last_commit(&sandbox, "%s"), "First commit");
    let status = status_of(&sandbox);
    assert!(
        !status.unborn,
        "a repository with a commit is no longer unborn"
    );
    assert!(status.changes.is_empty());
}

// --- a repository someone else configured ---------------------------------------

/// Sets up a repository whose own configuration names programs to run, each
/// of which leaves a marker file, and returns the folder the markers go in.
fn booby_trap(sandbox: &Sandbox) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let markers = sandbox.home.join("markers");
    let hooks = sandbox.home.join("hooks");
    fs::create_dir_all(&markers).expect("create marker folder");
    fs::create_dir_all(&hooks).expect("create hooks folder");
    let script = |name: &str| {
        let path = sandbox.home.join(format!("{name}.sh"));
        let body = format!(
            "#!/bin/sh\ntouch '{}'\ncat 2>/dev/null\nexit 0\n",
            markers.join(name).display()
        );
        fs::write(&path, body).expect("write script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod script");
        path
    };
    let fsmonitor = script("fsmonitor");
    let textconv = script("textconv");
    let hook = hooks.join("post-index-change");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\ntouch '{}'\n",
            markers.join("post-index-change").display()
        ),
    )
    .expect("write hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod hook");

    commit_standard_baseline(sandbox);
    write(sandbox, ".gitattributes", b"*.txt diff=trap\n");
    git(
        sandbox,
        &["config", "core.fsmonitor", fsmonitor.to_str().unwrap()],
    );
    git(
        sandbox,
        &["config", "diff.trap.textconv", textconv.to_str().unwrap()],
    );
    git(
        sandbox,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    // Setting the configuration above ran nothing yet; start from no markers.
    for entry in fs::read_dir(&markers).expect("read markers") {
        fs::remove_file(entry.expect("marker entry").path()).expect("remove marker");
    }
    markers
}

fn markers_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .expect("read markers")
        .map(|entry| {
            entry
                .expect("marker entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn looking_at_a_repository_runs_none_of_its_configured_programs() {
    let sandbox = sandbox();
    let markers = booby_trap(&sandbox);
    write(&sandbox, "modify.txt", b"changed\n");
    write(&sandbox, "new.txt", b"new\n");

    SystemGit
        .repository_root(&sandbox.repo)
        .expect("repository root");
    SystemGit.status(&sandbox.repo).expect("status");
    SystemGit
        .diff(&sandbox.repo, Path::new("modify.txt"), DiffSide::Unstaged)
        .expect("diff of a tracked file");
    SystemGit
        .diff(&sandbox.repo, Path::new("new.txt"), DiffSide::Unstaged)
        .expect("diff of an untracked file");
    SystemGit
        .stage(&sandbox.repo, &[PathBuf::from("modify.txt")])
        .expect("stage");
    SystemGit
        .diff(&sandbox.repo, Path::new("modify.txt"), DiffSide::Staged)
        .expect("staged diff");
    SystemGit
        .unstage(&sandbox.repo, &[PathBuf::from("modify.txt")])
        .expect("unstage");

    assert_eq!(markers_in(&markers), Vec::<String>::new());
}

#[test]
fn a_file_that_became_a_symlink_has_a_readable_diff() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    fs::remove_file(sandbox.repo.join("modify.txt")).expect("remove file");
    std::os::unix::fs::symlink("elsewhere", sandbox.repo.join("modify.txt")).expect("symlink");

    let diff = diff_of(&sandbox, "modify.txt", DiffSide::Unstaged);
    let lines = lines_of(&diff);
    assert!(
        lines.contains(&(DiffLineKind::Added, "elsewhere".to_string())),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|(kind, _)| *kind == DiffLineKind::Removed),
        "{lines:?}"
    );
}

#[test]
fn an_untracked_folder_that_is_its_own_repository_reports_gits_error() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    let nested = sandbox.repo.join("nested");
    fs::create_dir_all(&nested).expect("create nested folder");
    let mut init = git_command(&sandbox, &["init", "-q", "-b", "main"]);
    init.current_dir(&nested);
    assert!(init.status().expect("run git init").success());
    fs::write(nested.join("inner.txt"), b"inner\n").expect("write inner file");

    let result = SystemGit.diff(&sandbox.repo, Path::new("nested/"), DiffSide::Unstaged);
    assert!(matches!(result, Err(GitError::Failed { .. })), "{result:?}");
}

#[test]
fn a_repository_git_refuses_for_its_owner_keeps_gits_explanation() {
    let sandbox = sandbox();
    // SAFETY: `SERIAL` is held, as for every test in this file.
    unsafe { std::env::set_var("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1") };
    let result = SystemGit.repository_root(&sandbox.repo);
    unsafe { std::env::remove_var("GIT_TEST_ASSUME_DIFFERENT_OWNER") };

    match result {
        Err(GitError::Failed { message, .. }) => {
            assert!(message.contains("dubious ownership"), "{message}");
        }
        other => panic!("expected Failed with git's message, got {other:?}"),
    }
}

#[test]
fn a_git_dir_in_the_environment_does_not_redirect_the_repository() {
    let sandbox = sandbox();
    commit_standard_baseline(&sandbox);
    write(&sandbox, "modify.txt", b"changed\n");
    // SAFETY: `SERIAL` is held, as for every test in this file.
    unsafe { std::env::set_var("GIT_DIR", sandbox.plain.join("no-such.git")) };
    let result = SystemGit.status(&sandbox.repo);
    unsafe { std::env::remove_var("GIT_DIR") };

    let status = result.expect("status reads the repository `-C` names");
    assert_eq!(status.changes.len(), 1);
}
