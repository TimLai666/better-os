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
    // No "done" bar: the emptied form and the list are the confirmation.
    assert_eq!(session.notice, None);
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

// --- ticket 103 -----------------------------------------------------------

/// "Amend last commit" right after opening, before the history was ever
/// read: the form says it is reading, amend stays on, and the message and
/// co-authors arrive from `git log -1`.
#[test]
fn amend_before_the_history_is_read_fills_the_form_from_the_last_commit() {
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
    assert!(!session.repo.as_ref().unwrap().history.loaded);
    let before = session.repo.as_ref().unwrap().form.generation;
    session.amend_last();
    let form = &session.repo.as_ref().unwrap().form;
    assert!(form.amend && form.reading_last, "ticked, and reading");
    assert_eq!(form.summary, "");
    session.run_git_jobs();
    let form = &session.repo.as_ref().unwrap().form;
    assert!(form.amend && !form.reading_last);
    assert_eq!(
        (form.summary.as_str(), form.description.as_str()),
        ("second", "Body")
    );
    assert_eq!(form.co_authors, "Ada <ada@example.invalid>");
    assert!(
        form.generation > before,
        "the window copies it into the fields"
    );
    // Unticked while reading: nothing is filled in afterwards.
    session.stop_amending();
    session.repo.as_mut().unwrap().form.summary.clear();
    session.amend_last();
    session.stop_amending();
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().form.summary, "");
}

/// The same from the History tab's menu and the Repository menu, once the
/// history has been read: the form fills in the same call.
#[test]
fn amend_after_the_history_is_read_fills_the_form_at_once_and_shows_changes() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let mut session = opened(&s);
    session.set_tab(Tab::History);
    session.run_git_jobs();
    session.amend_last();
    let repo = session.repo.as_ref().unwrap();
    assert_eq!(repo.tab, Tab::Changes);
    assert!(repo.form.amend && !repo.form.reading_last);
    assert_eq!(repo.form.summary, "first");
}

fn without_identity(s: &Sandbox) -> PathBuf {
    let config = s.home.join("gitconfig");
    std::fs::write(&config, "[user]\n\tuseConfigOnly = true\n").unwrap();
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
        std::env::set_var("GIT_CONFIG_GLOBAL", &config);
    }
    config
}

/// With no name or email, committing asks for them instead of showing
/// Git's refusal; saving writes them to the global settings (here a file in
/// the temporary home) and the commit goes ahead.
#[test]
fn a_commit_without_an_identity_asks_for_one_then_commits() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let config = without_identity(&s);
    let mut session = opened(&s);
    s.write("a.txt", "changed\n");
    session.refresh();
    session.run_git_jobs();
    session.repo.as_mut().unwrap().form.summary = "Second".into();
    session.commit();
    session.run_git_jobs();
    assert_eq!(session.dialog, Some(Dialog::Identity));
    assert!(session.problem.is_none(), "{:?}", session.problem);
    assert_eq!(s.subjects(), vec!["first"]);
    // A bad email is refused, and nothing is written.
    session.save_identity("Ada Lovelace", "not an address");
    assert_eq!(session.dialog, Some(Dialog::Identity));
    session.save_identity("Ada Lovelace", "ada@example.com");
    session.run_git_jobs();
    assert_eq!(session.dialog, None);
    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.contains("name = Ada Lovelace"), "{written}");
    assert!(written.contains("email = ada@example.com"), "{written}");
    assert_eq!(s.subjects(), vec!["Second", "first"]);
    assert_eq!(
        s.git(&["log", "-1", "--format=%an <%ae>"]).trim(),
        "Ada Lovelace <ada@example.com>"
    );
}

/// Creating a repository with no identity asks first and creates it after;
/// cancelling creates nothing.
#[test]
fn a_new_repository_without_an_identity_asks_then_is_created() {
    let s = sandbox();
    without_identity(&s);
    let mut session = GitSession::new(Arc::new(SystemGit), files(&s));
    let target = s.tmp.path().join("made");
    let new = || git_platform::NewRepository {
        path: target.clone(),
        default_branch: "main".into(),
        readme: Some("# made\n".into()),
        gitignore: None,
        license: None,
    };
    session.create_repository(new(), None, None);
    session.run_git_jobs();
    assert_eq!(session.dialog, Some(Dialog::Identity));
    assert!(!target.exists(), "nothing is made before the identity");
    session.cancel_dialog();
    assert_eq!(session.identity_pending, None);

    session.create_repository(new(), None, None);
    session.run_git_jobs();
    assert_eq!(session.dialog, Some(Dialog::Identity));
    session.save_identity("Ada", "ada@example.com");
    session.run_git_jobs();
    assert!(target.join(".git").is_dir());
    assert_eq!(
        git_in(&target, &["log", "-1", "--format=%s %ae"]).trim(),
        "Initial commit ada@example.com"
    );
    assert!(session.repo.is_some(), "it opens: {:?}", session.problem);
    assert_eq!(session.notice, None, "no done bar");
}

/// A branch checked out in another worktree opens that worktree instead of
/// failing in Git, and the repository list learns every repository's
/// linked worktrees.
#[test]
fn worktrees_show_in_the_lists_and_their_branches_open_them() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let one = s.tmp.path().join("wt-one");
    let two = s.tmp.path().join("wt-two");
    s.git(&["worktree", "add", "-q", "-b", "one", one.to_str().unwrap()]);
    s.git(&["worktree", "add", "-q", "-b", "two", two.to_str().unwrap()]);
    let mut session = opened(&s);
    let repo = session.repo.as_ref().unwrap();
    let linked = git_gui::worktree_links::linked_rows(&repo.info.worktrees, Some(&repo.root));
    assert_eq!(linked.len(), 2);
    assert_eq!(linked[0].branch.as_deref(), Some("one"));
    assert_eq!(
        git_gui::worktree_links::checked_out_elsewhere("two", &repo.info.worktrees, &repo.root),
        Some(two.canonicalize().unwrap())
    );
    assert_eq!(
        git_gui::worktree_links::checked_out_elsewhere("main", &repo.info.worktrees, &repo.root),
        None,
        "the branch checked out here"
    );

    // The list's other repositories are read in the background.
    session.read_worktrees();
    session.run_git_jobs();
    assert_eq!(session.worktrees_of.len(), 1);
    assert_eq!(session.worktrees_for(&s.repo).len(), 3);

    session.switch_branch("two");
    assert!(
        matches!(&session.dialog, Some(Dialog::Trust { path, .. }) if path.canonicalize().unwrap() == two.canonicalize().unwrap()),
        "{:?}",
        session.dialog
    );
    session.trust_and_open(two.clone());
    session.run_git_jobs();
    let repo = session.repo.as_ref().unwrap();
    assert_eq!(repo.branch(), Some("two"));
    assert!(session.problem.is_none(), "{:?}", session.problem);
}

/// Changes made from outside, with the window untouched: the watch sees a
/// file created, edited, staged, and deleted, and the refresh it asks for
/// lists them. Churn in an ignored folder (one that was there when the watch
/// began, and one made afterwards) and in an ignored file asks for nothing.
#[test]
fn changes_made_outside_are_seen_without_a_click_and_ignored_ones_are_not() {
    use git_gui::live;
    use std::time::{Duration, Instant};

    let s = sandbox();
    s.write(".gitignore", "target/\nbuild/\n*.log\n");
    s.write("target/debug/.keep", "");
    s.commit_file("a.txt", "a\n", "first");
    let mut session = opened(&s);
    let watching =
        live::start(Arc::new(SystemGit), &session.repo.as_ref().unwrap().root).expect("watch");
    assert!(
        watching
            .plan
            .folders()
            .iter()
            .all(|(folder, _)| !folder.starts_with(s.repo.join("target"))),
        "an ignored folder gets no watch"
    );

    // Waits up to `limit` for the watch to report a change and its quiet
    // time to pass.
    let wait = |watching: &live::Watching, limit: Duration| {
        let start = Instant::now();
        let mut debounce = live::Debounce::default();
        while start.elapsed() < limit {
            let now = start.elapsed().as_millis() as u64;
            if watching.take().changed {
                debounce.changed(now);
            }
            if debounce.due(now) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    };
    let seen = |watching: &live::Watching| wait(watching, Duration::from_secs(10));
    let quiet = |watching: &live::Watching| !wait(watching, Duration::from_millis(1500));
    let listed = |session: &mut GitSession, path: &str| {
        session.refresh();
        session.run_git_jobs();
        session
            .repo
            .as_ref()
            .unwrap()
            .status
            .changes
            .iter()
            .any(|c| c.path == Path::new(path))
    };

    std::fs::write(s.repo.join("target/debug/out.o"), "build output").unwrap();
    std::fs::write(s.repo.join("debug.log"), "a log").unwrap();
    assert!(
        quiet(&watching),
        "churn in target/ and in a log asks for nothing"
    );
    std::fs::create_dir_all(s.repo.join("build/obj")).unwrap();
    std::fs::write(s.repo.join("build/obj/x.o"), "x").unwrap();
    assert!(quiet(&watching), "a new ignored folder asks for nothing");

    s.write("new.txt", "new\n");
    assert!(seen(&watching), "a created file is seen");
    assert!(listed(&mut session, "new.txt"));

    s.write("a.txt", "edited\n");
    assert!(seen(&watching), "an edit is seen");
    assert!(listed(&mut session, "a.txt"));

    s.git(&["add", "new.txt"]);
    assert!(seen(&watching), "staging from a terminal is seen");

    std::fs::remove_file(s.repo.join("new.txt")).unwrap();
    assert!(seen(&watching), "a delete is seen");
    s.git(&["reset", "-q", "--", "new.txt"]);
    assert!(seen(&watching));
    assert!(!listed(&mut session, "new.txt"));
}

/// A tracked file whose time is in the future makes `git status` want to
/// rewrite the index on every run. Better Git watches the index, so a status
/// that wrote it would start the next refresh, for ever. Its reads take no
/// optional lock, so the refresh it asks for leaves the index alone.
#[test]
fn a_refresh_never_writes_the_index_it_watches() {
    use git_gui::live;
    use std::time::{Duration, Instant, SystemTime};

    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    s.write("a.txt", "b\n");
    std::fs::File::options()
        .write(true)
        .open(s.repo.join("a.txt"))
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
    let mut session = opened(&s);
    let watching =
        live::start(Arc::new(SystemGit), &session.repo.as_ref().unwrap().root).expect("watch");
    let index = s.repo.join(".git/index");
    let before = std::fs::metadata(&index).unwrap().modified().unwrap();
    for _ in 0..3 {
        session.refresh();
        session.run_git_jobs();
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(500) {
            assert!(
                !watching.take().changed,
                "the refresh changed what it watches"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    assert_eq!(
        std::fs::metadata(&index).unwrap().modified().unwrap(),
        before
    );
}

/// A name Git already has is never replaced: the step writes only the
/// missing email, whatever the name field held.
#[test]
fn the_identity_step_keeps_a_name_git_already_has() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let config = without_identity(&s);
    std::fs::write(
        &config,
        "[user]\n\tuseConfigOnly = true\n\tname = Ada Existing\n",
    )
    .unwrap();
    let mut session = opened(&s);
    s.write("a.txt", "changed\n");
    session.refresh();
    session.run_git_jobs();
    session.repo.as_mut().unwrap().form.summary = "Second".into();
    session.commit();
    session.run_git_jobs();
    assert_eq!(session.dialog, Some(Dialog::Identity));
    assert_eq!(
        session.git_config.get("user.name"),
        "Ada Existing",
        "read before it opened"
    );
    session.save_identity("Someone Else", "ada@example.com");
    session.run_git_jobs();
    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.contains("name = Ada Existing"), "{written}");
    assert!(!written.contains("Someone Else"), "{written}");
    assert_eq!(
        s.git(&["log", "-1", "--format=%an <%ae>"]).trim(),
        "Ada Existing <ada@example.com>"
    );
}

/// A new repository with nothing to commit needs no identity; one asked
/// for while another repository is opened is not asked about afterwards.
#[test]
fn an_empty_new_repository_needs_no_identity() {
    let s = sandbox();
    without_identity(&s);
    let mut session = GitSession::new(Arc::new(SystemGit), files(&s));
    let target = s.tmp.path().join("bare-start");
    session.create_repository(
        git_platform::NewRepository {
            path: target.clone(),
            default_branch: "main".into(),
            readme: None,
            gitignore: None,
            license: None,
        },
        None,
        None,
    );
    session.run_git_jobs();
    assert_eq!(session.dialog, None);
    assert!(target.join(".git").is_dir());
    assert!(session.repo.is_some(), "{:?}", session.problem);
}

/// A commit stopped for want of an identity while the person moved on to
/// another repository asks nothing: the question would be about a commit
/// that can no longer be made there.
#[test]
fn a_stale_identity_question_is_dropped() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    without_identity(&s);
    let mut session = opened(&s);
    s.write("a.txt", "changed\n");
    session.refresh();
    session.run_git_jobs();
    session.repo.as_mut().unwrap().form.summary = "Second".into();
    session.commit();
    let job = session.next_job().expect("the commit");
    let outcome = job.run(&SystemGit);
    session.close_repository();
    session.apply(outcome);
    session.run_git_jobs();
    assert_eq!(session.dialog, None);
    assert_eq!(session.identity_pending, None);
}

/// When Git's settings already have a name and an email and Git still
/// refuses (here the repository's own settings blank the name), the step
/// would have nothing to write and could only ask again: Git's own words
/// are shown instead.
#[test]
fn a_refusal_the_step_cannot_fix_shows_gits_own_words() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let config = without_identity(&s);
    std::fs::write(
        &config,
        "[user]\n\tuseConfigOnly = true\n\tname = Ada\n\temail = ada@example.com\n",
    )
    .unwrap();
    s.git(&["config", "user.name", ""]);
    let mut session = opened(&s);
    s.write("a.txt", "changed\n");
    session.refresh();
    session.run_git_jobs();
    session.repo.as_mut().unwrap().form.summary = "Second".into();
    session.commit();
    session.run_git_jobs();
    assert_eq!(session.dialog, None, "no step that could only ask again");
    assert_eq!(session.identity_pending, None);
    assert!(
        matches!(&session.problem, Some(Problem::Git { error: GitError::Failed { message, .. }, .. }) if message.contains("ident")),
        "{:?}",
        session.problem
    );
}

/// Ticket 113: a linked worktree saved by an older Better Git as a
/// repository of its own is folded into its repository once the list is
/// read, the worktree that was open stays open and marked, opening another
/// linked worktree adds no second repository, and removing the repository
/// takes every worktree with it from the list and none from disk.
#[test]
fn linked_worktrees_saved_as_repositories_merge_into_their_repository() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let root = s.tmp.path().canonicalize().unwrap();
    let main = s.repo.canonicalize().unwrap();
    let one = root.join("wt-one");
    let two = root.join("wt-two");
    s.git(&["worktree", "add", "-q", "-b", "one", one.to_str().unwrap()]);
    s.git(&["worktree", "add", "-q", "--detach", two.to_str().unwrap()]);
    let state = s.home.join("state/better-os");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("git-repositories.json"),
        serde_json::json!({
            "repositories": [
                { "path": one, "alias": "Ticket" },
                { "path": main },
            ]
        })
        .to_string(),
    )
    .unwrap();

    let paths = |session: &GitSession| {
        session
            .repositories
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect::<Vec<_>>()
    };
    // The window reads the list's worktrees when it starts.
    let mut session = GitSession::new(Arc::new(SystemGit), files(&s));
    assert_eq!(session.repository_groups("").len(), 2, "nothing known yet");
    session.read_worktrees();
    session.run_git_jobs();
    assert_eq!(
        paths(&session),
        std::slice::from_ref(&main),
        "merged on load"
    );
    assert_eq!(session.repositories.alias(&main), Some("Ticket"));
    let reread = GitSession::new(Arc::new(SystemGit), files(&s));
    assert_eq!(paths(&reread), std::slice::from_ref(&main), "and written");

    session.open(one.clone());
    session.trust_and_open(one.clone());
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().root, one);
    assert_eq!(paths(&session), std::slice::from_ref(&main));
    let groups = session.repository_groups("");
    assert_eq!(groups.len(), 1, "{groups:#?}");
    // The worktrees are the toolbar's worktree dropdown's (ticket 116).
    let rows = session.worktree_rows("");
    let names: Vec<&str> = rows.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["repo", "wt-one", "wt-two"]);
    assert!(rows[0].main);
    assert!(rows[1].current, "the open worktree stays marked");
    assert_eq!(rows[2].branch, None);
    assert!(rows[2].short_sha.is_some());

    // Opening another worktree selects it under the same repository.
    session.open(two.clone());
    session.trust_and_open(two.clone());
    session.run_git_jobs();
    assert_eq!(paths(&session), std::slice::from_ref(&main));
    assert_eq!(session.recent.first(), Some(&two));
    let groups = session.repository_groups("");
    assert_eq!(groups.len(), 1);
    assert!(session.worktree_rows("")[2].current);

    session.remove_repository(&main);
    assert!(session.repositories.entries.is_empty());
    assert!(session.recent.is_empty(), "{:?}", session.recent);
    assert!(session.repo.is_none(), "its worktree was open");
    let reread = GitSession::new(Arc::new(SystemGit), files(&s));
    assert!(reread.repositories.entries.is_empty());
    assert!(main.exists() && one.exists() && two.exists());
}

/// Ticket 113 review: a folder Git cannot read as a repository of its own
/// (gone, or a plain folder inside another repository, which Git would
/// answer for) is left in the list exactly as it was, and drawn as missing.
#[test]
fn a_failed_read_leaves_the_saved_entry_exactly_as_it_was() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let inner = s.repo.canonicalize().unwrap().join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    let gone = s.tmp.path().join("gone");
    let state = s.home.join("state/better-os");
    std::fs::create_dir_all(&state).unwrap();
    let file = state.join("git-repositories.json");
    let saved = serde_json::json!({
        "repositories": [
            { "path": inner, "alias": "Dotfiles" },
            { "path": gone },
        ]
    })
    .to_string();
    std::fs::write(&file, &saved).unwrap();

    let mut session = GitSession::new(Arc::new(SystemGit), files(&s));
    session.read_worktrees();
    session.run_git_jobs();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        saved,
        "not written"
    );
    assert_eq!(session.repositories.writes, 0);
    let groups = session.repository_groups("");
    assert_eq!(groups.len(), 2, "{groups:#?}");
    assert!(groups.iter().all(|g| g.missing && g.opens.is_none()));
    assert!(groups.iter().any(|g| g.name == "Dotfiles"));
}

/// Ticket 113 review: a read that answers after its repository was removed
/// puts nothing back.
#[test]
fn a_late_read_for_a_removed_repository_changes_nothing() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let one = s.tmp.path().canonicalize().unwrap().join("wt-one");
    s.git(&["worktree", "add", "-q", "-b", "one", one.to_str().unwrap()]);
    let mut session = opened(&s);
    let main = session.repo.as_ref().unwrap().root.clone();
    // Saved by an older version: the linked worktree as a repository.
    session.repositories.add(&one).unwrap();
    session.read_worktrees();
    let job = session
        .next_list()
        .expect("the read waits beside the queue");
    session.remove_repository(&main);
    session.remove_repository(&one);
    assert!(session.repositories.entries.is_empty());
    let outcome = job.run(&SystemGit);
    session.apply_list(outcome);
    assert!(session.repositories.entries.is_empty(), "nothing came back");
    let reread = GitSession::new(Arc::new(SystemGit), files(&s));
    assert!(reread.repositories.entries.is_empty());
    assert!(session.repository_groups("").is_empty());
}

/// Ticket 116: the toolbar names the repository, as its row in the
/// repository list does, and the open worktree by its folder; the window
/// title adds the worktree only when it is not the main one; choosing the
/// repository again opens the worktree that was open last.
#[test]
fn the_toolbar_names_the_repository_and_the_worktree_apart() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let root = s.tmp.path().canonicalize().unwrap();
    let main = s.repo.canonicalize().unwrap();
    let one = root.join("wt-one");

    // Only the main worktree.
    let mut session = opened(&s);
    let names = session.open_names().unwrap();
    assert_eq!(names.repository, "repo");
    assert_eq!(names.worktree, "repo");
    assert!(!names.linked && names.only_main);
    assert_eq!(session.window_title("Better Git"), "repo — Better Git");
    assert_eq!(session.worktree_rows("").len(), 1);
    assert!(session.worktree_rows("")[0].main && session.worktree_rows("")[0].current);

    // A linked worktree, opened.
    s.git(&["worktree", "add", "-q", "-b", "one", one.to_str().unwrap()]);
    session.open(one.clone());
    session.trust_and_open(one.clone());
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().root, one);
    let names = session.open_names().unwrap();
    assert_eq!(names.repository, "repo", "not the worktree's folder");
    assert_eq!(names.worktree, "wt-one");
    assert!(names.linked && !names.only_main);
    assert_eq!(names.repository_folder, "repo");
    assert_eq!(
        session.window_title("Better Git"),
        "repo · wt-one — Better Git"
    );
    let groups = session.repository_groups("");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, names.repository, "the list's own name");

    // The alias names it everywhere; folders keep the folder's name.
    session.set_alias(&main, "Site");
    let names = session.open_names().unwrap();
    assert_eq!(names.repository, "Site");
    assert_eq!(names.repository_folder, "repo");
    assert_eq!(
        session.window_title("Better Git"),
        "Site · wt-one — Better Git"
    );
    assert_eq!(session.repository_name(&main), "Site");

    // Choosing the repository opens the worktree that was open last.
    session.open(main.clone());
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().root, main);
    assert_eq!(session.window_title("Better Git"), "Site — Better Git");
    assert_eq!(session.repository_groups("")[0].opens.as_ref(), Some(&main));
    session.open(one.clone());
    session.run_git_jobs();
    session.close_repository();
    assert_eq!(session.window_title("Better Git"), "Better Git");
    // As the dropdown does when it opens.
    session.read_worktrees();
    session.run_git_jobs();
    let opens = session.repository_groups("")[0].opens.clone().unwrap();
    assert_eq!(opens, one, "the worktree open last");
    session.open(opens);
    session.run_git_jobs();
    assert_eq!(session.repo.as_ref().unwrap().root, one);
}

/// Ticket 116: a bare repository's worktree is named after the bare
/// repository, and it is never the main worktree.
#[test]
fn a_bare_repository_names_its_worktree_after_itself() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let root = s.tmp.path().canonicalize().unwrap();
    let bare = root.join("site.git");
    let work = root.join("site-work");
    harness::git_in(
        &root,
        &[
            "clone",
            "-q",
            "--bare",
            s.repo.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    harness::git_in(
        &bare,
        &["worktree", "add", "-q", work.to_str().unwrap(), "main"],
    );
    let mut session = GitSession::new(Arc::new(SystemGit), files(&s));
    session.open(work.clone());
    session.trust_and_open(work.clone());
    session.run_git_jobs();
    assert!(session.repo.is_some(), "{:?}", session.problem);
    let names = session.open_names().unwrap();
    assert_eq!(names.repository, "site.git");
    assert_eq!(names.worktree, "site-work");
    assert!(names.linked);
    assert!(!names.only_main, "a bare repository has no main worktree");
    assert_eq!(
        session.window_title("Better Git"),
        "site.git · site-work — Better Git"
    );
    let rows = session.worktree_rows("");
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert!(rows[0].current && !rows[0].main);
}

/// Ticket 116 review: the open worktree cannot be removed from its own
/// window, and the confirmation for another one says what it holds, read
/// in the background.
#[test]
fn removing_a_worktree_says_what_it_holds_and_never_the_open_one() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let root = s.tmp.path().canonicalize().unwrap();
    let one = root.join("wt-one");
    s.git(&["worktree", "add", "-q", "-b", "one", one.to_str().unwrap()]);
    std::fs::write(one.join("a.txt"), "changed\n").unwrap();
    std::fs::write(one.join("new.txt"), "new\n").unwrap();
    let mut session = opened(&s);
    let main = session.repo.as_ref().unwrap().root.clone();

    session.ask_remove_worktree(main.clone());
    assert!(session.dialog.is_none(), "the main and open one: no");
    session.ask_remove_worktree(one.clone());
    assert!(matches!(
        &session.dialog,
        Some(Dialog::RemoveWorktree { path, force: false }) if *path == one
    ));
    assert_eq!(session.worktree_changes_for(&one), None, "being read");
    session.run_git_jobs();
    let found = session.worktree_changes_for(&one).unwrap().unwrap();
    assert_eq!((found.changed, found.untracked), (1, 1));

    // From the linked worktree itself, it cannot be removed.
    session.dialog = None;
    session.open(one.clone());
    session.trust_and_open(one.clone());
    session.run_git_jobs();
    session.ask_remove_worktree(one.clone());
    assert!(session.dialog.is_none());
    assert!(one.exists());
}

/// Ticket 116 review: every window writes the recently opened list; each
/// starts from the file, so two windows keep what both opened.
#[test]
fn two_windows_share_the_recent_list() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let root = s.tmp.path().canonicalize().unwrap();
    let one = root.join("wt-one");
    s.git(&["worktree", "add", "-q", "-b", "one", one.to_str().unwrap()]);
    let main = s.repo.canonicalize().unwrap();
    let mut first = opened(&s);
    let mut second = GitSession::new(Arc::new(SystemGit), files(&s));
    assert_eq!(second.recent, std::slice::from_ref(&main), "read at start");
    let file = s.home.join("state/better-os/git-recent.json");
    assert!(file.exists(), "opening wrote it");

    // The second window opens the linked worktree; the first one then
    // opens the main one again, and keeps the second window's too.
    second.open(one.clone());
    second.trust_and_open(one.clone());
    second.run_git_jobs();
    first.open(main.clone());
    first.run_git_jobs();
    assert_eq!(first.recent, [main.clone(), one.clone()]);
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        saved["repositories"],
        serde_json::json!([main.to_str().unwrap(), one.to_str().unwrap()])
    );
    // And the second one sees it when its dropdown opens.
    second.read_worktrees();
    assert_eq!(second.recent, [main, one]);
}

/// Ticket 116: lock with a reason and unlock from the worktree dropdown;
/// Remove is not offered while it is locked; a worktree whose folder is gone
/// is cleared from the list after a confirmation that names every one Git
/// would clear, and a locked one stays.
#[test]
fn worktrees_are_locked_unlocked_and_cleared_from_the_list() {
    let s = sandbox();
    s.commit_file("a.txt", "a\n", "first");
    let root = s.tmp.path().canonicalize().unwrap();
    let [one, gone, also_gone, away] =
        ["wt-one", "wt-gone", "wt-also-gone", "wt-away"].map(|n| root.join(n));
    for (path, branch) in [(&one, "one"), (&gone, "g"), (&also_gone, "a"), (&away, "w")] {
        s.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            path.to_str().unwrap(),
        ]);
    }
    let mut session = opened(&s);
    let row = |session: &GitSession, path: &Path| {
        session
            .worktree_rows("")
            .into_iter()
            .find(|w| w.path == path)
            .unwrap()
    };

    session.lock_worktree(one.clone(), "  on a USB stick ");
    session.run_git_jobs();
    let locked = row(&session, &one);
    assert!(locked.locked);
    assert_eq!(locked.lock_reason.as_deref(), Some("on a USB stick"));
    session.ask_remove_worktree(one.clone());
    assert!(session.dialog.is_none(), "a locked worktree is not removed");
    session.unlock_worktree(one.clone());
    session.run_git_jobs();
    assert!(!row(&session, &one).locked);
    session.ask_remove_worktree(one.clone());
    assert!(matches!(
        session.dialog,
        Some(Dialog::RemoveWorktree { .. })
    ));
    session.dialog = None;

    session.lock_worktree(away.clone(), "");
    session.run_git_jobs();
    assert_eq!(row(&session, &away).lock_reason, None);
    for path in [&gone, &also_gone, &away] {
        std::fs::remove_dir_all(path).unwrap();
    }
    session.refresh();
    session.run_git_jobs();
    session.ask_clear_worktrees();
    let Some(Dialog::ClearWorktrees { paths }) = session.dialog.clone() else {
        panic!("{:?}", session.dialog);
    };
    let mut named = paths.clone();
    named.sort();
    let mut expected = vec![gone.clone(), also_gone.clone()];
    expected.sort();
    assert_eq!(named, expected, "both, and not the locked one");
    session.clear_worktrees();
    session.run_git_jobs();
    let left: Vec<PathBuf> = session
        .worktree_rows("")
        .into_iter()
        .map(|w| w.path)
        .collect();
    assert!(left.contains(&away) && left.contains(&one));
    assert!(!left.contains(&gone) && !left.contains(&also_gone));
}
