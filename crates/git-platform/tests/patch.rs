//! Committing part of the changes, discarding with a copy in the trash, and
//! ignoring files, against a real `git`.

use std::fs;
use std::path::{Path, PathBuf};

use git_core::{DiffLineKind, FileDiff, change_fingerprint, line_key};
use git_platform::patch::{LineSelection, extension_pattern, path_pattern};
use git_platform::{
    CommitMessage, CommitOptions, DiffOptions, DiffSide, FileSelection, Git, GitError, SystemGit,
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

fn message(summary: &str) -> CommitMessage {
    CommitMessage {
        summary: summary.to_string(),
        description: String::new(),
    }
}

fn diff_with(s: &Sandbox, path: &str, options: DiffOptions) -> FileDiff {
    SystemGit
        .diff_with(&s.repo, Path::new(path), DiffSide::WorkingTree, options)
        .unwrap()
}

/// The lines at `(hunk, line)` of the diff shown with `options`, as the
/// window ticks them.
fn select_in(
    s: &Sandbox,
    path: &str,
    options: DiffOptions,
    pick: impl Fn(usize, usize) -> bool,
) -> LineSelection {
    let diff = diff_with(s, path, options);
    let FileDiff::Text { hunks } = &diff else {
        panic!("binary")
    };
    let mut selected = Vec::new();
    for (h, hunk) in hunks.iter().enumerate() {
        for (l, line) in hunk.lines.iter().enumerate() {
            if line.kind != DiffLineKind::Context && pick(h, l) {
                selected.push(line_key(line));
            }
        }
    }
    LineSelection {
        fingerprint: change_fingerprint(&diff),
        selected,
    }
}

fn select(s: &Sandbox, path: &str, pick: impl Fn(usize, usize) -> bool) -> LineSelection {
    select_in(s, path, DiffOptions::default(), pick)
}

fn lines(diff: &FileDiff) -> Vec<(DiffLineKind, String)> {
    match diff {
        FileDiff::Text { hunks } => hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|l| (l.kind, l.text.clone())))
            .collect(),
        FileDiff::Binary => panic!("binary"),
    }
}

const BASE: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n";

#[test]
fn working_tree_diff_is_against_head_whatever_is_staged() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write("f.txt", &BASE.replace("two", "TWO"));
    s.git(&["add", "f.txt"]);
    s.write("f.txt", &BASE.replace("two", "TWO").replace("ten", "TEN"));
    let diff = SystemGit
        .diff(&s.repo, Path::new("f.txt"), DiffSide::WorkingTree)
        .unwrap();
    let changed: Vec<_> = lines(&diff)
        .into_iter()
        .filter(|(k, _)| *k != DiffLineKind::Context)
        .collect();
    assert_eq!(changed.len(), 4, "both edits, staged or not: {changed:?}");
}

#[test]
fn whitespace_and_whole_file_options() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write("f.txt", &BASE.replace("two", "two   "));
    let hidden = SystemGit
        .diff_with(
            &s.repo,
            Path::new("f.txt"),
            DiffSide::WorkingTree,
            DiffOptions {
                ignore_whitespace: true,
                whole_file: false,
            },
        )
        .unwrap();
    assert!(
        lines(&hidden).is_empty(),
        "a whitespace-only change is hidden"
    );
    let whole = SystemGit
        .diff_with(
            &s.repo,
            Path::new("f.txt"),
            DiffSide::WorkingTree,
            DiffOptions {
                ignore_whitespace: false,
                whole_file: true,
            },
        )
        .unwrap();
    assert_eq!(lines(&whole).len(), 13, "every line of the file is shown");
}

#[test]
fn commit_selected_lines_commits_only_those() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write(
        "f.txt",
        &BASE.replace("two", "TWO").replace("eleven", "ELEVEN"),
    );
    // In hunk 0: context one, -two, +TWO, ...; select both changed lines.
    let selection = FileSelection {
        path: PathBuf::from("f.txt"),
        original_path: None,
        lines: Some(select(&s, "f.txt", |h, l| h == 0 && (l == 1 || l == 2))),
    };
    SystemGit
        .commit_selection(
            &s.repo,
            &[selection],
            &message("only two"),
            &CommitOptions::default(),
        )
        .unwrap();
    let committed = s.git(&["show", "HEAD:f.txt"]);
    assert_eq!(committed, BASE.replace("two", "TWO"));
    assert_eq!(
        s.read("f.txt"),
        BASE.replace("two", "TWO").replace("eleven", "ELEVEN")
    );
    assert_eq!(s.porcelain(), " M f.txt\n");
}

#[test]
fn commit_one_hunk_and_a_whole_file_and_leave_another_out() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.commit_file("g.txt", "g\n", "g");
    s.write(
        "f.txt",
        &BASE.replace("two", "TWO").replace("eleven", "ELEVEN"),
    );
    s.write("g.txt", "G\n");
    s.write("new.txt", "new\n");
    let files = [
        FileSelection {
            path: PathBuf::from("f.txt"),
            original_path: None,
            lines: Some(select(&s, "f.txt", |h, _| h == 1)),
        },
        FileSelection::whole("new.txt"),
    ];
    SystemGit
        .commit_selection(&s.repo, &files, &message("hunk"), &CommitOptions::default())
        .unwrap();
    assert_eq!(
        s.git(&["show", "HEAD:f.txt"]),
        BASE.replace("eleven", "ELEVEN")
    );
    assert_eq!(s.git(&["show", "HEAD:new.txt"]), "new\n");
    assert_eq!(
        s.git(&["show", "HEAD:g.txt"]),
        "g\n",
        "g.txt was not ticked"
    );
}

#[test]
fn partial_commit_of_a_new_file() {
    let s = sandbox();
    s.commit_file("base.txt", "b\n", "base");
    s.write("n.txt", "a\nb\nc\n");
    let selection = FileSelection {
        path: PathBuf::from("n.txt"),
        original_path: None,
        lines: Some(select(&s, "n.txt", |_, l| l == 0 || l == 2)),
    };
    SystemGit
        .commit_selection(
            &s.repo,
            &[selection],
            &message("part"),
            &CommitOptions::default(),
        )
        .unwrap();
    assert_eq!(s.git(&["show", "HEAD:n.txt"]), "a\nc\n");
    assert_eq!(s.read("n.txt"), "a\nb\nc\n");
}

#[test]
fn a_selection_made_on_an_older_diff_is_refused() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write("f.txt", &BASE.replace("two", "TWO"));
    let selection = FileSelection {
        path: PathBuf::from("f.txt"),
        original_path: None,
        lines: Some(LineSelection {
            fingerprint: 99,
            selected: vec![(Some(2), None)],
        }),
    };
    let error = SystemGit
        .commit_selection(
            &s.repo,
            &[selection],
            &message("x"),
            &CommitOptions::default(),
        )
        .unwrap_err();
    assert_eq!(error, GitError::ChangedMeanwhile);
    assert_eq!(s.subjects(), vec!["base"]);
}

fn trashed(s: &Sandbox) -> Vec<String> {
    let dir = s.home.join("data/Trash/files");
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn discard_files_restores_head_and_keeps_a_copy_in_the_trash() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.commit_file("d.txt", "keep\n", "d");
    s.write("f.txt", "edited\n");
    fs::remove_file(s.repo.join("d.txt")).unwrap();
    s.write("u.txt", "untracked\n");
    s.write("a.txt", "added\n");
    s.git(&["add", "a.txt"]);
    let files = ["f.txt", "d.txt", "u.txt", "a.txt"].map(FileSelection::whole);
    SystemGit.discard_files(&s.repo, &files).unwrap();
    assert_eq!(s.porcelain(), "");
    assert_eq!(s.read("f.txt"), BASE);
    assert_eq!(s.read("d.txt"), "keep\n");
    assert!(!s.repo.join("u.txt").exists());
    assert_eq!(trashed(&s), vec!["a.txt", "f.txt", "u.txt"]);
    let copy = fs::read_to_string(s.home.join("data/Trash/files/f.txt")).unwrap();
    assert_eq!(copy, "edited\n");
    let info = fs::read_to_string(s.home.join("data/Trash/info/f.txt.trashinfo")).unwrap();
    assert!(info.starts_with("[Trash Info]\nPath=/"));
    assert!(info.contains("DeletionDate="));
}

#[test]
fn discard_twice_keeps_both_copies() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write("f.txt", "first\n");
    SystemGit
        .discard_files(&s.repo, &[FileSelection::whole("f.txt")])
        .unwrap();
    s.write("f.txt", "second\n");
    SystemGit
        .discard_files(&s.repo, &[FileSelection::whole("f.txt")])
        .unwrap();
    assert_eq!(trashed(&s), vec!["f.txt", "f.txt.2"]);
}

#[test]
fn discard_selected_lines_keeps_the_rest() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write(
        "f.txt",
        &BASE.replace("two", "TWO").replace("eleven", "ELEVEN"),
    );
    // Discard the whole second hunk's change only.
    let selection = FileSelection {
        path: PathBuf::from("f.txt"),
        original_path: None,
        lines: Some(select(&s, "f.txt", |h, _| h == 1)),
    };
    SystemGit.discard_lines(&s.repo, &selection).unwrap();
    assert_eq!(s.read("f.txt"), BASE.replace("two", "TWO"));
    assert_eq!(trashed(&s), vec!["f.txt"]);
}

#[test]
fn discard_one_added_line_of_an_untracked_file() {
    let s = sandbox();
    s.commit_file("base.txt", "b\n", "base");
    s.write("n.txt", "a\nb\nc\n");
    let selection = FileSelection {
        path: PathBuf::from("n.txt"),
        original_path: None,
        lines: Some(select(&s, "n.txt", |_, l| l == 1)),
    };
    SystemGit.discard_lines(&s.repo, &selection).unwrap();
    assert_eq!(s.read("n.txt"), "a\nc\n");
}

#[test]
fn ignore_appends_once_and_patterns_escape() {
    let s = sandbox();
    s.write(".gitignore", "target");
    SystemGit.ignore(&s.repo, "*.log").unwrap();
    SystemGit.ignore(&s.repo, "*.log").unwrap();
    assert_eq!(s.read(".gitignore"), "target\n*.log\n");
    assert!(SystemGit.ignore(&s.repo, " ").is_err());
    assert_eq!(
        extension_pattern(Path::new("a/b.log")).as_deref(),
        Some("*.log")
    );
    assert_eq!(extension_pattern(Path::new("Makefile")), None);
    assert_eq!(path_pattern(Path::new("dir/a[1].txt")), "/dir/a\\[1\\].txt");
    s.write("x.log", "x");
    assert!(!s.porcelain().contains("x.log"));
}

#[test]
fn image_versions_come_from_head_and_the_working_tree() {
    let s = sandbox();
    s.commit_file("i.png", "old", "img");
    s.write("i.png", "new");
    let (old, new) = SystemGit
        .file_versions(&s.repo, Path::new("i.png"), None)
        .unwrap();
    assert_eq!(old.as_deref(), Some(&b"old"[..]));
    assert_eq!(new.as_deref(), Some(&b"new"[..]));
    s.write("added.png", "a");
    let (old, _) = SystemGit
        .file_versions(&s.repo, Path::new("added.png"), None)
        .unwrap();
    assert_eq!(old, None);
}

// --- independent review fixes -------------------------------------------

#[test]
fn partial_commit_and_discard_with_diff_noprefix_touch_the_right_file() {
    let s = sandbox();
    s.commit_file("dir/f.txt", BASE, "base");
    s.commit_file("f.txt", BASE, "same name at the root");
    s.git(&["config", "diff.noprefix", "true"]);
    s.git(&["config", "diff.mnemonicPrefix", "true"]);
    s.write(
        "dir/f.txt",
        &BASE.replace("two", "TWO").replace("eleven", "ELEVEN"),
    );
    let selection = FileSelection {
        path: PathBuf::from("dir/f.txt"),
        original_path: None,
        lines: Some(select(&s, "dir/f.txt", |h, _| h == 0)),
    };
    SystemGit
        .commit_selection(
            &s.repo,
            &[selection],
            &message("first hunk"),
            &CommitOptions::default(),
        )
        .unwrap();
    assert_eq!(
        s.git(&["show", "HEAD:dir/f.txt"]),
        BASE.replace("two", "TWO")
    );
    assert_eq!(
        s.git(&["show", "HEAD:f.txt"]),
        BASE,
        "the root file is untouched"
    );
    let discard = FileSelection {
        path: PathBuf::from("dir/f.txt"),
        original_path: None,
        lines: Some(select(&s, "dir/f.txt", |_, _| true)),
    };
    SystemGit.discard_lines(&s.repo, &discard).unwrap();
    assert_eq!(s.read("dir/f.txt"), BASE.replace("two", "TWO"));
    assert_eq!(s.read("f.txt"), BASE);
}

#[test]
fn a_selection_made_on_the_whole_file_view_commits_the_same_lines() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write(
        "f.txt",
        &BASE.replace("two", "TWO").replace("eleven", "ELEVEN"),
    );
    let whole = DiffOptions {
        ignore_whitespace: false,
        whole_file: true,
    };
    // One hunk in the whole-file view; tick the lines of "two" only.
    let diff = diff_with(&s, "f.txt", whole);
    let FileDiff::Text { hunks } = &diff else {
        panic!()
    };
    assert_eq!(hunks.len(), 1);
    let selection = select_in(&s, "f.txt", whole, |_, l| {
        hunks[0].lines[l].text.eq_ignore_ascii_case("two")
    });
    SystemGit
        .commit_selection(
            &s.repo,
            &[FileSelection {
                path: PathBuf::from("f.txt"),
                original_path: None,
                lines: Some(selection),
            }],
            &message("two"),
            &CommitOptions::default(),
        )
        .unwrap();
    assert_eq!(s.git(&["show", "HEAD:f.txt"]), BASE.replace("two", "TWO"));
}

#[test]
fn a_change_of_the_same_shape_made_meanwhile_is_refused() {
    let s = sandbox();
    s.commit_file("f.txt", BASE, "base");
    s.write("f.txt", &BASE.replace("two", "TWO"));
    let selection = select(&s, "f.txt", |_, _| true);
    // Same hunks, same line counts, different text.
    s.write("f.txt", &BASE.replace("two", "TOO"));
    let error = SystemGit
        .commit_selection(
            &s.repo,
            &[FileSelection {
                path: PathBuf::from("f.txt"),
                original_path: None,
                lines: Some(selection),
            }],
            &message("x"),
            &CommitOptions::default(),
        )
        .unwrap_err();
    assert_eq!(error, GitError::ChangedMeanwhile);
    assert_eq!(s.subjects(), vec!["base"]);
}

#[test]
fn ignore_refuses_a_gitignore_that_is_a_link_out_of_the_repository() {
    let s = sandbox();
    let outside = s.tmp.path().join("outside.txt");
    fs::write(&outside, "precious\n").unwrap();
    std::os::unix::fs::symlink(&outside, s.repo.join(".gitignore")).unwrap();
    assert!(SystemGit.ignore(&s.repo, "*.log").is_err());
    assert_eq!(fs::read_to_string(&outside).unwrap(), "precious\n");
}

#[test]
fn path_patterns_keep_trailing_spaces_and_special_characters() {
    assert_eq!(path_pattern(Path::new("a b ")), "/a b\\ ");
    assert_eq!(path_pattern(Path::new("#x!*?")), "/\\#x\\!\\*\\?");
    let s = sandbox();
    s.write("odd name ", "x");
    s.write("odd name", "y");
    SystemGit
        .ignore(&s.repo, &path_pattern(Path::new("odd name ")))
        .unwrap();
    let untracked: Vec<String> = s
        .git(&["ls-files", "--others", "--exclude-standard", "-z"])
        .split('\0')
        .map(str::to_string)
        .collect();
    assert!(untracked.contains(&"odd name".to_string()), "{untracked:?}");
    assert!(
        !untracked.contains(&"odd name ".to_string()),
        "{untracked:?}"
    );
}

#[test]
fn an_image_that_is_a_link_to_a_device_is_refused_without_reading_it() {
    let s = sandbox();
    s.commit_file("base.txt", "b\n", "base");
    std::os::unix::fs::symlink("/dev/zero", s.repo.join("x.png")).unwrap();
    let started = std::time::Instant::now();
    assert!(
        SystemGit
            .file_versions(&s.repo, Path::new("x.png"), None)
            .is_err()
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let big = git_platform::patch::read_capped(&s.repo.join("base.txt"), 1);
    assert!(big.is_err(), "a file over the cap is refused");
}
