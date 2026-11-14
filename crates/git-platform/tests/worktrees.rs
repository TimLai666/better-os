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

/// Ticket 116: lock with and without a reason, unlock, and Remove refused
/// on a locked worktree (one `--force` does not override a lock).
#[test]
fn lock_unlock_and_a_locked_worktree_is_not_removed() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let one = s.tmp.path().join("one");
    let two = s.tmp.path().join("two");
    SystemGit.add_worktree(&s.repo, &one, "one", true).unwrap();
    SystemGit.add_worktree(&s.repo, &two, "two", true).unwrap();
    SystemGit
        .lock_worktree(&s.repo, &one, Some("on a USB stick"))
        .unwrap();
    SystemGit.lock_worktree(&s.repo, &two, None).unwrap();
    let list = SystemGit.info(&s.repo).unwrap().worktrees;
    assert!(list[1].locked && list[2].locked);
    assert_eq!(list[1].lock_reason.as_deref(), Some("on a USB stick"));
    assert_eq!(list[2].lock_reason, None);
    // A blank reason is no reason.
    SystemGit.unlock_worktree(&s.repo, &two).unwrap();
    SystemGit.lock_worktree(&s.repo, &two, Some("  ")).unwrap();
    assert_eq!(
        SystemGit.info(&s.repo).unwrap().worktrees[2].lock_reason,
        None
    );

    for force in [false, true] {
        let refused = SystemGit.remove_worktree(&s.repo, &one, force).unwrap_err();
        assert!(
            format!("{refused:?}").contains("locked"),
            "force {force}: {refused:?}"
        );
        assert!(one.exists());
    }
    SystemGit.unlock_worktree(&s.repo, &one).unwrap();
    assert!(!SystemGit.info(&s.repo).unwrap().worktrees[1].locked);
    SystemGit.remove_worktree(&s.repo, &one, false).unwrap();
    assert!(!one.exists());
    assert!(
        SystemGit
            .lock_worktree(&s.repo, std::path::Path::new("rel"), None)
            .is_err()
    );
}

/// Ticket 116: prune clears every worktree whose folder is gone, and
/// leaves a locked one and one that is there.
#[test]
fn prune_clears_worktrees_whose_folder_is_gone() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let [gone, also_gone, kept, here] =
        ["gone", "also-gone", "kept", "here"].map(|n| s.tmp.path().join(n));
    for (path, branch) in [(&gone, "g"), (&also_gone, "a"), (&kept, "k"), (&here, "h")] {
        SystemGit.add_worktree(&s.repo, path, branch, true).unwrap();
    }
    SystemGit
        .lock_worktree(&s.repo, &kept, Some("away"))
        .unwrap();
    for path in [&gone, &also_gone, &kept] {
        std::fs::remove_dir_all(path).unwrap();
    }
    let before = SystemGit.info(&s.repo).unwrap().worktrees;
    assert_eq!(before.iter().filter(|w| w.prunable).count(), 2);
    SystemGit.prune_worktrees(&s.repo).unwrap();
    let mut after: Vec<String> = SystemGit
        .info(&s.repo)
        .unwrap()
        .worktrees
        .iter()
        .skip(1)
        .map(|w| w.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    after.sort();
    assert_eq!(after, ["here", "kept"]);
}
