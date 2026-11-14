//! What keeps Better Git's window current: whether Git knows who is
//! committing, the repository's own directories, and the directories Git
//! ignores. `HOME` and the global configuration point into a temporary
//! directory; the person's own `~/.gitconfig` is never read.

use std::path::PathBuf;

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

use harness::sandbox;

#[test]
fn git_without_a_name_or_email_is_an_unknown_identity_not_an_error() {
    let s = sandbox();
    let git = SystemGit;
    assert_eq!(git.identity_known(&s.repo), Ok(true));
    // SAFETY: the sandbox holds `SERIAL`.
    unsafe {
        for key in [
            "GIT_AUTHOR_NAME",
            "GIT_COMMITTER_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_EMAIL",
        ] {
            std::env::remove_var(key);
        }
        // No name or email anywhere, and no guessing one from the host.
        std::env::set_var("GIT_CONFIG_GLOBAL", s.home.join("gitconfig"));
    }
    s.git(&["config", "--global", "user.useConfigOnly", "true"]);
    assert_eq!(git.identity_known(&s.repo), Ok(false));
    s.git(&["config", "--global", "user.name", "Ada"]);
    assert_eq!(git.identity_known(&s.repo), Ok(false));
    s.git(&["config", "--global", "user.email", "ada@example.com"]);
    assert_eq!(git.identity_known(&s.repo), Ok(true));
    // Outside any repository, as before a new repository is made.
    assert_eq!(git.identity_known(std::path::Path::new("/")), Ok(true));
}

#[test]
fn a_linked_worktree_has_its_own_git_directory_and_shares_the_common_one() {
    let s = sandbox();
    let git = SystemGit;
    s.commit_file("a.txt", "a", "first");
    let (git_dir, common) = git.git_dirs(&s.repo).unwrap();
    assert_eq!(git_dir, s.repo.canonicalize().unwrap().join(".git"));
    assert_eq!(git_dir, common);

    let linked = s.tmp.path().join("linked");
    s.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "side",
        linked.to_str().unwrap(),
    ]);
    let (linked_dir, linked_common) = git.git_dirs(&linked).unwrap();
    assert_eq!(linked_common, common);
    assert!(linked_dir.starts_with(common.join("worktrees")));

    let listed = git.worktrees(&s.repo).unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed[0].main);
    assert_eq!(listed[1].branch.as_deref(), Some("side"));
}

#[test]
fn ignored_directories_are_listed_once_at_their_top() {
    let s = sandbox();
    let git = SystemGit;
    s.write(".gitignore", "node_modules/\ntarget/\n*.log\n");
    s.write("node_modules/pkg/index.js", "x");
    s.write("target/debug/build/out", "x");
    s.write("src/main.rs", "fn main() {}");
    s.write("debug.log", "x");
    let mut found = git.ignored_directories(&s.repo).unwrap();
    found.sort();
    assert_eq!(
        found,
        vec![PathBuf::from("node_modules"), PathBuf::from("target")]
    );
}

#[test]
fn ignored_files_are_told_apart_from_tracked_and_new_ones_in_one_call() {
    let s = sandbox();
    let git = SystemGit;
    s.commit_file("kept.log", "tracked although it matches", "track a log");
    s.write(".gitignore", "*.log\n");
    s.write("debug.log", "x");
    s.write("notes.txt", "x");
    let asked: Vec<PathBuf> = ["debug.log", "notes.txt", "kept.log"]
        .iter()
        .map(|p| s.repo.join(p))
        .collect();
    let ignored = git.ignored_paths(&s.repo, &asked).unwrap();
    assert_eq!(ignored, vec![s.repo.join("debug.log")]);
    assert_eq!(
        git.ignored_paths(&s.repo, &[]).unwrap(),
        Vec::<PathBuf>::new()
    );
}
