//! Conflicts from a merge, a rebase, and a cherry-pick against a real `git`.

use std::path::Path;

use git_core::Operation;
use git_platform::{Git, GitError, HistoryEdit, SystemGit};

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

/// `main` and `topic` both change `a.txt`, and `topic` deletes `d.txt` that
/// `main` changes.
fn diverged(s: &Sandbox) {
    s.write("a.txt", "base\n");
    s.write("d.txt", "base\n");
    s.git(&["add", "."]);
    s.git(&["commit", "-q", "-m", "base"]);
    s.git(&["switch", "-q", "-c", "topic"]);
    s.write("a.txt", "topic\n");
    s.git(&["rm", "-q", "d.txt"]);
    s.git(&["commit", "-q", "-am", "topic"]);
    s.git(&["switch", "-q", "main"]);
    s.write("a.txt", "main\n");
    s.write("d.txt", "main\n");
    s.git(&["commit", "-q", "-am", "main"]);
}

#[test]
fn merge_conflicts_resolved_with_each_side_then_continued() {
    let s = sandbox();
    diverged(&s);
    assert!(matches!(
        SystemGit.merge(&s.repo, "topic", false),
        Err(GitError::Conflicts(_))
    ));
    let status = SystemGit.status(&s.repo).unwrap();
    assert_eq!(status.changes.iter().filter(|c| c.conflicted).count(), 2);
    SystemGit
        .resolve_with(&s.repo, Path::new("a.txt"), false)
        .unwrap();
    assert_eq!(s.read("a.txt"), "topic\n");
    SystemGit
        .resolve_with(&s.repo, Path::new("d.txt"), false)
        .unwrap();
    assert!(!s.repo.join("d.txt").exists(), "theirs deleted it");
    SystemGit
        .continue_operation(&s.repo, Operation::Merge)
        .unwrap();
    assert_eq!(SystemGit.info(&s.repo).unwrap().operation, None);
    assert!(s.subjects()[0].starts_with("Merge branch 'topic'"));
}

#[test]
fn merge_conflict_edited_by_hand_marked_resolved_or_aborted() {
    let s = sandbox();
    diverged(&s);
    SystemGit.merge(&s.repo, "topic", false).unwrap_err();
    s.write("a.txt", "both\n");
    SystemGit
        .mark_resolved(&s.repo, Path::new("a.txt"))
        .unwrap();
    SystemGit
        .resolve_with(&s.repo, Path::new("d.txt"), true)
        .unwrap();
    assert_eq!(s.read("d.txt"), "main\n");
    let status = SystemGit.status(&s.repo).unwrap();
    assert!(status.changes.iter().all(|c| !c.conflicted));
    SystemGit
        .abort_operation(&s.repo, Operation::Merge)
        .unwrap();
    assert_eq!(s.read("a.txt"), "main\n");
    assert_eq!(SystemGit.info(&s.repo).unwrap().operation, None);
}

#[test]
fn rebase_and_cherry_pick_conflicts_continue_and_abort() {
    let s = sandbox();
    diverged(&s);
    s.git(&["switch", "-q", "topic"]);
    assert!(matches!(
        SystemGit.rebase(&s.repo, "main"),
        Err(GitError::Conflicts(_))
    ));
    assert_eq!(
        SystemGit.info(&s.repo).unwrap().operation,
        Some(Operation::Rebase)
    );
    SystemGit
        .resolve_with(&s.repo, Path::new("a.txt"), true)
        .unwrap();
    SystemGit
        .mark_resolved(&s.repo, Path::new("d.txt"))
        .unwrap();
    SystemGit
        .continue_operation(&s.repo, Operation::Rebase)
        .unwrap();
    assert_eq!(SystemGit.info(&s.repo).unwrap().operation, None);
    drop(s);

    let s = sandbox();
    diverged(&s);
    let topic = s.git(&["rev-parse", "topic"]).trim().to_string();
    let error = SystemGit
        .edit_history(
            &s.repo,
            &HistoryEdit::CherryPick {
                shas: vec![topic],
                onto: None,
            },
        )
        .unwrap_err();
    assert!(matches!(error, GitError::Conflicts(_)));
    assert_eq!(
        SystemGit.info(&s.repo).unwrap().operation,
        Some(Operation::CherryPick)
    );
    SystemGit
        .abort_operation(&s.repo, Operation::CherryPick)
        .unwrap();
    assert_eq!(SystemGit.info(&s.repo).unwrap().operation, None);
    assert_eq!(s.subjects()[0], "main");
}

// --- independent review fixes -------------------------------------------

#[test]
fn a_squash_merge_that_conflicts_can_be_continued_and_aborted() {
    let s = sandbox();
    diverged(&s);
    assert!(matches!(
        SystemGit.merge(&s.repo, "topic", true),
        Err(GitError::Conflicts(_))
    ));
    // git keeps no MERGE_HEAD for a squash merge; the window still knows.
    assert_eq!(
        SystemGit.info(&s.repo).unwrap().operation,
        Some(Operation::SquashMerge)
    );
    // An unrelated stash must survive whatever happens next.
    SystemGit
        .abort_operation(&s.repo, Operation::SquashMerge)
        .unwrap();
    assert_eq!(s.read("a.txt"), "main\n");
    assert_eq!(SystemGit.info(&s.repo).unwrap().operation, None);
    assert_eq!(s.porcelain(), "");

    SystemGit.merge(&s.repo, "topic", true).unwrap_err();
    SystemGit
        .resolve_with(&s.repo, Path::new("a.txt"), false)
        .unwrap();
    SystemGit
        .resolve_with(&s.repo, Path::new("d.txt"), false)
        .unwrap();
    SystemGit
        .continue_operation(&s.repo, Operation::SquashMerge)
        .unwrap();
    assert_eq!(SystemGit.info(&s.repo).unwrap().operation, None);
    let subject = s.subjects()[0].clone();
    assert!(subject.starts_with("Squashed commit"), "{subject}");
    assert_eq!(s.read("a.txt"), "topic\n");
    assert_eq!(
        s.git(&["rev-list", "--parents", "-1", "HEAD"])
            .split_whitespace()
            .count(),
        2,
        "a squash makes an ordinary commit"
    );
}

#[test]
fn the_sides_of_a_conflict_are_named_for_each_operation() {
    let s = sandbox();
    diverged(&s);
    SystemGit.merge(&s.repo, "topic", false).unwrap_err();
    let sides = SystemGit.info(&s.repo).unwrap().sides.unwrap();
    assert_eq!(
        (sides.ours.as_str(), sides.theirs.as_str()),
        ("main", "topic")
    );
    SystemGit
        .abort_operation(&s.repo, Operation::Merge)
        .unwrap();

    // In a rebase of topic onto main, "ours" is main's commit and
    // "theirs" is topic's: the other way round from a merge.
    s.git(&["switch", "-q", "topic"]);
    SystemGit.rebase(&s.repo, "main").unwrap_err();
    let main = s.git(&["rev-parse", "--short", "main"]).trim().to_string();
    let sides = SystemGit.info(&s.repo).unwrap().sides.unwrap();
    assert_eq!(sides.ours, main);
    assert_eq!(sides.theirs, "topic");
    SystemGit
        .resolve_with(&s.repo, Path::new("a.txt"), false)
        .unwrap();
    assert_eq!(s.read("a.txt"), "topic\n", "theirs is topic's version");
}
