//! The session against a real `git`, in repositories made in temporary
//! directories, driven the way the window drives it. `HOME`, the XDG
//! directories, and the global git configuration point into the temporary
//! directory; nothing reaches the person's own files or GitHub.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use better_ui::fluent::toggle::CheckState;
use git_core::{FileDiff, Operation};
use git_gui::changes::Badge;
use git_gui::dialogs::{BranchPurpose, Dialog};
use git_gui::session::{DiffContent, Files, GitSession, Notice, OpKind, Problem, Tab};
use git_platform::{GitError, SystemGit};

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

fn files(s: &Sandbox) -> Files {
    let state = s.home.join("state/better-os");
    Files {
        settings: Some(s.home.join("config/better-os/git.json")),
        repositories: Some(state.join("git-repositories.json")),
        recent: Some(state.join("git-recent.json")),
        trust: Some(state.join("git-trusted")),
    }
}

/// A session with the repository trusted and opened.
fn opened(s: &Sandbox) -> GitSession {
    let mut session = GitSession::new(Arc::new(SystemGit), files(s));
    session.open(s.repo.clone());
    assert!(
        matches!(session.dialog, Some(Dialog::Trust { .. })),
        "the first open asks"
    );
    session.trust_and_open(s.repo.clone());
    session.run_git_jobs();
    assert!(session.repo.is_some(), "{:?}", session.problem);
    session
}

fn badge(session: &GitSession, path: &str) -> Badge {
    session
        .repo
        .as_ref()
        .unwrap()
        .rows()
        .into_iter()
        .find(|r| r.path == Path::new(path))
        .unwrap_or_else(|| panic!("{path} not listed"))
        .badge
}

const BASE: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n";

#[test]
fn trust_is_asked_once_per_path_and_remembered() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    drop(opened(&s));
    let mut again = GitSession::new(Arc::new(SystemGit), files(&s));
    again.open(s.repo.clone());
    assert!(again.dialog.is_none(), "trusted before");
    again.run_git_jobs();
    assert!(again.repo.is_some());
    assert_eq!(again.repositories.entries.len(), 1);
    assert_eq!(again.recent, vec![s.repo.clone()]);
}

#[test]
fn the_changes_list_matches_git_status_against_head() {
    let s = sandbox();
    s.commit_file("mod.txt", "m\n", "base");
    s.commit_file("del.txt", "d\n", "del");
    s.commit_file("ren.txt", "r\n", "ren");
    s.write("mod.txt", "changed\n");
    std::fs::remove_file(s.repo.join("del.txt")).unwrap();
    s.git(&["mv", "ren.txt", "renamed.txt"]);
    s.write("sp ace 中.txt", "new\n");
    s.write("-dash.txt", "new\n");
    let session = opened(&s);
    assert_eq!(badge(&session, "mod.txt"), Badge::Modified);
    assert_eq!(badge(&session, "del.txt"), Badge::Deleted);
    assert_eq!(badge(&session, "renamed.txt"), Badge::Renamed);
    assert_eq!(badge(&session, "sp ace 中.txt"), Badge::Added);
    assert_eq!(badge(&session, "-dash.txt"), Badge::Added);
    let rows = session.repo.as_ref().unwrap().rows();
    assert!(
        rows.iter().all(|r| r.check == CheckState::Checked),
        "everything is in by default"
    );
}

#[test]
fn ticking_one_hunk_commits_only_that_hunk() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write(
        "f.txt",
        &BASE.replace("two", "TWO").replace("eleven", "ELEVEN"),
    );
    s.write("other.txt", "other\n");
    let mut session = opened(&s);
    session.select_file(Path::new("f.txt"));
    session.run_git_jobs();
    assert!(session.lines_selectable());
    session.toggle_hunk(1);
    session.toggle_file(Path::new("other.txt"));
    let rows = session.repo.as_ref().unwrap().rows();
    assert_eq!(
        rows.iter()
            .find(|r| r.path == Path::new("f.txt"))
            .unwrap()
            .check,
        CheckState::Indeterminate
    );
    session.repo.as_mut().unwrap().form.summary = "Only two".into();
    assert!(session.can_commit());
    session.commit();
    session.run_git_jobs();
    assert_eq!(s.git(&["show", "HEAD:f.txt"]), BASE.replace("two", "TWO"));
    assert!(
        s.git(&["show", "--stat", "HEAD"])
            .lines()
            .all(|l| !l.contains("other.txt"))
    );
    let repo = session.repo.as_ref().unwrap();
    assert_eq!(repo.form.summary, "", "the form empties");
    assert_eq!(repo.status.changes.len(), 2, "the rest is still there");
    assert!(matches!(session.notice, Some(Notice::Done(OpKind::Commit))));
}

#[test]
fn hiding_whitespace_stops_line_selection() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write(
        "f.txt",
        &BASE.replace("two", "two  ").replace("eleven", "ELEVEN"),
    );
    let mut session = opened(&s);
    session.set_hide_whitespace(true);
    session.select_file(Path::new("f.txt"));
    session.run_git_jobs();
    assert!(!session.lines_selectable());
    let Some(DiffContent::Text(FileDiff::Text { hunks })) = session
        .repo
        .as_ref()
        .unwrap()
        .diff
        .as_ref()
        .map(|d| d.content.clone())
    else {
        panic!()
    };
    assert_eq!(hunks.len(), 1, "the whitespace-only hunk is hidden");
}

#[test]
fn discard_asks_then_keeps_a_copy_in_the_trash() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write("f.txt", "gone\n");
    let mut session = opened(&s);
    session.ask_discard(vec![PathBuf::from("f.txt")]);
    assert!(matches!(session.dialog, Some(Dialog::DiscardFiles { .. })));
    session.discard(vec![PathBuf::from("f.txt")]);
    session.run_git_jobs();
    assert_eq!(s.read("f.txt"), BASE);
    assert_eq!(
        std::fs::read_to_string(s.home.join("data/Trash/files/f.txt")).unwrap(),
        "gone\n"
    );
}

#[test]
fn a_path_that_is_not_a_repository_opens_nothing_and_git_missing_is_reported() {
    let s = sandbox();
    let plain = s.tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let mut session = GitSession::new(Arc::new(SystemGit), files(&s));
    session.trust_and_open(plain.clone());
    session.run_git_jobs();
    assert!(session.repo.is_none());
    assert!(
        matches!(&session.problem, Some(Problem::Git { error: GitError::NotARepository(p), .. }) if *p == plain)
    );
    let path = std::env::var_os("PATH");
    // SAFETY: the test holds `SERIAL`.
    unsafe { std::env::set_var("PATH", "/nonexistent") };
    session.trust_and_open(s.repo.clone());
    session.run_git_jobs();
    unsafe { std::env::set_var("PATH", path.unwrap()) };
    assert!(matches!(
        &session.problem,
        Some(Problem::Git {
            error: GitError::NotInstalled,
            ..
        })
    ));
}

#[test]
fn a_repository_with_no_commit_commits_its_first() {
    let s = sandbox();
    s.write("first.txt", "1\n");
    let mut session = opened(&s);
    assert!(session.repo.as_ref().unwrap().status.unborn);
    session.repo.as_mut().unwrap().form.summary = "First".into();
    session.commit();
    session.run_git_jobs();
    assert_eq!(s.subjects(), vec!["First"]);
}

#[test]
fn history_undo_puts_the_message_back_and_amend_replaces() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.write("b.txt", "b\n");
    s.git(&["add", "b.txt"]);
    s.git(&[
        "commit",
        "-q",
        "-m",
        "second",
        "-m",
        "Body\n\nCo-authored-by: Ada <ada@example.invalid>",
    ]);
    let mut session = opened(&s);
    session.set_tab(Tab::History);
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().history.commits.len(), 2);
    session.select_commit(0);
    session.run_git_jobs();
    let repo = session.repo.as_ref().unwrap();
    assert_eq!(repo.history.files.len(), 1);
    assert!(repo.diff.is_some(), "the first file's diff is shown");
    session.ask_undo_commit();
    assert_eq!(session.dialog, Some(Dialog::UndoCommit));
    session.undo_commit();
    session.run_git_jobs();
    let form = &session.repo.as_ref().unwrap().form;
    assert_eq!(
        (form.summary.as_str(), form.description.as_str()),
        ("second", "Body")
    );
    assert!(form.show_co_authors && form.co_authors == "Ada <ada@example.invalid>");
    assert_eq!(s.subjects(), vec!["first"]);
    session.commit();
    session.run_git_jobs();
    assert_eq!(
        s.git(&["log", "-1", "--format=%B"]).trim_end(),
        "second\n\nBody\n\nCo-authored-by: Ada <ada@example.invalid>"
    );
    session.set_tab(Tab::History);
    session.run_git_jobs();
    session.amend_last();
    session.repo.as_mut().unwrap().form.summary = "second, amended".into();
    session.commit();
    session.run_git_jobs();
    assert_eq!(s.subjects(), vec!["second, amended", "first"]);
}

#[test]
fn reorder_squash_revert_tag_and_cherry_pick_from_the_session() {
    let s = sandbox();
    for name in ["a", "b", "c"] {
        s.commit_file(&format!("{name}.txt"), "x\n", name);
    }
    s.git(&["branch", "other", "HEAD~2"]);
    let mut session = opened(&s);
    session.set_tab(Tab::History);
    session.run_git_jobs();
    assert!(
        session.can_rewrite(2),
        "no upstream: every commit may be rewritten"
    );
    session.reorder(0, 1); // c goes just before (newer than) b: nothing moves
    session.run_git_jobs();
    session.reorder(1, 0); // b becomes newest
    session.run_git_jobs();
    assert_eq!(s.subjects(), vec!["b", "c", "a"]);
    session.run_git_jobs();
    session.ask_squash(0, 1);
    assert_eq!(session.dialog, Some(Dialog::Squash { from: 0, to: 1 }));
    session.squash(0, 1, "c and b");
    session.run_git_jobs();
    assert_eq!(s.subjects(), vec!["c and b", "a"]);
    session.run_git_jobs();
    let head = s.head();
    session.create_tag(head.clone(), "v 1");
    session.run_git_jobs();
    assert_eq!(s.git(&["tag"]).trim(), "v-1");
    assert_eq!(session.repo.as_ref().unwrap().unpushed_tags, vec!["v-1"]);
    session.revert(0);
    session.run_git_jobs();
    assert!(s.subjects()[0].starts_with("Revert"));
    session.run_git_jobs();
    session.cherry_pick(1, Some("other".into()));
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().branch(), Some("other"));
    assert_eq!(s.subjects()[0], "c and b");
}

#[test]
fn branches_switch_leaving_changes_then_restore_the_stash() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let mut session = opened(&s);
    session.create_branch("my feature", None);
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().branch(), Some("my-feature"));
    s.write("a.txt", "edited on my-feature\n");
    session.refresh();
    session.run_git_jobs();
    session.switch_branch("main");
    assert!(matches!(
        session.dialog,
        Some(Dialog::SwitchWithChanges { .. })
    ));
    session.switch_with("main", false);
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().branch(), Some("main"));
    assert_eq!(s.read("a.txt"), "a\n");
    session.switch_branch("my-feature");
    session.run_git_jobs();
    assert!(matches!(session.notice, Some(Notice::StashWaiting(_))));
    let name = session.branch_stash().unwrap().name.clone();
    session.view_stash(name.clone());
    session.run_git_jobs();
    assert_eq!(
        session
            .repo
            .as_ref()
            .unwrap()
            .stash
            .as_ref()
            .unwrap()
            .files
            .len(),
        1
    );
    session.restore_stash(name);
    session.run_git_jobs();
    assert_eq!(s.read("a.txt"), "edited on my-feature\n");
    assert!(session.branch_stash().is_none());
    session.rename_branch("my-feature".into(), "renamed");
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().branch(), Some("renamed"));
}

#[test]
fn a_merge_conflict_is_listed_resolved_and_continued() {
    let s = sandbox();
    s.commit_file("a.txt", "base\n", "base");
    s.git(&["switch", "-q", "-c", "topic"]);
    s.commit_file("a.txt", "topic\n", "topic");
    s.git(&["switch", "-q", "main"]);
    s.commit_file("a.txt", "main\n", "main");
    let mut session = opened(&s);
    session.pick_branch(BranchPurpose::Merge);
    session.branch_picked(BranchPurpose::Merge, "topic".into());
    session.run_git_jobs();
    let repo = session.repo.as_ref().unwrap();
    assert_eq!(repo.info.operation, Some(Operation::Merge));
    assert!(repo.in_conflict());
    assert!(!session.can_commit(), "the commit form waits for Continue");
    let rows = git_gui::conflicts::rows(&repo.status, &repo.info.conflict_markers);
    assert_eq!(rows[0].markers, Some(1));
    session.resolve_with(PathBuf::from("a.txt"), false);
    session.run_git_jobs();
    session.continue_operation();
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().info.operation, None);
    assert_eq!(s.read("a.txt"), "topic\n");
}

#[test]
fn push_refused_then_pull_then_push_and_fetch_counts() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let bare = s.with_origin();
    let other = s.other_clone(&bare);
    std::fs::write(other.join("t.txt"), "t\n").unwrap();
    git_in(&other, &["add", "t.txt"]);
    git_in(&other, &["commit", "-q", "-m", "theirs"]);
    git_in(&other, &["push", "-q", "origin", "main"]);
    s.commit_file("m.txt", "m\n", "mine");
    let mut session = opened(&s);
    session.push(false);
    session.run_git_jobs();
    let Some(Problem::Git {
        kind: OpKind::Push,
        error,
    }) = &session.problem
    else {
        panic!("{:?}", session.problem)
    };
    assert_eq!(
        git_core::classify_refusal(error.message().unwrap()),
        git_core::Refusal::NotFastForward
    );
    session.fetch(false, 1000);
    session.run_git_jobs();
    assert_eq!(
        session.repo.as_ref().unwrap().status.ahead_behind,
        Some((1, 1))
    );
    assert!(!session.fetch_due(1000 + 10), "fetched just now");
    assert!(session.fetch_due(1000 + 301), "five minutes later");
    session.pull();
    session.run_git_jobs();
    session.push(false);
    session.run_git_jobs();
    assert_eq!(
        session.repo.as_ref().unwrap().status.ahead_behind,
        Some((0, 0))
    );
}

#[test]
fn worktrees_remotes_clone_and_create() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let mut session = opened(&s);
    let wt = s.tmp.path().join("wt");
    session.add_worktree(wt.clone(), "side", true);
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().info.worktrees.len(), 2);
    session.add_remote("upstream", "https://example.invalid/x.git");
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().info.remotes.len(), 1);
    let bare = s.with_origin();
    let destination = s.tmp.path().join("clones/copy");
    session.clone_repository(bare.display().to_string(), destination.clone());
    session.run_git_jobs();
    assert_eq!(
        session.repo.as_ref().unwrap().root,
        destination,
        "a clone opens, trusted"
    );
    let created = s.tmp.path().join("made/new");
    session.create_repository(
        git_platform::NewRepository {
            path: created.clone(),
            default_branch: "main".into(),
            readme: Some(git_gui::publish::readme("new", "")),
            gitignore: Some(git_gui::publish::GITIGNORES[0].1.into()),
            license: None,
        },
        None,
        None,
    );
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().root, created);
    assert_eq!(git_in(&created, &["ls-files"]).lines().count(), 2);
}

#[test]
fn settings_git_config_and_repository_aliases_persist() {
    let s = sandbox();
    let config = s.home.join("gitconfig");
    // SAFETY: the test holds `SERIAL`.
    unsafe { std::env::set_var("GIT_CONFIG_GLOBAL", &config) };
    s.commit_file("a.txt", "a\n", "first");
    let mut session = opened(&s);
    session.open_settings();
    session.run_git_jobs();
    assert!(session.git_config.loaded);
    session.set_git_config(vec![("user.name".into(), "Ada".into())]);
    session.run_git_jobs();
    assert_eq!(session.git_config.get("user.name"), "Ada");
    session.settings.confirm_discard = false;
    session.save_settings();
    session.set_alias(&s.repo.clone(), "Work");
    let again = GitSession::new(Arc::new(SystemGit), files(&s));
    assert!(!again.settings.confirm_discard);
    assert_eq!(again.repositories.alias(&s.repo), Some("Work"));
    let mut session = again;
    session.ask_remove_repository(s.repo.clone());
    assert!(matches!(
        session.dialog,
        Some(Dialog::RemoveRepository { .. })
    ));
    session.remove_repository(&s.repo.clone());
    assert!(session.repositories.entries.is_empty());
    assert!(s.repo.join("a.txt").exists(), "the folder stays");
}

// --- independent review fixes -------------------------------------------

fn history_subjects(session: &GitSession) -> Vec<String> {
    session
        .repo
        .as_ref()
        .unwrap()
        .history
        .commits
        .iter()
        .map(|c| c.summary.clone())
        .collect()
}

#[test]
fn a_drag_on_a_stale_history_is_refused_and_refresh_reloads_it() {
    let s = sandbox();
    for name in ["a", "b", "c"] {
        s.commit_file(&format!("{name}.txt"), "x\n", name);
    }
    let mut session = opened(&s);
    session.set_tab(Tab::History);
    session.run_git_jobs();
    // A commit made elsewhere (a terminal, another window).
    s.commit_file("d.txt", "x\n", "d, made elsewhere");
    session.reorder(1, 0);
    session.run_git_jobs();
    assert!(
        matches!(
            &session.problem,
            Some(Problem::Git {
                kind: OpKind::Reorder,
                error: GitError::ChangedMeanwhile
            })
        ),
        "{:?}",
        session.problem
    );
    assert_eq!(s.subjects(), vec!["d, made elsewhere", "c", "b", "a"]);
    // Coming back to the window reads the history again.
    session.refresh();
    session.run_git_jobs();
    assert_eq!(
        history_subjects(&session),
        vec!["d, made elsewhere", "c", "b", "a"]
    );
}

#[test]
fn undo_puts_back_the_message_of_the_commit_git_has() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let mut session = opened(&s);
    session.set_tab(Tab::History);
    session.run_git_jobs();
    // The list says "first"; HEAD is now another commit.
    s.commit_file("b.txt", "b\n", "made elsewhere");
    session.undo_commit();
    session.run_git_jobs();
    assert_eq!(
        session.repo.as_ref().unwrap().form.summary,
        "made elsewhere"
    );
    assert_eq!(s.subjects(), vec!["first"]);
}

#[test]
fn a_merge_locks_the_choice_of_files_and_lines() {
    let s = sandbox();
    s.commit_file("a.txt", "base\n", "base");
    s.commit_file("f.txt", BASE, "f");
    s.git(&["switch", "-q", "-c", "topic"]);
    s.commit_file("a.txt", "topic\n", "topic a");
    s.commit_file(
        "f.txt",
        &BASE.replace("two", "TWO").replace("eleven", "ELEVEN"),
        "topic f",
    );
    s.git(&["switch", "-q", "main"]);
    s.commit_file("a.txt", "main\n", "main");
    let mut session = opened(&s);
    session.pick_branch(BranchPurpose::Merge);
    session.branch_picked(BranchPurpose::Merge, "topic".into());
    session.run_git_jobs();
    assert!(session.selection_locked());
    // f.txt came in from topic; leaving it or part of it out of the merge
    // commit would drop topic's change.
    session.toggle_file(Path::new("f.txt"));
    session.toggle_all();
    session.select_file(Path::new("f.txt"));
    session.run_git_jobs();
    assert!(!session.lines_selectable());
    session.toggle_hunk(0);
    let rows = session.repo.as_ref().unwrap().rows();
    let f = rows.iter().find(|r| r.path == Path::new("f.txt")).unwrap();
    assert_eq!(f.check, CheckState::Checked);
}

#[test]
fn conflicts_left_from_another_stash_never_drop_the_newest_stash() {
    let s = sandbox();
    s.commit_file("a.txt", "base\n", "base");
    s.write("a.txt", "stash one\n");
    s.git(&["stash", "push", "-q", "-m", "one"]);
    s.write("b.txt", "unrelated\n");
    s.git(&["stash", "push", "-q", "-u", "-m", "two"]);
    s.commit_file("a.txt", "committed\n", "moved on");
    // Applying the older stash stops on a conflict, outside Better Git.
    let applied = std::process::Command::new("git")
        .arg("-C")
        .arg(&s.repo)
        .args(["stash", "apply", "stash@{1}"])
        .output()
        .unwrap();
    assert!(!applied.status.success());
    let mut session = opened(&s);
    let repo = session.repo.as_ref().unwrap();
    assert!(repo.in_conflict());
    assert_eq!(repo.stash_conflict, None, "no stash is guessed");
    s.write("a.txt", "resolved\n");
    session.mark_resolved(PathBuf::from("a.txt"));
    session.run_git_jobs();
    session.continue_operation();
    session.run_git_jobs();
    assert_eq!(
        s.git(&["stash", "list"]).lines().count(),
        2,
        "both stashes kept"
    );
    assert_eq!(s.read("a.txt"), "resolved\n");
    assert!(!session.repo.as_ref().unwrap().in_conflict());
}

#[test]
fn a_squash_merge_conflict_reopened_continues_without_touching_stashes() {
    let s = sandbox();
    s.commit_file("a.txt", "base\n", "base");
    s.git(&["switch", "-q", "-c", "topic"]);
    s.commit_file("a.txt", "topic\n", "topic");
    s.git(&["switch", "-q", "main"]);
    s.commit_file("a.txt", "main\n", "main");
    s.write("keep.txt", "kept\n");
    s.git(&["stash", "push", "-q", "-u", "-m", "the person's"]);
    let mut session = opened(&s);
    session.pick_branch(BranchPurpose::SquashMerge);
    session.branch_picked(BranchPurpose::SquashMerge, "topic".into());
    session.run_git_jobs();
    drop(session);
    let mut session = GitSession::new(Arc::new(SystemGit), files(&s));
    session.open(s.repo.clone());
    session.run_git_jobs();
    assert_eq!(
        session.repo.as_ref().unwrap().info.operation,
        Some(Operation::SquashMerge)
    );
    session.resolve_with(PathBuf::from("a.txt"), false);
    session.run_git_jobs();
    session.continue_operation();
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().info.operation, None);
    assert!(s.subjects()[0].starts_with("Squashed commit"));
    assert_eq!(s.git(&["stash", "list"]).lines().count(), 1);
}

#[test]
fn push_and_delete_use_the_upstream_name_and_force_keeps_its_lease() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let bare = s.with_origin();
    s.git(&["switch", "-q", "-c", "local-name"]);
    s.git(&["push", "-q", "-u", "origin", "local-name:feature"]);
    s.commit_file("b.txt", "b\n", "more");
    let mut session = opened(&s);
    session.push(false);
    session.run_git_jobs();
    assert!(session.problem.is_none(), "{:?}", session.problem);
    assert_eq!(
        git_in(&bare, &["log", "-1", "--format=%s", "feature"]).trim(),
        "more"
    );
    assert!(!git_in(&bare, &["branch"]).contains("local-name"));

    // The question is asked; someone pushes; a background fetch moves
    // origin/feature before the person confirms.
    s.git(&["commit", "-q", "--amend", "-m", "more, rewritten"]);
    session.refresh();
    session.run_git_jobs();
    session.ask_force_push();
    let Some(Dialog::ForcePush { expected }) = session.dialog.clone() else {
        panic!("{:?}", session.dialog)
    };
    let other = s.other_clone(&bare);
    git_in(&other, &["switch", "-q", "feature"]);
    std::fs::write(other.join("t.txt"), "t\n").unwrap();
    git_in(&other, &["add", "t.txt"]);
    git_in(&other, &["commit", "-q", "-m", "theirs"]);
    git_in(&other, &["push", "-q", "origin", "feature"]);
    session.fetch(true, 5000);
    session.run_git_jobs();
    session.push_with(Some(git_platform::Lease { expected }));
    session.run_git_jobs();
    assert!(
        matches!(
            &session.problem,
            Some(Problem::Git {
                kind: OpKind::ForcePush,
                ..
            })
        ),
        "{:?}",
        session.problem
    );
    assert_eq!(
        git_in(&bare, &["log", "-1", "--format=%s", "feature"]).trim(),
        "theirs"
    );

    session.switch_branch("main");
    session.run_git_jobs();
    session.delete_branch("local-name".into(), true);
    session.run_git_jobs();
    assert!(!git_in(&bare, &["branch"]).contains("feature"));
}

#[test]
fn a_stuck_background_fetch_does_not_hold_up_a_commit_and_can_be_stopped() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.git(&["remote", "add", "origin", "ssh://example.invalid/x.git"]);
    // SAFETY: tests hold `SERIAL`.
    unsafe {
        std::env::set_var("GIT_SSH_COMMAND", "sleep 60 #");
    }
    let mut session = opened(&s);
    session.fetch(true, 1000);
    let background = session.next_background().expect("a background fetch");
    let git = session.git();
    let fetch = std::thread::spawn(move || background.run(git.as_ref()));
    s.write("b.txt", "b\n");
    session.refresh();
    session.run_git_jobs();
    session.repo.as_mut().unwrap().form.summary = "while fetching".into();
    session.commit();
    let started = std::time::Instant::now();
    session.run_git_jobs();
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(s.subjects()[0], "while fetching");
    assert!(session.is_background_fetching());
    session.cancel_network();
    let outcome = fetch.join().unwrap();
    session.apply_background(outcome);
    // SAFETY: tests hold `SERIAL`.
    unsafe {
        std::env::remove_var("GIT_SSH_COMMAND");
    }
    assert!(!session.is_background_fetching());
    assert!(
        session.problem.is_none(),
        "a background fetch fails quietly"
    );
}
