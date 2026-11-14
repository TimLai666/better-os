//! The Graph tab's session side against a real `git` in temporary
//! repositories, driven the way the window drives it. `HOME`, the XDG
//! directories, and the global git configuration point into the temporary
//! directory; nothing reaches the person's own files.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use git_core::RefKind;
use git_gui::dialogs::Dialog;
use git_gui::graph::{Direction, GRAPH_PAGE};
use git_gui::session::{DiffContent, Files, GitSession, Tab};
use git_platform::{GraphQuery, GraphScope, SystemGit};

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
            std::env::set_var("XDG_CONFIG_HOME", home.join("config"));
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

        pub fn commit(&self, message: &str) {
            fs::write(self.repo.join(format!("{message}.txt")), message).unwrap();
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", message]);
        }

        pub fn sha(&self, revision: &str) -> String {
            self.git(&["rev-parse", revision]).trim().to_string()
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

fn opened(s: &Sandbox) -> GitSession {
    let mut session = GitSession::new(Arc::new(SystemGit), files(s));
    session.open(s.repo.clone());
    session.trust_and_open(s.repo.clone());
    session.run_git_jobs();
    assert!(session.repo.is_some(), "{:?}", session.problem);
    session
}

/// main: a - b - M(merge of feature f) - c, a tag on b, `origin` with main.
fn branched() -> Sandbox {
    let s = sandbox();
    s.commit("a");
    s.git(&["switch", "-q", "-c", "feature"]);
    s.commit("f");
    s.git(&["switch", "-q", "main"]);
    s.commit("b");
    s.git(&["tag", "v1"]);
    s.git(&["merge", "-q", "--no-ff", "-m", "M", "feature"]);
    s.commit("c");
    let bare = s.tmp.path().join("origin.git");
    git_in(
        s.tmp.path(),
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    s.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    s.git(&["push", "-q", "origin", "main"]);
    s
}

fn summaries(session: &GitSession) -> Vec<String> {
    session
        .repo
        .as_ref()
        .unwrap()
        .graph
        .commits
        .iter()
        .map(|c| c.summary.clone())
        .collect()
}

fn graph(session: &GitSession) -> &git_gui::graph::GraphView {
    &session.repo.as_ref().unwrap().graph
}

#[test]
fn the_graph_tab_reads_every_branch_with_lanes_labels_and_history() {
    let s = branched();
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    let view = graph(&session);
    assert!(view.loaded && view.end, "{:?}", session.problem);
    assert_eq!(view.commits.len(), 5);
    assert_eq!(view.rows.len(), 5);
    assert_eq!(view.columns, 2, "main and the merged feature");
    // HEAD's commit is at the top with main, origin/main, both labelled.
    let top = view.labels(0);
    assert!(
        top.iter()
            .any(|l| l.kind == RefKind::Local && l.name == "main" && l.current)
    );
    assert!(
        top.iter()
            .any(|l| l.kind == RefKind::Remote && l.name == "origin/main")
    );
    let b = view.commits.iter().position(|c| c.summary == "b").unwrap();
    assert!(
        view.labels(b)
            .iter()
            .any(|l| l.kind == RefKind::Tag && l.name == "v1")
    );
    // History's first page came too, for dragging.
    assert!(session.repo.as_ref().unwrap().history.loaded);
}

#[test]
fn selecting_a_commit_shows_its_files_and_diff() {
    let s = branched();
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    let f = graph(&session)
        .commits
        .iter()
        .position(|c| c.summary == "f")
        .unwrap();
    session.select_graph_commit(f);
    session.run_git_jobs();
    let repo = session.repo.as_ref().unwrap();
    assert_eq!(
        repo.graph.selected.as_deref(),
        Some(s.sha("feature").as_str())
    );
    assert_eq!(repo.graph.files.len(), 1);
    assert_eq!(repo.graph.files[0].path, PathBuf::from("f.txt"));
    assert_eq!(repo.graph.file, Some(0));
    assert!(matches!(
        repo.diff.as_ref().map(|d| &d.content),
        Some(DiffContent::Text(_))
    ));
    // Back on the tab after History, the same commit is shown again.
    session.set_tab(Tab::History);
    session.run_git_jobs();
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    let repo = session.repo.as_ref().unwrap();
    assert_eq!(repo.graph.selected_index(), Some(f));
    assert!(repo.diff.is_some());
}

#[test]
fn a_change_elsewhere_reads_the_graph_again_and_keeps_the_selection() {
    let s = branched();
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    let f = graph(&session)
        .commits
        .iter()
        .position(|c| c.summary == "f")
        .unwrap();
    session.select_graph_commit(f);
    session.run_git_jobs();
    let replaced = graph(&session).replaced;
    // Nothing moved: a refresh leaves the graph alone.
    session.refresh();
    session.run_git_jobs();
    assert_eq!(graph(&session).replaced, replaced);
    // A commit made in a terminal: the graph is read again, with it.
    s.commit("d");
    session.refresh();
    session.run_git_jobs();
    let view = graph(&session);
    assert_eq!(view.replaced, replaced + 1);
    assert_eq!(view.commits[0].summary, "d");
    assert_eq!(
        view.selected_commit().map(|c| c.summary.as_str()),
        Some("f"),
        "the selection follows its commit to its new row"
    );
    // Away from the tab, a change only marks it stale; showing it reads it.
    session.set_tab(Tab::Changes);
    s.commit("e");
    session.refresh();
    session.run_git_jobs();
    assert!(graph(&session).stale);
    assert_eq!(graph(&session).commits[0].summary, "d");
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    assert_eq!(graph(&session).commits[0].summary, "e");
    assert!(!graph(&session).stale);
}

#[test]
fn the_filter_narrows_the_graph_and_is_remembered() {
    let s = branched();
    s.git(&["switch", "-q", "-c", "side", "v1"]);
    s.commit("s");
    s.git(&["switch", "-q", "main"]);
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    assert!(summaries(&session).contains(&"s".to_string()));
    session.set_graph_query(GraphQuery {
        scope: GraphScope::Current,
        remotes: false,
        tags: false,
    });
    session.run_git_jobs();
    assert!(!summaries(&session).contains(&"s".to_string()));
    assert!(
        graph(&session)
            .labels(0)
            .iter()
            .all(|l| l.kind == RefKind::Local)
    );
    assert_eq!(session.settings.graph_scope, "current");
    // A new window starts with the same filter.
    drop(session);
    let mut again = opened(&s);
    again.set_tab(Tab::Graph);
    again.run_git_jobs();
    assert_eq!(graph(&again).query.scope, GraphScope::Current);
    assert!(!graph(&again).query.remotes && !graph(&again).query.tags);
}

#[test]
fn find_selects_and_scrolls_to_a_match_by_subject_author_or_hash() {
    let s = branched();
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    session.open_graph_search();
    session.set_graph_search("f");
    session.graph_find(Direction::Next);
    session.run_git_jobs();
    let view = graph(&session);
    let at = view.commits.iter().position(|c| c.summary == "f").unwrap();
    assert_eq!(view.selected_index(), Some(at));
    assert_eq!(view.scroll_to, Some(at));
    let prefix = s.sha("v1")[..8].to_string();
    session.set_graph_search(&prefix);
    session.graph_find(Direction::Next);
    assert_eq!(
        graph(&session)
            .selected_commit()
            .map(|c| c.summary.as_str()),
        Some("b")
    );
    session.set_graph_search("Better Git Test");
    session.graph_find(Direction::Previous);
    assert!(graph(&session).selected_index().is_some());
    session.set_graph_search("no such commit");
    session.graph_find(Direction::Next);
    assert!(graph(&session).not_found);
    session.close_graph_search();
    assert!(!graph(&session).search_open && !graph(&session).not_found);
}

/// `n` commits on main, through fast-import, oldest named `Commit 0`.
fn long_history(s: &Sandbox, n: usize) {
    let mut stream = Vec::new();
    for i in 0..n {
        let message = format!("Commit {i}\n");
        stream.extend(
            format!(
                "commit refs/heads/main\nmark :{}\ncommitter Better Git Test <test@example.invalid> {} +0000\ndata {}\n{message}",
                i + 1,
                1_600_000_000 + i * 60,
                message.len()
            )
            .bytes(),
        );
        if i > 0 {
            stream.extend(format!("from :{i}\n").bytes());
        }
        stream.extend(format!("M 644 inline f.txt\ndata {}\n{message}\n", message.len()).bytes());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&s.repo)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&stream).unwrap();
    assert!(child.wait().unwrap().success());
    s.git(&["reset", "-q", "--hard", "main"]);
}

#[test]
fn a_long_history_loads_page_by_page_and_find_reads_on_to_a_match() {
    let s = sandbox();
    long_history(&s, GRAPH_PAGE * 2 + 50);
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    assert_eq!(graph(&session).commits.len(), GRAPH_PAGE, "one page first");
    assert!(!graph(&session).end);
    assert!(graph(&session).wants_more(GRAPH_PAGE - 10));
    session.load_more_graph();
    session.run_git_jobs();
    assert_eq!(graph(&session).commits.len(), GRAPH_PAGE * 2);
    // The oldest commit is not loaded yet; find reads on until it is.
    session.set_graph_search("Commit 0");
    session.select_graph_commit(0);
    session.graph_find(Direction::Next);
    session.run_git_jobs();
    let view = graph(&session);
    assert!(view.end);
    assert_eq!(view.commits.len(), GRAPH_PAGE * 2 + 50);
    assert_eq!(
        view.selected_commit().map(|c| c.summary.as_str()),
        Some("Commit 0")
    );
    assert!(view.search_pending.is_none());
    // One line the whole way down, across the pages.
    let rows = view.rows.rows(0..view.commits.len());
    assert!(rows.iter().all(|r| r.column == 0 && r.lane == rows[0].lane));
    // A refresh after a commit reads all of it again in one swap.
    s.git(&["commit", "-q", "--allow-empty", "-m", "newest"]);
    session.refresh();
    session.run_git_jobs();
    let view = graph(&session);
    assert_eq!(view.commits.len(), GRAPH_PAGE * 2 + 51);
    assert_eq!(view.commits[0].summary, "newest");
}

#[test]
fn graph_actions_cherry_pick_revert_and_rewrite_like_history() {
    let s = sandbox();
    s.commit("a");
    s.commit("b");
    s.commit("c");
    s.git(&["branch", "other", "HEAD~2"]);
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    let sha = |session: &GitSession, name: &str| {
        graph(session)
            .commits
            .iter()
            .find(|c| c.summary == name)
            .unwrap()
            .sha
            .clone()
    };
    // Nothing pushed and no merge: every commit may be rewritten.
    assert!(session.graph_can_rewrite(&sha(&session, "b")));
    // Dropping c on the top edge of b: c stays newest (nothing moves);
    // b on c's: b becomes newest.
    let (b, c) = (sha(&session, "b"), sha(&session, "c"));
    session.graph_reorder(&b, &c);
    session.run_git_jobs();
    assert_eq!(
        graph(&session).commits[0].summary,
        "b",
        "{:?}",
        session.problem
    );
    // Squash asks first, through History's dialog.
    let (b, c) = (sha(&session, "b"), sha(&session, "c"));
    session.graph_squash(&b, &c);
    assert!(matches!(session.dialog, Some(Dialog::Squash { .. })));
    session.cancel_dialog();
    // Cherry-pick onto another branch.
    let c = sha(&session, "c");
    session.cherry_pick_sha(c, Some("other".into()));
    session.run_git_jobs();
    assert_eq!(s.git(&["log", "-1", "--format=%s", "other"]).trim(), "c");
    // Revert the commit now at the top of other's line.
    s.git(&["switch", "-q", "main"]);
    session.refresh();
    session.run_git_jobs();
    let top = graph(&session).commits[0].clone();
    session.revert_commit(&top);
    session.run_git_jobs();
    assert!(
        s.git(&["log", "-1", "--format=%s"]).starts_with("Revert"),
        "{:?}",
        session.problem
    );
    assert!(graph(&session).commits[0].summary.starts_with("Revert"));
}

#[test]
fn hiding_the_tab_stops_git_and_showing_it_picks_up_where_it_stopped() {
    let s = sandbox();
    long_history(&s, GRAPH_PAGE * 2 + 20);
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    let feed = graph(&session).feed().unwrap().clone();
    assert!(feed.is_reading(), "git waits on the pipe for the next page");
    session.set_tab(Tab::History);
    assert!(
        !feed.is_reading(),
        "no git log runs while the tab is hidden"
    );
    session.run_git_jobs();
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    assert_eq!(
        graph(&session).commits.len(),
        GRAPH_PAGE,
        "nothing read again"
    );
    session.load_more_graph();
    session.run_git_jobs();
    session.load_more_graph();
    session.run_git_jobs();
    let view = graph(&session);
    assert!(view.end);
    assert_eq!(view.commits.len(), GRAPH_PAGE * 2 + 20);
    let subjects: Vec<String> = view.commits.iter().map(|c| c.summary.clone()).collect();
    let expected: Vec<String> = (0..GRAPH_PAGE * 2 + 20)
        .rev()
        .map(|i| format!("Commit {i}"))
        .collect();
    assert_eq!(subjects, expected, "picked up in order, nothing twice");
}

#[test]
fn a_commit_made_while_the_tab_was_hidden_is_neither_doubled_nor_lost() {
    let s = sandbox();
    long_history(&s, GRAPH_PAGE * 2 + 20);
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    assert_eq!(graph(&session).commits.len(), GRAPH_PAGE);
    session.set_tab(Tab::History);
    session.run_git_jobs();
    // A commit lands on main while the graph's walk is stopped, and the
    // window has not refreshed yet.
    s.git(&["commit", "-q", "--allow-empty", "-m", "made meanwhile"]);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    while !graph(&session).end {
        session.load_more_graph();
        session.run_git_jobs();
    }
    let view = graph(&session);
    let subjects: Vec<String> = view.commits.iter().map(|c| c.summary.clone()).collect();
    let mut expected = vec!["made meanwhile".to_string()];
    expected.extend(
        (0..GRAPH_PAGE * 2 + 20)
            .rev()
            .map(|i| format!("Commit {i}")),
    );
    assert_eq!(subjects, expected, "every commit once, in order");
}

#[test]
fn a_refused_reorder_or_squash_says_why() {
    let s = sandbox();
    s.commit("a");
    s.commit("b");
    let bare = s.tmp.path().join("origin.git");
    git_in(
        s.tmp.path(),
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    s.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    s.git(&["push", "-q", "-u", "origin", "main"]);
    s.commit("c");
    s.git(&["branch", "side", "HEAD~2"]);
    let mut session = opened(&s);
    session.set_tab(Tab::Graph);
    session.run_git_jobs();
    let sha = |session: &GitSession, name: &str| {
        graph(session)
            .commits
            .iter()
            .find(|c| c.summary == name)
            .unwrap()
            .sha
            .clone()
    };
    // `b` is pushed: squashing `c` into it is refused, and why is said.
    let (b, c) = (sha(&session, "b"), sha(&session, "c"));
    session.graph_squash(&c, &b);
    assert_eq!(
        graph(&session).refusal,
        Some(git_gui::graph::Refusal::Pushed)
    );
    assert!(session.dialog.is_none());
    session.dismiss_graph_refusal();
    assert!(graph(&session).refusal.is_none());
    // Dropping a commit on itself does nothing and says nothing.
    session.graph_reorder(&c, &c);
    assert!(graph(&session).refusal.is_none());
    // Not on the current branch's line.
    s.git(&["switch", "-q", "side"]);
    s.commit("s");
    s.git(&["switch", "-q", "main"]);
    session.refresh();
    session.run_git_jobs();
    let (side, c) = (sha(&session, "s"), sha(&session, "c"));
    session.graph_reorder(&side, &c);
    assert_eq!(
        graph(&session).refusal,
        Some(git_gui::graph::Refusal::NotOnBranch)
    );
}

/// The acceptance figure, through the session the window drives: a
/// generated 100,000-commit repository with a branch merged back every 50
/// commits. Run with `cargo test -p git-gui --test graph --release --
/// --ignored --nocapture`; it prints the numbers.
#[test]
#[ignore = "builds a 100,000-commit repository; run on purpose for the figures"]
fn a_hundred_thousand_commit_graph_shows_its_first_rows_in_under_a_second() {
    use std::time::{Duration, Instant};
    let s = sandbox();
    let mut stream = Vec::new();
    let (mut main, mut feature): (usize, Option<(usize, usize)>) = (0, None);
    for i in 0..100_000usize {
        let mark = i + 1;
        if i % 50 == 10 && feature.is_none() && main > 0 {
            feature = Some((main, 0));
        }
        let on_feature = matches!(feature, Some((_, own)) if own < 20) && i % 2 == 1;
        let (branch, parent) = match (on_feature, feature) {
            (true, Some((tip, _))) => ("refs/heads/feature", tip),
            _ => ("refs/heads/main", main),
        };
        let message = format!("Commit {i}\n");
        stream.extend(
            format!(
                "commit {branch}\nmark :{mark}\ncommitter Better Git Test <test@example.invalid> {} +0000\ndata {}\n{message}",
                1_600_000_000 + i * 60,
                message.len()
            )
            .bytes(),
        );
        if parent > 0 {
            stream.extend(format!("from :{parent}\n").bytes());
        }
        if !on_feature
            && let Some((tip, own)) = feature
            && own >= 20
        {
            stream.extend(format!("merge :{tip}\n").bytes());
            feature = None;
        }
        stream.extend(
            format!(
                "M 644 inline f{}.txt\ndata {}\n{message}\n",
                i % 100,
                message.len()
            )
            .bytes(),
        );
        match (on_feature, feature.as_mut()) {
            (true, Some((tip, own))) => {
                *tip = mark;
                *own += 1;
            }
            _ => main = mark,
        }
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&s.repo)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&stream).unwrap();
    assert!(child.wait().unwrap().success());
    s.git(&["reset", "-q", "--hard", "main"]);
    assert_eq!(s.git(&["rev-list", "--all", "--count"]).trim(), "100000");

    let mut session = opened(&s);
    let start = Instant::now();
    session.set_tab(Tab::Graph);
    // Exactly what the window's background thread does with the job.
    let job = session.next_graph().expect("the first page is asked for");
    let outcome = job.run(&SystemGit);
    let read = start.elapsed();
    session.apply_graph(outcome);
    let first = start.elapsed();
    let view = graph(&session);
    assert_eq!(view.commits.len(), GRAPH_PAGE);
    eprintln!(
        "first page of {GRAPH_PAGE}: read and laid out off the UI thread in {read:?}; applied on it by {first:?}"
    );

    let mut slowest_read = Duration::ZERO;
    let mut slowest_apply = Duration::ZERO;
    while !graph(&session).end {
        session.load_more_graph();
        let job = session.next_graph().expect("a next page");
        let t = Instant::now();
        let outcome = job.run(&SystemGit);
        slowest_read = slowest_read.max(t.elapsed());
        let t = Instant::now();
        session.apply_graph(outcome);
        slowest_apply = slowest_apply.max(t.elapsed());
    }
    eprintln!(
        "all {} commits after {:?}; slowest page read {slowest_read:?} (background), slowest page applied {slowest_apply:?} (UI thread)",
        graph(&session).commits.len(),
        start.elapsed()
    );
    // Finding a commit by subject across everything loaded, on the UI thread.
    session.set_graph_search("Commit 7");
    session.select_graph_commit(0);
    let t = Instant::now();
    session.graph_find(Direction::Next);
    eprintln!("find across 100,000 loaded commits: {:?}", t.elapsed());
    assert_eq!(graph(&session).commits.len(), 100_000);
    // The lanes of one screen of rows, rebuilt from the stored state as a
    // frame does, deep in the history.
    let t = Instant::now();
    let screen = graph(&session).rows.rows(77_777..77_807);
    let lanes_per_frame = t.elapsed();
    assert_eq!(screen.len(), 30);
    eprintln!("lanes for 30 rows at row 77,777: {lanes_per_frame:?}");
    // A commit after the whole history was shown: everything is read again
    // off the window's thread and swapped in on it.
    s.git(&["commit", "-q", "--allow-empty", "-m", "newest"]);
    session.refresh();
    while let Some(job) = session.next_job() {
        let outcome = job.run(&SystemGit);
        session.apply(outcome);
    }
    let job = session.next_graph().expect("the graph is read again");
    let t = Instant::now();
    let outcome = job.run(&SystemGit);
    let reread = t.elapsed();
    let t = Instant::now();
    session.apply_graph(outcome);
    let swap = t.elapsed();
    assert_eq!(graph(&session).commits.len(), 100_001);
    eprintln!(
        "after a commit: 100,001 commits read again in {reread:?} (background), swapped in {swap:?} (UI thread)"
    );
    assert!(swap < Duration::from_millis(16), "the swap took {swap:?}");
    assert!(first < Duration::from_secs(1), "first rows after {first:?}");
    assert!(
        slowest_apply < Duration::from_millis(50),
        "a page took {slowest_apply:?} to apply"
    );
}
