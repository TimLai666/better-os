//! The session against a real `git`, in repositories made in temporary
//! directories.
//!
//! `HOME` points at the temporary directory, the global and system git
//! configuration are switched off, and the commit identity is fixed. The
//! environment is process-wide, so the tests take `SERIAL` and run one at a
//! time.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};

use git_gui::session::{Badge, Check, DiffPane, GitSession, Problem};
use git_platform::{GitError, SystemGit};
use tempfile::TempDir;

static SERIAL: Mutex<()> = Mutex::new(());

struct Sandbox {
    _serial: MutexGuard<'static, ()>,
    tmp: TempDir,
    repo: PathBuf,
}

fn sandbox() -> Sandbox {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let tmp = TempDir::new().expect("create temporary directory");
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&repo).unwrap();
    // SAFETY: every test in this file holds `SERIAL` while it runs, so no
    // other thread reads or writes the environment at the same time.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_AUTHOR_NAME", "Better Git Test");
        std::env::set_var("GIT_AUTHOR_EMAIL", "test@example.invalid");
        std::env::set_var("GIT_COMMITTER_NAME", "Better Git Test");
        std::env::set_var("GIT_COMMITTER_EMAIL", "test@example.invalid");
    }
    let sandbox = Sandbox {
        _serial: serial,
        tmp,
        repo,
    };
    git(&sandbox, &["init", "-q", "-b", "main"]);
    sandbox
}

fn git_output(sandbox: &Sandbox, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(&sandbox.repo)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .expect("run git")
}

fn git(sandbox: &Sandbox, args: &[&str]) -> String {
    let output = git_output(sandbox, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn write(sandbox: &Sandbox, path: &str, content: &str) {
    let path = sandbox.repo.join(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn session(sandbox: &Sandbox) -> GitSession {
    let recent = sandbox.tmp.path().join("state/better-os/git-recent.json");
    let mut session = GitSession::new(Arc::new(SystemGit), Some(recent));
    session.open(&sandbox.repo);
    session.run_pending();
    assert_eq!(session.problem(), None);
    session
}

fn row<'a>(rows: &'a [git_gui::session::ChangeRow], path: &str) -> &'a git_gui::session::ChangeRow {
    rows.iter()
        .find(|row| row.path == Path::new(path))
        .unwrap_or_else(|| panic!("no row for {path}: {rows:?}"))
}

#[test]
fn the_changes_list_matches_git_status() {
    let sandbox = sandbox();
    for name in [
        "modified.txt",
        "deleted.txt",
        "old name.txt",
        "both.txt",
        "clash.txt",
    ] {
        write(&sandbox, name, &format!("{name}\none\ntwo\n"));
    }
    git(&sandbox, &["add", "-A"]);
    git(&sandbox, &["commit", "-q", "-m", "base"]);

    // A conflict: two branches change one line differently.
    git(&sandbox, &["checkout", "-q", "-b", "other"]);
    write(&sandbox, "clash.txt", "other\n");
    git(&sandbox, &["commit", "-q", "-am", "other"]);
    git(&sandbox, &["checkout", "-q", "main"]);
    write(&sandbox, "clash.txt", "main\n");
    git(&sandbox, &["commit", "-q", "-am", "main"]);
    assert!(
        !git_output(&sandbox, &["merge", "-q", "other"])
            .status
            .success()
    );

    write(&sandbox, "modified.txt", "changed\n");
    fs::remove_file(sandbox.repo.join("deleted.txt")).unwrap();
    git(&sandbox, &["mv", "old name.txt", "new name.txt"]);
    write(&sandbox, "added.txt", "new\n");
    git(&sandbox, &["add", "added.txt"]);
    write(&sandbox, "both.txt", "staged\n");
    git(&sandbox, &["add", "both.txt"]);
    write(&sandbox, "both.txt", "staged\nand changed again\n");
    write(&sandbox, "dir/naïve 檔案.txt", "untracked\n");
    write(&sandbox, "-dash.txt", "untracked\n");

    let session = session(&sandbox);
    let rows = session.rows();
    let expect = [
        ("modified.txt", Badge::Modified, Check::Unchecked),
        ("deleted.txt", Badge::Deleted, Check::Unchecked),
        ("new name.txt", Badge::Renamed, Check::Checked),
        ("added.txt", Badge::Added, Check::Checked),
        ("both.txt", Badge::Modified, Check::Indeterminate),
        ("clash.txt", Badge::Conflicted, Check::Unchecked),
        ("dir/naïve 檔案.txt", Badge::Added, Check::Unchecked),
        ("-dash.txt", Badge::Added, Check::Unchecked),
    ];
    for (path, badge, check) in expect {
        let found = row(&rows, path);
        assert_eq!((found.badge, found.check), (badge, check), "{path}");
    }
    assert_eq!(rows.len(), expect.len(), "{rows:?}");
    assert!(!row(&rows, "clash.txt").stageable);
    assert_eq!(row(&rows, "dir/naïve 檔案.txt").folder, "dir");
}

#[test]
fn staging_and_unstaging_change_exactly_the_chosen_file() {
    let sandbox = sandbox();
    write(&sandbox, "a.txt", "a\n");
    write(&sandbox, "-b.txt", "b\n");
    write(&sandbox, "c d.txt", "c\n");
    git(&sandbox, &["add", "-A"]);
    git(&sandbox, &["commit", "-q", "-m", "base"]);
    for name in ["a.txt", "-b.txt", "c d.txt"] {
        write(&sandbox, name, "changed\n");
    }

    let mut session = session(&sandbox);
    session.toggle_stage(Path::new("-b.txt"));
    session.run_pending();
    assert_eq!(
        git(&sandbox, &["diff", "--cached", "--name-only"]),
        "-b.txt\n"
    );
    assert_eq!(row(&session.rows(), "-b.txt").check, Check::Checked);

    session.toggle_stage(Path::new("c d.txt"));
    session.run_pending();
    session.toggle_stage(Path::new("-b.txt"));
    session.run_pending();
    assert_eq!(
        git(&sandbox, &["diff", "--cached", "--name-only"]),
        "c d.txt\n"
    );

    session.stage_all();
    session.run_pending();
    assert_eq!(session.all_check(), Check::Checked);
    session.unstage_all();
    session.run_pending();
    assert_eq!(git(&sandbox, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(session.problem(), None);
}

#[test]
fn unstaging_a_staged_rename_puts_both_paths_back() {
    let sandbox = sandbox();
    write(&sandbox, "old.txt", "content\n");
    git(&sandbox, &["add", "-A"]);
    git(&sandbox, &["commit", "-q", "-m", "base"]);
    git(&sandbox, &["mv", "old.txt", "new.txt"]);

    let mut session = session(&sandbox);
    assert_eq!(row(&session.rows(), "new.txt").badge, Badge::Renamed);
    session.toggle_stage(Path::new("new.txt"));
    session.run_pending();
    assert_eq!(session.problem(), None);
    assert_eq!(git(&sandbox, &["diff", "--cached", "--name-only"]), "");
}

#[test]
fn a_commit_made_in_the_window_is_in_the_log_and_the_list_empties() {
    let sandbox = sandbox();
    write(&sandbox, "a.txt", "a\n");
    git(&sandbox, &["add", "-A"]);
    git(&sandbox, &["commit", "-q", "-m", "base"]);
    write(&sandbox, "a.txt", "a\nb\n");

    let mut session = session(&sandbox);
    session.select(Path::new("a.txt"));
    session.run_pending();
    let DiffPane::Text(hunks) = session.diff_pane() else {
        panic!("expected a text diff: {:?}", session.diff_pane());
    };
    assert_eq!(hunks[0].lines.last().unwrap().text, "b");

    session.toggle_stage(Path::new("a.txt"));
    session.run_pending();
    session.summary = "Add b".to_string();
    session.description = "Second line.".to_string();
    assert!(session.can_commit());
    session.commit();
    session.run_pending();

    assert_eq!(session.problem(), None);
    assert_eq!(
        git(&sandbox, &["log", "-1", "--format=%B"]),
        "Add b\n\nSecond line.\n\n"
    );
    assert!(session.rows().is_empty());
    assert_eq!(session.summary, "");
    assert!(!session.can_commit());
}

#[test]
fn a_repository_with_no_commits_yet_works() {
    let sandbox = sandbox();
    write(&sandbox, "README.md", "hello\n");
    let mut session = session(&sandbox);
    assert_eq!(session.branch(), Some("main"));
    assert!(session.status().unwrap().unborn);

    session.toggle_stage(Path::new("README.md"));
    session.run_pending();
    assert_eq!(row(&session.rows(), "README.md").check, Check::Checked);
    session.toggle_stage(Path::new("README.md"));
    session.run_pending();
    assert_eq!(row(&session.rows(), "README.md").check, Check::Unchecked);

    session.stage_all();
    session.run_pending();
    session.summary = "First".to_string();
    session.commit();
    session.run_pending();
    assert_eq!(session.problem(), None);
    assert_eq!(git(&sandbox, &["log", "--format=%s"]), "First\n");
    assert!(session.rows().is_empty());
}

#[test]
fn a_binary_file_says_it_is_binary() {
    let sandbox = sandbox();
    fs::write(sandbox.repo.join("image.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    git(&sandbox, &["add", "-A"]);
    git(&sandbox, &["commit", "-q", "-m", "base"]);
    fs::write(sandbox.repo.join("image.bin"), [0u8, 1, 2, 3, 0, 9]).unwrap();
    let mut session = session(&sandbox);
    session.select(Path::new("image.bin"));
    session.run_pending();
    assert_eq!(session.diff_pane(), DiffPane::Binary);
}

#[test]
fn a_folder_that_is_not_a_repository_opens_nothing() {
    let sandbox = sandbox();
    let plain = sandbox.tmp.path().join("plain");
    fs::create_dir_all(&plain).unwrap();
    let mut session = GitSession::new(Arc::new(SystemGit), None);
    session.open(&plain);
    session.run_pending();
    assert_eq!(session.repository(), None);
    assert_eq!(
        session.problem(),
        Some(&Problem::Git(GitError::NotARepository(plain)))
    );
}

#[test]
fn git_missing_is_reported_not_a_crash() {
    let sandbox = sandbox();
    let empty = sandbox.tmp.path().join("no-programs");
    fs::create_dir_all(&empty).unwrap();
    let path = std::env::var_os("PATH");
    // SAFETY: `SERIAL` is held, as above; PATH is put back before it is
    // released.
    unsafe { std::env::set_var("PATH", &empty) };
    let mut session = GitSession::new(Arc::new(SystemGit), None);
    session.open(&sandbox.repo);
    session.run_pending();
    unsafe {
        match path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
    assert_eq!(session.repository(), None);
    assert_eq!(
        session.problem(),
        Some(&Problem::Git(GitError::NotInstalled))
    );
}
