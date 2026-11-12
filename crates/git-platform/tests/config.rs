//! The global git configuration and new repositories. `GIT_CONFIG_GLOBAL`
//! points at a file in the temporary home, never `~/.gitconfig`.

use git_platform::{Git, NewRepository, SystemGit};

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
fn read_and_write_the_global_name_email_and_default_branch() {
    let s = sandbox();
    let file = s.home.join("gitconfig");
    // SAFETY: the test holds `SERIAL`.
    unsafe { std::env::set_var("GIT_CONFIG_GLOBAL", &file) };
    assert_eq!(SystemGit.global_config("user.name").unwrap(), None);
    SystemGit
        .set_global_config("user.name", "Ada Lovelace")
        .unwrap();
    SystemGit
        .set_global_config("user.email", "ada@example.com")
        .unwrap();
    SystemGit
        .set_global_config("init.defaultBranch", "trunk")
        .unwrap();
    assert_eq!(
        SystemGit.global_config("user.name").unwrap().as_deref(),
        Some("Ada Lovelace")
    );
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("defaultBranch = trunk")
    );
    SystemGit.set_global_config("user.name", "").unwrap();
    SystemGit.set_global_config("user.name", "").unwrap();
    assert_eq!(SystemGit.global_config("user.name").unwrap(), None);
    assert!(
        SystemGit.set_global_config("core.editor", "x").is_err(),
        "only the settings page's keys"
    );
    assert!(SystemGit.global_config("core.hooksPath").is_err());
}

#[test]
fn init_makes_the_files_and_a_first_commit() {
    let s = sandbox();
    let path = s.tmp.path().join("new project");
    SystemGit
        .init_repository(&NewRepository {
            path: path.clone(),
            default_branch: "trunk".into(),
            readme: Some("# new project\n".into()),
            gitignore: Some("target/\n".into()),
            license: Some("MIT\n".into()),
        })
        .unwrap();
    assert_eq!(git_in(&path, &["branch", "--show-current"]).trim(), "trunk");
    assert_eq!(
        git_in(&path, &["log", "--format=%s"]).trim(),
        "Initial commit"
    );
    assert_eq!(git_in(&path, &["ls-files"]).lines().count(), 3);
    assert!(
        SystemGit
            .init_repository(&NewRepository {
                path: path.clone(),
                ..Default::default()
            })
            .is_err()
    );
    let bare = s.tmp.path().join("empty");
    SystemGit
        .init_repository(&NewRepository {
            path: bare.clone(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(git_in(&bare, &["branch", "--show-current"]).trim(), "main");
}
