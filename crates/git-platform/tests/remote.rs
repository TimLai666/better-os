//! Fetch, pull, push, remotes, and clone against local bare repositories.

use git_core::{Refusal, classify_refusal};
use git_platform::credentials::Helper;
use git_platform::{
    Cancel, Credentials, Git, GitError, Lease, NetworkOptions, PushRequest, SystemGit, base64,
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

fn options() -> NetworkOptions {
    NetworkOptions::default()
}

/// A push of `branch` to the same name on `origin`, setting the upstream.
fn request(branch: &str, force: Option<Lease>) -> PushRequest {
    PushRequest {
        remote: "origin".into(),
        branch: branch.into(),
        remote_branch: branch.into(),
        set_upstream: true,
        force,
    }
}

fn lease(s: &Sandbox, remote_ref: &str) -> Option<Lease> {
    Some(Lease {
        expected: Some(s.git(&["rev-parse", remote_ref]).trim().to_string()),
    })
}

fn someone_else_pushes(s: &Sandbox, bare: &std::path::Path, file: &str) {
    let other = s.tmp.path().join(format!("other-{file}"));
    git_in(
        s.tmp.path(),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join(file), "theirs\n").unwrap();
    git_in(&other, &["add", file]);
    git_in(&other, &["commit", "-q", "-m", "their work"]);
    git_in(&other, &["push", "-q", "origin", "main"]);
}

#[test]
fn push_a_new_branch_sets_its_upstream_and_pushes_tags() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let bare = s.with_origin();
    s.git(&["switch", "-q", "-c", "topic"]);
    s.commit_file("t.txt", "t\n", "topic");
    s.git(&["tag", "v1"]);
    let with_tag = NetworkOptions {
        tags: vec!["v1".into()],
        ..NetworkOptions::default()
    };
    SystemGit
        .push(&s.repo, &request("topic", None), &with_tag)
        .unwrap();
    assert_eq!(
        s.git(&["rev-parse", "--abbrev-ref", "topic@{upstream}"])
            .trim(),
        "origin/topic"
    );
    assert_eq!(git_in(&bare, &["tag"]).trim(), "v1");
    SystemGit
        .delete_remote_branch(&s.repo, "origin", "topic", &options())
        .unwrap();
    assert!(!git_in(&bare, &["branch"]).contains("topic"));
}

#[test]
fn a_rejected_push_explains_and_pull_then_push_works() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let bare = s.with_origin();
    someone_else_pushes(&s, &bare, "theirs.txt");
    s.commit_file("mine.txt", "mine\n", "my work");
    let error = SystemGit
        .push(&s.repo, &request("main", None), &options())
        .unwrap_err();
    assert_eq!(
        classify_refusal(error.message().unwrap()),
        Refusal::NotFastForward
    );
    SystemGit.pull(&s.repo, &options()).unwrap();
    assert!(s.repo.join("theirs.txt").exists());
    SystemGit
        .push(&s.repo, &request("main", None), &options())
        .unwrap();
    let status = SystemGit.status(&s.repo).unwrap();
    assert_eq!(status.ahead_behind, Some((0, 0)));
}

#[test]
fn force_push_with_lease_overwrites_only_what_was_fetched() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let bare = s.with_origin();
    s.commit_file("b.txt", "b\n", "second");
    SystemGit
        .push(&s.repo, &request("main", None), &options())
        .unwrap();
    s.git(&["commit", "-q", "--amend", "-m", "second, rewritten"]);
    SystemGit
        .push(
            &s.repo,
            &request("main", lease(&s, "origin/main")),
            &options(),
        )
        .unwrap();
    assert_eq!(
        git_in(&bare, &["log", "-1", "--format=%s"]).trim(),
        "second, rewritten"
    );
    // Someone pushes; then a background fetch moves origin/main to their
    // commit before the person, who saw the old one, force-pushes. A lease
    // without an expected value would now overwrite their work.
    let seen = lease(&s, "origin/main");
    someone_else_pushes(&s, &bare, "late.txt");
    SystemGit
        .fetch(&s.repo, Some("origin"), &options())
        .unwrap();
    s.git(&["commit", "-q", "--amend", "-m", "again"]);
    let error = SystemGit
        .push(&s.repo, &request("main", seen), &options())
        .unwrap_err();
    assert!(
        matches!(
            classify_refusal(error.message().unwrap()),
            Refusal::StaleLease | Refusal::NotFastForward
        ),
        "{error:?}"
    );
    assert_eq!(
        git_in(&bare, &["log", "-1", "--format=%s"]).trim(),
        "their work",
        "their commit is still there"
    );
}

#[test]
fn fetch_updates_behind_and_pull_with_local_changes_in_the_way() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let bare = s.with_origin();
    let other = s.other_clone(&bare);
    std::fs::write(other.join("a.txt"), "theirs\n").unwrap();
    git_in(&other, &["commit", "-q", "-am", "theirs"]);
    git_in(&other, &["push", "-q", "origin", "main"]);
    SystemGit
        .fetch(&s.repo, Some("origin"), &options())
        .unwrap();
    assert_eq!(
        SystemGit.status(&s.repo).unwrap().ahead_behind,
        Some((0, 1))
    );
    s.write("a.txt", "mine, uncommitted\n");
    let error = SystemGit.pull(&s.repo, &options()).unwrap_err();
    assert_eq!(
        classify_refusal(error.message().unwrap()),
        Refusal::LocalChangesWouldBeOverwritten
    );
}

#[test]
fn a_missing_remote_is_explained() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.git(&[
        "remote",
        "add",
        "origin",
        s.tmp.path().join("nowhere").to_str().unwrap(),
    ]);
    let error = SystemGit
        .fetch(&s.repo, Some("origin"), &options())
        .unwrap_err();
    assert_eq!(
        classify_refusal(error.message().unwrap()),
        Refusal::RepositoryNotFound
    );
}

#[test]
fn remotes_add_change_and_remove() {
    let s = sandbox();
    SystemGit
        .add_remote(&s.repo, "upstream", "https://example.invalid/a.git")
        .unwrap();
    SystemGit
        .set_remote_url(&s.repo, "upstream", "https://example.invalid/b.git")
        .unwrap();
    let remotes = SystemGit.info(&s.repo).unwrap().remotes;
    assert_eq!(remotes[0].fetch_url, "https://example.invalid/b.git");
    assert!(SystemGit.add_remote(&s.repo, "-bad", "x").is_err());
    assert!(SystemGit.add_remote(&s.repo, "has space", "x").is_err());
    SystemGit.remove_remote(&s.repo, "upstream").unwrap();
    assert!(SystemGit.info(&s.repo).unwrap().remotes.is_empty());
}

#[test]
fn clone_into_a_new_folder_and_refuse_a_full_one() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let bare = s.with_origin();
    let destination = s.tmp.path().join("clones/copy");
    SystemGit
        .clone_repository(bare.to_str().unwrap(), &destination, &options())
        .unwrap();
    assert!(destination.join("a.txt").exists());
    let error = SystemGit
        .clone_repository(bare.to_str().unwrap(), &destination, &options())
        .unwrap_err();
    assert!(matches!(error, GitError::Failed { .. }));
    assert!(
        SystemGit
            .clone_repository("x", std::path::Path::new("relative"), &options())
            .is_err()
    );
}

#[test]
fn the_token_reaches_git_through_a_helper_and_never_the_environment() {
    let _s = sandbox();
    let credentials = Credentials {
        host: "github.com".into(),
        token: "t0k-secret".into(),
    };
    let helper = Helper::new(&credentials).unwrap();
    let args = helper.config_args();
    assert!(args.iter().all(|a| !a.contains("t0k-secret")), "{args:?}");
    let path = helper.path().to_path_buf();
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    // git asks the helper the way a fetch does, and a program it starts
    // (here `env`, standing in for a hook) does not see the token.
    let mut fill = std::process::Command::new("git")
        .args(&args)
        .args(["credential", "fill"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    fill.stdin
        .take()
        .unwrap()
        .write_all(b"protocol=https\nhost=github.com\n\n")
        .unwrap();
    let out = String::from_utf8(fill.wait_with_output().unwrap().stdout).unwrap();
    assert!(out.contains("username=x-access-token\n"), "{out}");
    assert!(out.contains("password=t0k-secret\n"), "{out}");
    let env = std::process::Command::new("git")
        .args(&args)
        .args(["-c", "alias.e=!env", "e"])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&env.stdout).contains("t0k-secret"));
    drop(helper);
    assert!(!path.exists(), "the token's file goes with the command");
    assert!(!path.parent().unwrap().exists());
    assert!(
        Helper::new(&Credentials {
            host: "evil host;rm".into(),
            token: "x".into()
        })
        .is_err()
    );
    assert_eq!(base64(b"ab"), "YWI=");
    let debug = format!("{credentials:?}");
    assert!(!debug.contains("t0k-secret"));
}

#[test]
fn push_goes_to_the_upstream_name_and_keeps_the_upstream_set() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let bare = s.with_origin();
    s.git(&["switch", "-q", "-c", "local-name"]);
    s.git(&["push", "-q", "-u", "origin", "local-name:feature"]);
    s.commit_file("b.txt", "b\n", "more");
    SystemGit
        .push(
            &s.repo,
            &PushRequest {
                remote: "origin".into(),
                branch: "local-name".into(),
                remote_branch: "feature".into(),
                set_upstream: false,
                force: None,
            },
            &options(),
        )
        .unwrap();
    assert_eq!(
        git_in(&bare, &["log", "-1", "--format=%s", "feature"]).trim(),
        "more"
    );
    assert!(!git_in(&bare, &["branch"]).contains("local-name"));
    assert_eq!(
        s.git(&["rev-parse", "--abbrev-ref", "local-name@{upstream}"])
            .trim(),
        "origin/feature"
    );
    SystemGit
        .delete_remote_branch(&s.repo, "origin", "feature", &options())
        .unwrap();
    assert!(!git_in(&bare, &["branch"]).contains("feature"));
}

#[test]
fn push_runs_the_pre_push_hook_that_git_lfs_needs() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.with_origin();
    s.commit_file("b.txt", "b\n", "second");
    let marker = s.tmp.path().join("pre-push-ran");
    let hook = s.repo.join(".git/hooks/pre-push");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    SystemGit
        .push(&s.repo, &request("main", None), &options())
        .unwrap();
    assert!(marker.exists());
}

/// An `ssh` that says nothing and never answers, for the remote.
fn hanging_remote(s: &Sandbox) {
    s.git(&["remote", "add", "origin", "ssh://example.invalid/x.git"]);
    // SAFETY: tests hold `SERIAL`.
    unsafe {
        std::env::set_var("GIT_SSH_COMMAND", "sleep 60 #");
    }
}

fn unhang() {
    // SAFETY: tests hold `SERIAL`.
    unsafe {
        std::env::remove_var("GIT_SSH_COMMAND");
    }
}

#[test]
fn a_fetch_that_stalls_is_stopped() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    hanging_remote(&s);
    let started = std::time::Instant::now();
    let result = SystemGit.fetch(
        &s.repo,
        Some("origin"),
        &NetworkOptions {
            stall: Some(std::time::Duration::from_millis(500)),
            ..options()
        },
    );
    unhang();
    assert_eq!(result, Err(GitError::Stalled));
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

#[test]
fn a_fetch_can_be_cancelled() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    hanging_remote(&s);
    let cancel = Cancel::new();
    let flip = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        flip.cancel();
    });
    let started = std::time::Instant::now();
    let result = SystemGit.fetch(
        &s.repo,
        Some("origin"),
        &NetworkOptions {
            cancel,
            ..options()
        },
    );
    unhang();
    assert_eq!(result, Err(GitError::Cancelled));
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

#[test]
fn ssh_is_told_to_ask_no_one() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.git(&["remote", "add", "origin", "ssh://example.invalid/x.git"]);
    // SAFETY: tests hold `SERIAL`.
    unsafe {
        std::env::set_var(
            "GIT_SSH_COMMAND",
            "echo \"askpass=$SSH_ASKPASS require=$SSH_ASKPASS_REQUIRE\" >&2; exit 1; #",
        );
    }
    let error = SystemGit
        .fetch(&s.repo, Some("origin"), &options())
        .unwrap_err();
    unhang();
    let message = error.message().unwrap_or_default().to_string();
    assert!(message.contains("askpass=false require=force"), "{message}");
}
