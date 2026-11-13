//! Stashes against a real `git`.

use std::path::PathBuf;

use git_core::stash::stash_message;
use git_platform::{Git, GitError, SystemGit};

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

#[test]
fn stash_all_shows_files_and_diffs_then_pops() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.write("a.txt", "edited\n");
    s.write("new.txt", "new\n");
    SystemGit
        .stash_all(&s.repo, &stash_message("main"))
        .unwrap();
    assert_eq!(s.porcelain(), "");
    let stash = SystemGit.info(&s.repo).unwrap().stashes.remove(0);
    assert_eq!(stash.branch(), Some("main"));
    let mut files = SystemGit.stash_files(&s.repo, &stash.name).unwrap();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
        [PathBuf::from("a.txt"), PathBuf::from("new.txt")]
    );
    for file in &files {
        let diff = SystemGit.stash_diff(&s.repo, &stash.name, file).unwrap();
        let git_core::FileDiff::Text { hunks } = diff else {
            panic!("binary")
        };
        assert!(!hunks.is_empty(), "{:?}", file.path);
    }
    SystemGit.stash_pop(&s.repo, &stash.name).unwrap();
    assert_eq!(s.read("a.txt"), "edited\n");
    assert!(SystemGit.info(&s.repo).unwrap().stashes.is_empty());
}

#[test]
fn drop_discards_a_stash() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.write("a.txt", "edited\n");
    SystemGit
        .stash_all(&s.repo, &stash_message("main"))
        .unwrap();
    SystemGit.stash_drop(&s.repo, "stash@{0}").unwrap();
    assert!(SystemGit.info(&s.repo).unwrap().stashes.is_empty());
    assert_eq!(s.read("a.txt"), "a\n");
}

#[test]
fn a_conflicting_pop_keeps_the_stash_and_can_be_finished_or_undone() {
    for finish in [true, false] {
        let s = sandbox();
        s.commit_file("a.txt", "a\n", "first");
        s.write("a.txt", "stashed\n");
        SystemGit
            .stash_all(&s.repo, &stash_message("main"))
            .unwrap();
        s.commit_file("a.txt", "committed\n", "second");
        let error = SystemGit.stash_pop(&s.repo, "stash@{0}").unwrap_err();
        assert!(matches!(error, GitError::Conflicts(_)), "{error:?}");
        assert_eq!(SystemGit.info(&s.repo).unwrap().stashes.len(), 1);
        if finish {
            s.write("a.txt", "resolved\n");
            SystemGit
                .mark_resolved(&s.repo, std::path::Path::new("a.txt"))
                .unwrap();
            SystemGit
                .finish_stash_conflict(&s.repo, "stash@{0}")
                .unwrap();
            assert_eq!(s.porcelain(), " M a.txt\n");
            assert!(SystemGit.info(&s.repo).unwrap().stashes.is_empty());
        } else {
            SystemGit.abort_stash_conflict(&s.repo).unwrap();
            assert_eq!(s.read("a.txt"), "committed\n");
            assert_eq!(SystemGit.info(&s.repo).unwrap().stashes.len(), 1);
        }
    }
}

#[test]
fn a_repository_with_no_commit_has_no_stashes() {
    let s = sandbox();
    assert!(git_platform::stash::list(&s.repo).unwrap().is_empty());
}
