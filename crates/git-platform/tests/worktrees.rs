//! Worktrees against a real `git`.

use git_platform::{Git, SystemGit};

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
fn add_rename_and_remove_worktrees() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let wt = s.tmp.path().join("wt one");
    SystemGit
        .add_worktree(&s.repo, &wt, "feature", true)
        .unwrap();
    assert!(wt.join("a.txt").exists());
    let list = SystemGit.info(&s.repo).unwrap().worktrees;
    assert_eq!(list.len(), 2);
    assert!(list[0].main);
    assert_eq!(list[1].branch.as_deref(), Some("feature"));
    let moved = s.tmp.path().join("wt-two");
    SystemGit.move_worktree(&s.repo, &wt, &moved).unwrap();
    assert!(moved.join("a.txt").exists() && !wt.exists());
    std::fs::write(moved.join("dirty.txt"), "x").unwrap();
    assert!(
        SystemGit.remove_worktree(&s.repo, &moved, false).is_err(),
        "a worktree with changes needs force"
    );
    SystemGit.remove_worktree(&s.repo, &moved, true).unwrap();
    assert_eq!(SystemGit.info(&s.repo).unwrap().worktrees.len(), 1);
    assert!(
        SystemGit
            .add_worktree(&s.repo, std::path::Path::new("relative"), "x", true)
            .is_err()
    );
    let existing = s.tmp.path().join("wt-existing");
    s.git(&["branch", "existing"]);
    SystemGit
        .add_worktree(&s.repo, &existing, "existing", false)
        .unwrap();
    assert_eq!(SystemGit.info(&s.repo).unwrap().worktrees.len(), 2);
}

/// `core.worktree` says where a submodule's working tree is; a plain
/// repository has none.
#[test]
fn core_worktree_names_a_submodules_working_tree() {
    let s = sandbox();
    git_in(&s.repo, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let sup = s.tmp.path().join("sup");
    std::fs::create_dir_all(&sup).unwrap();
    git_in(&sup, &["init", "-q", "-b", "main"]);
    git_in(
        &sup,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            s.repo.to_str().unwrap(),
            "sub",
        ],
    );
    let modules = sup.join(".git/modules/sub");
    let found = SystemGit.core_worktree(&modules).unwrap().unwrap();
    assert_eq!(
        found.canonicalize().unwrap(),
        sup.join("sub").canonicalize().unwrap()
    );
    assert_eq!(SystemGit.core_worktree(&s.repo.join(".git")).unwrap(), None);
}
