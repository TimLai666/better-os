//! Branches against a real `git`.

use git_platform::{Git, GitError, SwitchChanges, SystemGit};

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

fn current(s: &Sandbox) -> String {
    s.git(&["branch", "--show-current"]).trim().to_string()
}

#[test]
fn create_from_a_commit_switch_rename_and_delete() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let first = s.head();
    s.commit_file("b.txt", "b\n", "second");
    SystemGit
        .create_branch(&s.repo, "from-first", Some(&first), true)
        .unwrap();
    assert_eq!(current(&s), "from-first");
    assert_eq!(s.head(), first);
    SystemGit
        .rename_branch(&s.repo, "from-first", "renamed")
        .unwrap();
    assert_eq!(current(&s), "renamed");
    assert!(
        SystemGit
            .create_branch(&s.repo, "-bad", None, false)
            .is_err()
    );
    SystemGit
        .switch(&s.repo, "main", SwitchChanges::Carry)
        .unwrap();
    SystemGit.delete_branch(&s.repo, "renamed", false).unwrap();
    let info = SystemGit.info(&s.repo).unwrap();
    assert_eq!(
        info.branches
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
        ["main"]
    );
}

#[test]
fn delete_refuses_an_unmerged_branch_unless_forced() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    SystemGit
        .create_branch(&s.repo, "topic", None, true)
        .unwrap();
    s.commit_file("t.txt", "t\n", "topic work");
    SystemGit
        .switch(&s.repo, "main", SwitchChanges::Carry)
        .unwrap();
    assert!(matches!(
        SystemGit.delete_branch(&s.repo, "topic", false),
        Err(GitError::Failed { .. })
    ));
    SystemGit.delete_branch(&s.repo, "topic", true).unwrap();
}

#[test]
fn switch_leaving_changes_stashes_them_for_the_branch() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    SystemGit
        .create_branch(&s.repo, "other", None, false)
        .unwrap();
    s.write("a.txt", "edited\n");
    s.write("new.txt", "new\n");
    SystemGit
        .switch(&s.repo, "other", SwitchChanges::Leave)
        .unwrap();
    assert_eq!(current(&s), "other");
    assert_eq!(s.porcelain(), "");
    let info = SystemGit.info(&s.repo).unwrap();
    assert_eq!(info.stashes.len(), 1);
    assert_eq!(info.stashes[0].branch(), Some("main"));
}

#[test]
fn switch_bringing_changes_carries_them_over() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    SystemGit
        .create_branch(&s.repo, "other", None, false)
        .unwrap();
    s.write("a.txt", "edited\n");
    s.write("new.txt", "new\n");
    SystemGit
        .switch(&s.repo, "other", SwitchChanges::Bring)
        .unwrap();
    assert_eq!(current(&s), "other");
    assert_eq!(s.read("a.txt"), "edited\n");
    assert!(s.repo.join("new.txt").exists());
    assert!(SystemGit.info(&s.repo).unwrap().stashes.is_empty());
}

#[test]
fn switch_to_a_commit_detaches_and_to_a_remote_branch_tracks_it() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let first = s.head();
    let bare = s.with_origin();
    let other = s.other_clone(&bare);
    git_in(&other, &["switch", "-q", "-c", "feature"]);
    std::fs::write(other.join("f.txt"), "f\n").unwrap();
    git_in(&other, &["add", "f.txt"]);
    git_in(&other, &["commit", "-q", "-m", "feature"]);
    git_in(&other, &["push", "-q", "origin", "feature"]);
    SystemGit.fetch(&s.repo, None, &Default::default()).unwrap();
    SystemGit
        .switch(&s.repo, "origin/feature", SwitchChanges::Carry)
        .unwrap();
    assert_eq!(current(&s), "feature");
    let info = SystemGit.info(&s.repo).unwrap();
    let feature = info.branches.iter().find(|b| b.name == "feature").unwrap();
    assert_eq!(feature.upstream.as_deref(), Some("origin/feature"));
    SystemGit
        .switch(&s.repo, &first, SwitchChanges::Carry)
        .unwrap();
    assert_eq!(current(&s), "", "detached");
}

#[test]
fn merge_rebase_and_compare() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "base");
    SystemGit
        .create_branch(&s.repo, "topic", None, true)
        .unwrap();
    s.commit_file("t.txt", "t\n", "topic one");
    s.commit_file("t2.txt", "t\n", "topic two");
    SystemGit
        .switch(&s.repo, "main", SwitchChanges::Carry)
        .unwrap();
    s.commit_file("m.txt", "m\n", "main work");
    let (ahead, behind) = SystemGit.compare(&s.repo, "main", "topic").unwrap();
    assert_eq!(ahead.len(), 2);
    assert_eq!(behind, 1);
    SystemGit
        .switch(&s.repo, "topic", SwitchChanges::Carry)
        .unwrap();
    SystemGit.rebase(&s.repo, "main").unwrap();
    assert_eq!(
        s.subjects(),
        vec!["topic two", "topic one", "main work", "base"]
    );
    SystemGit
        .switch(&s.repo, "main", SwitchChanges::Carry)
        .unwrap();
    SystemGit.merge(&s.repo, "topic", false).unwrap();
    assert_eq!(s.subjects()[0], "topic two", "fast-forward");
}

#[test]
fn merge_conflict_is_reported_as_conflicts() {
    let s = sandbox();
    s.commit_file("a.txt", "base\n", "base");
    SystemGit
        .create_branch(&s.repo, "topic", None, true)
        .unwrap();
    s.commit_file("a.txt", "topic\n", "topic");
    SystemGit
        .switch(&s.repo, "main", SwitchChanges::Carry)
        .unwrap();
    s.commit_file("a.txt", "main\n", "main");
    let error = SystemGit.merge(&s.repo, "topic", false).unwrap_err();
    assert!(matches!(error, GitError::Conflicts(_)));
    let info = SystemGit.info(&s.repo).unwrap();
    assert_eq!(info.operation, Some(git_core::Operation::Merge));
}

// --- independent review fixes -------------------------------------------

/// A submodule at `sub` with a change inside it that `git stash` cannot
/// stash: `git status` reports the repository as changed, yet `git stash
/// push` makes no stash.
fn dirty_submodule(s: &Sandbox) {
    let inner = s.tmp.path().join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    git_in(&inner, &["init", "-q", "-b", "main"]);
    std::fs::write(inner.join("x.txt"), "x\n").unwrap();
    git_in(&inner, &["add", "x.txt"]);
    git_in(&inner, &["commit", "-q", "-m", "inner"]);
    s.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        inner.to_str().unwrap(),
        "sub",
    ]);
    s.git(&["commit", "-q", "-m", "submodule"]);
    std::fs::write(s.repo.join("sub/x.txt"), "changed inside\n").unwrap();
}

#[test]
fn switching_with_nothing_stashable_never_pops_an_older_stash() {
    for changes in [SwitchChanges::Bring, SwitchChanges::Leave] {
        let s = sandbox();
        s.commit_file("a.txt", "a\n", "first");
        // An older stash of the person's own, unrelated to the switch.
        s.write("a.txt", "kept in an old stash\n");
        s.git(&["stash", "push", "-q", "-m", "an older stash"]);
        dirty_submodule(&s);
        SystemGit
            .create_branch(&s.repo, "other", None, false)
            .unwrap();
        SystemGit.switch(&s.repo, "other", changes).unwrap();
        assert_eq!(current(&s), "other");
        assert_eq!(
            s.read("a.txt"),
            "a\n",
            "{changes:?}: the old stash was popped"
        );
        let stashes = SystemGit.info(&s.repo).unwrap().stashes;
        assert_eq!(stashes.len(), 1, "{changes:?}");
    }
}
