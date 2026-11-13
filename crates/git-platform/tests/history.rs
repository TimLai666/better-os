//! Commit options and history edits against a real `git`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use git_core::CommitFile;
use git_platform::{
    CommitMessage, CommitOptions, DiffOptions, Git, GitError, HistoryEdit, SystemGit, TodoAction,
};

// --- harness: a temporary HOME, no global or system git configuration, a
// fixed identity, and a repository on `main`. The environment is
// process-wide, so tests hold `SERIAL`.

#[allow(dead_code)]
mod harness {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Mutex, MutexGuard};

    use tempfile::TempDir;

    static SERIAL: Mutex<()> = Mutex::new(());

    pub struct Sandbox {
        _serial: MutexGuard<'static, ()>,
        pub tmp: TempDir,
        pub home: PathBuf,
        pub repo: PathBuf,
    }

    pub fn sandbox() -> Sandbox {
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = TempDir::new().expect("temporary directory");
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&repo).unwrap();
        // SAFETY: tests hold `SERIAL`, so nothing else touches the
        // environment meanwhile.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_DATA_HOME", home.join("data"));
            std::env::set_var("XDG_STATE_HOME", home.join("state"));
            std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
            std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
            for key in ["GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"] {
                std::env::set_var(key, "Better Git Test");
            }
            for key in ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"] {
                std::env::set_var(key, "test@example.invalid");
            }
        }
        let sandbox = Sandbox {
            _serial: serial,
            tmp,
            home,
            repo,
        };
        git_in(&sandbox.repo, &["init", "-q", "-b", "main"]);
        sandbox
    }

    pub fn git_in(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    impl Sandbox {
        pub fn git(&self, args: &[&str]) -> String {
            git_in(&self.repo, args)
        }

        pub fn write(&self, relative: &str, contents: &str) {
            let path = self.repo.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, contents).unwrap();
        }

        pub fn read(&self, relative: &str) -> String {
            fs::read_to_string(self.repo.join(relative)).unwrap()
        }

        pub fn commit_file(&self, relative: &str, contents: &str, message: &str) {
            self.write(relative, contents);
            self.git(&["add", "--", relative]);
            self.git(&["commit", "-q", "-m", message]);
        }

        pub fn head(&self) -> String {
            self.git(&["rev-parse", "HEAD"]).trim().to_string()
        }

        pub fn subjects(&self) -> Vec<String> {
            self.git(&["log", "--format=%s"])
                .lines()
                .map(str::to_string)
                .collect()
        }

        pub fn porcelain(&self) -> String {
            self.git(&["status", "--porcelain"])
        }

        /// A bare repository beside this one, added as `origin`, with `main`
        /// pushed to it.
        pub fn with_origin(&self) -> PathBuf {
            let bare = self.tmp.path().join("origin.git");
            git_in(
                self.tmp.path(),
                &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
            );
            self.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
            self.git(&["push", "-q", "-u", "origin", "main"]);
            bare
        }

        /// A second clone of `origin`, for commits "someone else" pushes.
        pub fn other_clone(&self, bare: &Path) -> PathBuf {
            let other = self.tmp.path().join("other");
            git_in(
                self.tmp.path(),
                &[
                    "clone",
                    "-q",
                    bare.to_str().unwrap(),
                    other.to_str().unwrap(),
                ],
            );
            other
        }
    }
}

use harness::*;

fn message(summary: &str, description: &str) -> CommitMessage {
    CommitMessage {
        summary: summary.to_string(),
        description: description.to_string(),
    }
}

fn three_commits(s: &Sandbox) -> Vec<String> {
    s.commit_file("a.txt", "a\n", "first");
    s.commit_file("b.txt", "b\n", "second");
    s.commit_file("c.txt", "c\n", "third");
    s.git(&["log", "--format=%H"])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn log_reads_the_branch_with_paging() {
    let s = sandbox();
    assert!(
        SystemGit.log(&s.repo, "HEAD", 0, 10).unwrap().is_empty(),
        "no commit yet"
    );
    let shas = three_commits(&s);
    s.git(&["tag", "v1", &shas[1]]);
    let all = SystemGit.log(&s.repo, "HEAD", 0, 10).unwrap();
    assert_eq!(
        all.iter().map(|c| c.summary.as_str()).collect::<Vec<_>>(),
        ["third", "second", "first"]
    );
    assert_eq!(all[1].tags(), vec!["v1"]);
    let page = SystemGit.log(&s.repo, "HEAD", 1, 1).unwrap();
    assert_eq!(page[0].sha, shas[1]);
}

#[test]
fn commit_files_and_diff_of_one_commit() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.git(&["mv", "a.txt", "renamed.txt"]);
    s.write("b.txt", "b\n");
    s.git(&["add", "b.txt"]);
    s.git(&["commit", "-q", "-m", "second"]);
    let files = SystemGit.commit_files(&s.repo, "HEAD").unwrap();
    assert_eq!(files.len(), 2);
    let renamed = files
        .iter()
        .find(|f| f.path == Path::new("renamed.txt"))
        .unwrap();
    assert_eq!(renamed.original_path, Some(PathBuf::from("a.txt")));
    let added: &CommitFile = files.iter().find(|f| f.path == Path::new("b.txt")).unwrap();
    let diff = SystemGit
        .commit_diff(&s.repo, "HEAD", added, DiffOptions::default())
        .unwrap();
    assert!(
        matches!(diff, git_core::FileDiff::Text { ref hunks } if hunks[0].lines[0].text == "b")
    );
    let root = SystemGit.commit_files(&s.repo, "HEAD~1").unwrap();
    assert_eq!(root.len(), 1, "the root commit lists its files");
}

#[test]
fn commit_with_co_authors_amend_and_allow_empty() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.write("a.txt", "b\n");
    s.git(&["add", "a.txt"]);
    let options = CommitOptions {
        co_authors: vec![("Ada".into(), "ada@example.com".into())],
        ..CommitOptions::default()
    };
    SystemGit
        .commit_selection(
            &s.repo,
            &[git_platform::FileSelection::whole("a.txt")],
            &message("second", "why"),
            &options,
        )
        .unwrap();
    let body = s.git(&["log", "-1", "--format=%B"]);
    assert_eq!(
        body,
        "second\n\nwhy\n\nCo-authored-by: Ada <ada@example.com>\n\n"
    );
    let amend = CommitOptions {
        amend: true,
        allow_empty: true,
        ..CommitOptions::default()
    };
    SystemGit
        .commit(&s.repo, &message("second, amended", ""))
        .unwrap_err();
    SystemGit
        .commit_selection(&s.repo, &[], &message("second, amended", ""), &amend)
        .unwrap();
    assert_eq!(s.subjects(), vec!["second, amended", "first"]);
    let empty = CommitOptions {
        allow_empty: true,
        ..CommitOptions::default()
    };
    SystemGit
        .commit_selection(&s.repo, &[], &message("empty", ""), &empty)
        .unwrap();
    assert_eq!(s.subjects()[0], "empty");
}

#[test]
fn a_failing_hook_shows_its_output_and_no_verify_skips_it() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let hook = s.repo.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\necho 'lint says no' >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    s.write("a.txt", "b\n");
    let files = [git_platform::FileSelection::whole("a.txt")];
    let error = SystemGit
        .commit_selection(
            &s.repo,
            &files,
            &message("second", ""),
            &CommitOptions::default(),
        )
        .unwrap_err();
    assert!(
        error.message().unwrap().contains("lint says no"),
        "{error:?}"
    );
    assert_eq!(s.subjects(), vec!["first"]);
    let bypass = CommitOptions {
        no_verify: true,
        ..CommitOptions::default()
    };
    SystemGit
        .commit_selection(&s.repo, &files, &message("second", ""), &bypass)
        .unwrap();
    assert_eq!(s.subjects(), vec!["second", "first"]);
}

#[test]
fn undo_last_commit_keeps_its_changes_even_the_first() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.commit_file("b.txt", "b\n", "second");
    SystemGit
        .edit_history(&s.repo, &HistoryEdit::UndoLast)
        .unwrap();
    assert_eq!(s.subjects(), vec!["first"]);
    assert_eq!(s.porcelain(), "A  b.txt\n");
    s.git(&["commit", "-q", "-m", "second again"]);
    SystemGit
        .edit_history(&s.repo, &HistoryEdit::UndoLast)
        .unwrap();
    SystemGit
        .edit_history(&s.repo, &HistoryEdit::UndoLast)
        .unwrap();
    assert!(SystemGit.log(&s.repo, "HEAD", 0, 5).unwrap().is_empty());
    assert!(s.repo.join("a.txt").exists() && s.repo.join("b.txt").exists());
}

#[test]
fn reset_revert_and_cherry_pick() {
    let s = sandbox();
    let shas = three_commits(&s);
    SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::ResetTo {
                sha: shas[2].clone(),
            },
        )
        .unwrap();
    assert_eq!(s.subjects(), vec!["first"]);
    assert!(
        s.porcelain().contains("?? b.txt"),
        "later changes stay in the working tree"
    );
    s.git(&["add", "-A"]);
    s.git(&["commit", "-q", "-m", "both"]);
    let both = s.head();
    SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::Revert {
                sha: both.clone(),
                merge: false,
            },
        )
        .unwrap();
    assert!(s.subjects()[0].starts_with("Revert \"both\""));
    assert!(!s.repo.join("b.txt").exists());
    s.git(&["branch", "other", &shas[2]]);
    SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::CherryPick {
                shas: vec![both],
                onto: Some("other".into()),
            },
        )
        .unwrap();
    assert_eq!(s.git(&["branch", "--show-current"]).trim(), "other");
    assert_eq!(s.subjects(), vec!["both", "first"]);
}

#[test]
fn cherry_pick_conflict_stops_with_conflicts() {
    let s = sandbox();
    s.commit_file("a.txt", "base\n", "base");
    s.git(&["switch", "-q", "-c", "side"]);
    s.commit_file("a.txt", "side\n", "side");
    let side = s.head();
    s.git(&["switch", "-q", "main"]);
    s.commit_file("a.txt", "main\n", "main");
    let error = SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::CherryPick {
                shas: vec![side],
                onto: None,
            },
        )
        .unwrap_err();
    assert!(matches!(error, GitError::Conflicts(_)), "{error:?}");
}

#[test]
fn reorder_and_squash_rewrite_the_history() {
    let s = sandbox();
    let shas = three_commits(&s);
    let (third, second, first) = (&shas[0], &shas[1], &shas[2]);
    SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::Rewrite {
                base: Some(first.clone()),
                todo: vec![
                    (TodoAction::Pick, third.clone()),
                    (TodoAction::Pick, second.clone()),
                ],
                message: None,
                head: s.head(),
            },
        )
        .unwrap();
    assert_eq!(s.subjects(), vec!["second", "third", "first"]);
    let log: Vec<String> = s
        .git(&["log", "--format=%H"])
        .lines()
        .map(str::to_string)
        .collect();
    SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::Rewrite {
                base: None,
                todo: vec![
                    (TodoAction::Pick, log[2].clone()),
                    (TodoAction::Squash, log[1].clone()),
                    (TodoAction::Pick, log[0].clone()),
                ],
                message: Some("first and third".into()),
                head: s.head(),
            },
        )
        .unwrap();
    assert_eq!(s.subjects(), vec!["second", "first and third"]);
    assert_eq!(s.porcelain(), "");
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(s.repo.join(name).exists());
    }
}

#[test]
fn rewrite_refuses_a_leading_squash() {
    let s = sandbox();
    let shas = three_commits(&s);
    let error = SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::Rewrite {
                base: None,
                todo: vec![(TodoAction::Squash, shas[0].clone())],
                message: None,
                head: s.head(),
            },
        )
        .unwrap_err();
    assert!(matches!(error, GitError::Failed { .. }));
}

#[test]
fn a_huge_message_with_a_chatty_hook_does_not_deadlock() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let hook = s.repo.join(".git/hooks/pre-commit");
    // Prints far more than a pipe holds before git reads the message.
    fs::write(
        &hook,
        "#!/bin/sh\nhead -c 300000 /dev/zero | tr '\\0' x >&2\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    s.write("a.txt", "b\n");
    let description = "y".repeat(256 * 1024);
    SystemGit
        .commit_selection(
            &s.repo,
            &[git_platform::FileSelection::whole("a.txt")],
            &message("big", &description),
            &CommitOptions::default(),
        )
        .unwrap();
    assert_eq!(
        s.git(&["log", "-1", "--format=%b"]).trim().len(),
        description.len()
    );
}

// --- independent review fixes -------------------------------------------

#[test]
fn a_rewrite_made_on_a_stale_list_is_refused_and_drops_nothing() {
    let s = sandbox();
    let shas = three_commits(&s);
    let seen_head = s.head();
    // A commit arrives after the list was read (another window, a pull).
    s.commit_file("d.txt", "d\n", "fourth, made meanwhile");
    let error = SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::Rewrite {
                base: Some(shas[2].clone()),
                todo: vec![
                    (TodoAction::Pick, shas[0].clone()),
                    (TodoAction::Pick, shas[1].clone()),
                ],
                message: None,
                head: seen_head,
            },
        )
        .unwrap_err();
    assert_eq!(error, GitError::ChangedMeanwhile);
    assert_eq!(
        s.subjects(),
        vec!["fourth, made meanwhile", "third", "second", "first"]
    );
}

#[test]
fn a_todo_list_that_leaves_a_commit_out_is_refused_and_undone() {
    let s = sandbox();
    let shas = three_commits(&s);
    let error = SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::Rewrite {
                base: Some(shas[2].clone()),
                todo: vec![(TodoAction::Pick, shas[0].clone())],
                message: None,
                head: s.head(),
            },
        )
        .unwrap_err();
    assert!(matches!(error, GitError::Failed { .. }), "{error:?}");
    assert_eq!(s.subjects(), vec!["third", "second", "first"]);
    assert!(SystemGit.info(&s.repo).unwrap().operation.is_none());
}

#[test]
fn a_squash_that_stops_on_conflicts_keeps_the_message_the_person_wrote() {
    let s = sandbox();
    s.commit_file("base.txt", "b\n", "base");
    s.commit_file("a.txt", "one\n", "first");
    s.commit_file("a.txt", "two\n", "second");
    s.commit_file("a.txt", "three\n", "third");
    let log: Vec<String> = s
        .git(&["log", "--format=%H"])
        .lines()
        .map(str::to_string)
        .collect();
    // A link where the message file goes must not redirect the write.
    let outside = s.tmp.path().join("outside.txt");
    fs::write(&outside, "precious\n").unwrap();
    std::os::unix::fs::symlink(&outside, s.repo.join(".git/better-git-squash-message")).unwrap();
    // Fold "third" into "first": it conflicts.
    let error = SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::Rewrite {
                base: Some(log[3].clone()),
                todo: vec![
                    (TodoAction::Pick, log[2].clone()),
                    (TodoAction::Squash, log[0].clone()),
                    (TodoAction::Pick, log[1].clone()),
                ],
                message: Some("first and third, as I wrote it".into()),
                head: s.head(),
            },
        )
        .unwrap_err();
    assert!(matches!(error, GitError::Conflicts(_)), "{error:?}");
    assert_eq!(fs::read_to_string(&outside).unwrap(), "precious\n");
    s.write("a.txt", "three\n");
    SystemGit
        .mark_resolved(&s.repo, Path::new("a.txt"))
        .unwrap();
    // Continuing commits the squash, then stops again on "second".
    let operation = SystemGit.info(&s.repo).unwrap().operation.unwrap();
    let _ = SystemGit.continue_operation(&s.repo, operation);
    assert_eq!(
        s.git(&["log", "-1", "--format=%B", "HEAD"]).trim(),
        "first and third, as I wrote it"
    );
    SystemGit
        .abort_operation(&s.repo, git_core::Operation::Rebase)
        .unwrap();
    assert!(
        !s.repo.join(".git/better-git-squash-message").exists(),
        "no message file is left behind"
    );
    assert_eq!(s.subjects(), vec!["third", "second", "first", "base"]);
}

#[test]
fn listing_history_never_runs_the_signature_program() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let marker = s.tmp.path().join("gpg-ran");
    let gpg = s.tmp.path().join("fake-gpg");
    fs::write(
        &gpg,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&gpg, fs::Permissions::from_mode(0o755)).unwrap();
    s.git(&["config", "log.showSignature", "true"]);
    s.git(&["config", "gpg.program", gpg.to_str().unwrap()]);
    // A signature for git to check: the commit object gets a gpgsig header.
    let tree = s.git(&["rev-parse", "HEAD^{tree}"]);
    let body = format!(
        "tree {}\nauthor A <a@b> 0 +0000\ncommitter A <a@b> 0 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n x\n -----END PGP SIGNATURE-----\n\nsigned\n",
        tree.trim()
    );
    let path = s.tmp.path().join("commit.txt");
    fs::write(&path, body).unwrap();
    let sha = s.git(&["hash-object", "-t", "commit", "-w", path.to_str().unwrap()]);
    s.git(&["update-ref", "HEAD", sha.trim()]);
    let commits = SystemGit.log(&s.repo, "HEAD", 0, 10).unwrap();
    assert_eq!(commits[0].summary, "signed");
    assert!(!marker.exists(), "gpg.program ran");
}
