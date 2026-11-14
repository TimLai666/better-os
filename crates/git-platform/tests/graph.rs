//! The branch graph's reader against a real `git` in temporary
//! repositories: which revisions each filter walks, the labels git prints,
//! reading a page at a time, and a 100,000-commit history.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use git_core::{LaneLayout, RefKind, ref_labels};
use git_platform::{Git, GraphQuery, GraphScope, SystemGit};

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
        pub repo: PathBuf,
    }

    /// A temporary HOME, no global or system git configuration, a fixed
    /// identity and clock, and a repository on `main`.
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

        /// A commit with a fixed date, `seconds` after a fixed epoch, so
        /// the order of commits made in one second is still defined.
        pub fn commit(&self, message: &str, seconds: u32) {
            let date = format!("{} +0000", 1_700_000_000 + seconds);
            let path = self.repo.join(format!("{message}.txt"));
            fs::write(&path, message).unwrap();
            self.git(&["add", "-A"]);
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["commit", "-q", "-m", message])
                .env("GIT_AUTHOR_DATE", &date)
                .env("GIT_COMMITTER_DATE", &date)
                .output()
                .unwrap();
            assert!(output.status.success());
        }

        pub fn subjects(&self, query: &git_platform::GraphQuery) -> Vec<String> {
            use git_platform::Git;
            let mut source = git_platform::SystemGit
                .graph(&self.repo, query, &[], 0)
                .unwrap();
            let commits = source.next_page(10_000).unwrap();
            assert!(source.is_done());
            commits.into_iter().map(|c| c.summary).collect()
        }
    }
}

use harness::{git_in, sandbox};

/// main: m1 - m2 - M(merge f1) - m3, feature: f1 off m1, tag v1 on m2, a
/// remote with a branch `origin/remote-only` holding one commit nobody
/// has locally, and a stash.
fn repository() -> harness::Sandbox {
    let s = sandbox();
    s.commit("m1", 1);
    s.git(&["switch", "-q", "-c", "feature"]);
    s.commit("f1", 2);
    s.git(&["switch", "-q", "main"]);
    s.commit("m2", 3);
    s.git(&["tag", "v1"]);
    s.git(&["merge", "-q", "--no-ff", "-m", "M", "feature"]);
    s.commit("m3", 5);
    let bare = s.tmp.path().join("origin.git");
    git_in(
        s.tmp.path(),
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    s.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    s.git(&["push", "-q", "-u", "origin", "main"]);
    s.git(&["switch", "-q", "-c", "remote-only"]);
    s.commit("r1", 6);
    s.git(&["push", "-q", "origin", "remote-only"]);
    s.git(&["switch", "-q", "main"]);
    s.git(&["branch", "-q", "-D", "remote-only"]);
    // A tag on a commit no branch reaches.
    s.git(&["switch", "-q", "--detach", "HEAD"]);
    s.commit("t1", 7);
    s.git(&["tag", "only-tag"]);
    s.git(&["switch", "-q", "main"]);
    std::fs::write(s.repo.join("m3.txt"), "changed").unwrap();
    s.git(&["stash", "-q"]);
    s
}

fn query(scope: GraphScope, remotes: bool, tags: bool) -> GraphQuery {
    GraphQuery {
        scope,
        remotes,
        tags,
    }
}

#[test]
fn all_branches_walks_local_remote_and_tags_in_topological_order() {
    let s = repository();
    let subjects = s.subjects(&GraphQuery::default());
    // The stash's own commits are never walked.
    assert!(
        !subjects
            .iter()
            .any(|s| s.contains("WIP") || s.starts_with("index on"))
    );
    for wanted in ["m1", "f1", "m2", "M", "m3", "r1", "t1"] {
        assert!(
            subjects.contains(&wanted.to_string()),
            "{wanted} in {subjects:?}"
        );
    }
    // Every child before its parents.
    let at = |name: &str| subjects.iter().position(|s| s == name).unwrap();
    assert!(at("m3") < at("M"));
    assert!(at("M") < at("f1") && at("M") < at("m2"));
    assert!(at("f1") < at("m1") && at("m2") < at("m1"));
    assert!(at("r1") < at("m3"));
}

#[test]
fn each_filter_walks_only_what_it_names() {
    let s = repository();
    let local = s.subjects(&query(GraphScope::Local, true, true));
    assert!(!local.contains(&"r1".to_string()), "no remote-only commit");
    assert!(!local.contains(&"t1".to_string()), "no tag-only commit");
    assert!(local.contains(&"f1".to_string()));
    let no_remotes = s.subjects(&query(GraphScope::All, false, true));
    assert!(!no_remotes.contains(&"r1".to_string()));
    assert!(no_remotes.contains(&"t1".to_string()));
    let no_tags = s.subjects(&query(GraphScope::All, true, false));
    assert!(!no_tags.contains(&"t1".to_string()));
    assert!(no_tags.contains(&"r1".to_string()));
    s.git(&["switch", "-q", "feature"]);
    let current = s.subjects(&query(GraphScope::Current, true, true));
    assert_eq!(current, ["f1", "m1"]);
}

#[test]
fn labels_name_the_current_branch_remotes_and_tags() {
    let s = repository();
    let mut source = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 0)
        .unwrap();
    let commits = source.next_page(100).unwrap();
    let by = |name: &str| commits.iter().find(|c| c.summary == name).unwrap();
    let m3 = ref_labels(&by("m3").refs);
    assert!(
        m3.iter()
            .any(|l| l.kind == RefKind::Local && l.name == "main" && l.current)
    );
    assert!(
        m3.iter()
            .any(|l| l.kind == RefKind::Remote && l.name == "origin/main")
    );
    assert!(
        ref_labels(&by("m2").refs)
            .iter()
            .any(|l| l.kind == RefKind::Tag && l.name == "v1")
    );
    assert!(
        ref_labels(&by("f1").refs)
            .iter()
            .any(|l| l.kind == RefKind::Local && l.name == "feature" && !l.current)
    );
    assert!(
        ref_labels(&by("r1").refs)
            .iter()
            .any(|l| l.kind == RefKind::Remote && l.name == "origin/remote-only")
    );
    // Detached, HEAD is its own label.
    s.git(&["switch", "-q", "--detach", "v1"]);
    let mut source = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 0)
        .unwrap();
    let commits = source.next_page(100).unwrap();
    let m2 = commits.iter().find(|c| c.summary == "m2").unwrap();
    assert!(ref_labels(&m2.refs).iter().any(|l| l.kind == RefKind::Head));
}

#[test]
fn pages_come_a_few_at_a_time_and_the_end_is_reported() {
    let s = repository();
    let mut source = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 0)
        .unwrap();
    let mut total = 0;
    loop {
        let page = source.next_page(2).unwrap();
        assert!(page.len() <= 2);
        total += page.len();
        if page.len() < 2 {
            break;
        }
    }
    assert!(source.is_done());
    assert_eq!(total, 7);
    assert!(source.next_page(2).unwrap().is_empty());
    // Picking up after the first three gives the same order from there on.
    let all = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 0)
        .unwrap()
        .next_page(100)
        .unwrap();
    let rest = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 3)
        .unwrap()
        .next_page(100)
        .unwrap();
    assert_eq!(rest, all[3..]);
}

#[test]
fn a_walk_pinned_to_its_tips_ignores_what_moved_since() {
    let s = repository();
    let tips = SystemGit
        .graph_tips(&s.repo, &GraphQuery::default())
        .unwrap();
    assert!(!tips.is_empty());
    let main = git_in(&s.repo, &["rev-parse", "main"]).trim().to_string();
    assert!(tips.contains(&main));
    let before = s.subjects(&GraphQuery::default());
    s.commit("later", 9);
    // The tips are what they were: the same walk, without the new commit.
    let mut pinned = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &tips, 0)
        .unwrap();
    let again: Vec<String> = pinned
        .next_page(100)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    assert_eq!(again, before);
    // And the tips now differ, which is how a graph knows to start over.
    assert_ne!(
        SystemGit
            .graph_tips(&s.repo, &GraphQuery::default())
            .unwrap(),
        tips
    );
    // A tip that is not an id is refused before git runs.
    assert!(
        SystemGit
            .graph(&s.repo, &GraphQuery::default(), &["--all".to_string()], 0)
            .is_err()
    );
}

/// 60,000 tags: their ids are far more than one command line holds, and
/// the pinned walk still starts.
#[test]
fn tens_of_thousands_of_refs_pin_and_walk_without_an_argument_list() {
    let s = sandbox();
    s.commit("one", 1);
    s.commit("two", 2);
    let head = git_in(&s.repo, &["rev-parse", "HEAD"]).trim().to_string();
    let parent = git_in(&s.repo, &["rev-parse", "HEAD~1"]).trim().to_string();
    let mut updates = String::new();
    for i in 0..60_000 {
        let target = if i % 2 == 0 { &head } else { &parent };
        updates.push_str(&format!("create refs/tags/t{i:05} {target}\n"));
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&s.repo)
        .args(["update-ref", "--stdin"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(updates.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    git_in(&s.repo, &["pack-refs", "--all"]);
    let tips = SystemGit
        .graph_tips(&s.repo, &GraphQuery::default())
        .unwrap();
    assert!(tips.len() > 60_000, "{} tips", tips.len());
    let mut source = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &tips, 0)
        .unwrap();
    let subjects: Vec<String> = source
        .next_page(100)
        .unwrap()
        .into_iter()
        .map(|c| c.summary)
        .collect();
    assert_eq!(subjects, ["two", "one"]);
    assert!(source.is_done());
}

#[test]
fn an_empty_repository_and_an_unborn_branch_beside_others() {
    let s = sandbox();
    for scope in [GraphScope::All, GraphScope::Local, GraphScope::Current] {
        assert!(s.subjects(&query(scope, true, true)).is_empty());
    }
    s.commit("first", 1);
    s.git(&["switch", "-q", "--orphan", "fresh"]);
    // HEAD has no commit; the other branch is still shown.
    assert_eq!(s.subjects(&GraphQuery::default()), ["first"]);
    assert!(
        s.subjects(&query(GraphScope::Current, true, true))
            .is_empty()
    );
}

#[test]
fn a_folder_that_is_not_a_repository_is_an_error() {
    let s = sandbox();
    let plain = s.tmp.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(
        SystemGit
            .graph(&plain, &GraphQuery::default(), &[], 0)
            .is_err()
    );
}

/// A fast-import stream of `n` commits: `main` with a feature branch every
/// 50 commits that lives for 40 commits (20 of its own, interleaved with
/// main's) and is merged back, plus a release tag every 1,000 commits.
fn history(n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut main: Option<usize> = None;
    let mut feature: Option<(usize, usize, usize)> = None; // (tip, own commits, number)
    let mut branches = 0;
    for i in 0..n {
        let mark = i + 1;
        let time = 1_600_000_000 + i * 60;
        if i % 50 == 10
            && feature.is_none()
            && let Some(tip) = main
        {
            branches += 1;
            feature = Some((tip, 0, branches));
        }
        let on_feature = matches!(feature, Some((_, own, _)) if own < 20) && i % 2 == 1;
        let (branch, parent) = match (on_feature, feature) {
            (true, Some((tip, _, number))) => (format!("refs/heads/feature-{number}"), Some(tip)),
            _ => ("refs/heads/main".to_string(), main),
        };
        let message = format!("Commit {i}\n");
        out.extend(
            format!(
                "commit {branch}\nmark :{mark}\ncommitter Better Git Test <test@example.invalid> {time} +0000\ndata {}\n{message}",
                message.len()
            )
            .bytes(),
        );
        if let Some(parent) = parent {
            out.extend(format!("from :{parent}\n").bytes());
        }
        let merge = !on_feature && matches!(feature, Some((_, own, _)) if own >= 20);
        if merge && let Some((tip, _, _)) = feature {
            out.extend(format!("merge :{tip}\n").bytes());
            feature = None;
        }
        out.extend(
            format!(
                "M 644 inline f{}.txt\ndata {}\n{message}\n",
                i % 100,
                message.len()
            )
            .bytes(),
        );
        if i % 1000 == 999 {
            out.extend(format!("reset refs/tags/r{i}\nfrom :{mark}\n\n").bytes());
        }
        if on_feature {
            if let Some((tip, own, _)) = feature.as_mut() {
                *tip = mark;
                *own += 1;
            }
        } else {
            main = Some(mark);
        }
    }
    out
}

fn import(repo: &Path, stream: &[u8]) {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stream).unwrap();
    assert!(child.wait().unwrap().success());
    git_in(repo, &["reset", "-q", "--hard", "main"]);
}

#[test]
fn a_half_read_history_stops_git_when_dropped() {
    let s = sandbox();
    import(&s.repo, &history(5_000));
    let start = Instant::now();
    let mut source = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 0)
        .unwrap();
    assert_eq!(source.next_page(10).unwrap().len(), 10);
    drop(source);
    assert!(start.elapsed() < Duration::from_secs(10));
    // Everything comes back when read to the end, merges and all.
    let mut source = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 0)
        .unwrap();
    let mut lanes = LaneLayout::new();
    let mut total = 0;
    while !source.is_done() {
        for commit in source.next_page(1_000).unwrap() {
            lanes.push(&commit.sha, &commit.parents);
            total += 1;
        }
    }
    assert_eq!(total, 5_000);
    assert_eq!(lanes.open_lanes(), 0, "every lane met its commit");
}

/// A commit git cannot read the date of, written by hand on top of a long
/// history: the graph still reads it (dated 0), and dropping the stream
/// with git blocked on a full pipe returns at once.
#[test]
fn a_broken_commit_on_a_long_history_neither_hides_it_nor_hangs_the_drop() {
    let s = sandbox();
    import(&s.repo, &history(60_000));
    let tree = git_in(&s.repo, &["rev-parse", "main^{tree}"]);
    let parent = git_in(&s.repo, &["rev-parse", "main"]);
    let object = format!(
        "tree {}\nparent {}\nauthor A <a@example.invalid> notadate +0000\ncommitter A <a@example.invalid> notadate +0000\n\nbroken date\n",
        tree.trim(),
        parent.trim()
    );
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&s.repo)
        .args([
            "hash-object",
            "-t",
            "commit",
            "-w",
            "--literally",
            "--stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(object.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    git_in(&s.repo, &["update-ref", "refs/heads/main", &sha]);

    let mut source = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 0)
        .unwrap();
    let page = source.next_page(100).unwrap();
    let broken = page
        .iter()
        .find(|c| c.sha == sha)
        .expect("the broken commit is listed");
    assert_eq!(broken.author_time, 0);
    assert_eq!(broken.summary, "broken date");
    // git has far more to print than the pipe holds and is blocked on it.
    std::thread::sleep(Duration::from_millis(200));
    let (done, wait) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(source);
        let _ = done.send(());
    });
    assert!(
        wait.recv_timeout(Duration::from_secs(10)).is_ok(),
        "dropping a half-read stream hung"
    );
}

/// The acceptance figure: a generated 100,000-commit repository. Run with
/// `cargo test -p git-platform --test graph --release -- --ignored
/// --nocapture`; it prints the numbers.
#[test]
#[ignore = "builds a 100,000-commit repository; run on purpose for the figures"]
fn a_hundred_thousand_commits_open_in_under_a_second() {
    const PAGE: usize = 3_000;
    let s = sandbox();
    let built = Instant::now();
    import(&s.repo, &history(100_000));
    eprintln!("built the repository in {:?}", built.elapsed());
    let count = git_in(&s.repo, &["rev-list", "--all", "--count"]);
    assert_eq!(count.trim(), "100000");

    let start = Instant::now();
    let mut source = SystemGit
        .graph(&s.repo, &GraphQuery::default(), &[], 0)
        .unwrap();
    let mut lanes = LaneLayout::new();
    let first = source.next_page(PAGE).unwrap();
    let read = start.elapsed();
    let rows: Vec<_> = first
        .iter()
        .map(|c| lanes.push(&c.sha, &c.parents))
        .collect();
    let first_page = start.elapsed();
    assert_eq!(rows.len(), PAGE);
    eprintln!("first page of {PAGE}: read {read:?}, read and laid out {first_page:?}");

    let mut slowest = Duration::ZERO;
    let mut total = PAGE;
    let mut widest = 0;
    while !source.is_done() {
        let page_start = Instant::now();
        let page = source.next_page(PAGE).unwrap();
        for commit in &page {
            widest = widest.max(lanes.push(&commit.sha, &commit.parents).width);
        }
        slowest = slowest.max(page_start.elapsed());
        total += page.len();
    }
    eprintln!(
        "all {total} commits in {:?}; slowest later page {slowest:?}; widest row {widest} columns",
        start.elapsed()
    );
    assert_eq!(total, 100_000);
    assert!(
        first_page < Duration::from_secs(1),
        "first page took {first_page:?}"
    );
    assert!(
        slowest < Duration::from_millis(250),
        "a page took {slowest:?}"
    );
}
