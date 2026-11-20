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
    // The second copy is named the way Better Files names one (ticket 173):
    // the number before the extension, so it still opens as a text file.
    assert_eq!(trashed(&s), vec!["f.1.txt", "f.txt"]);
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

// --- renames (ticket 123): one row, and a diff of only what changed ------------

/// The status rows as (path, original, staged, unstaged, untracked).
/// A status row: path, original path, staged, unstaged, untracked.
type RenameRow = (
    String,
    Option<String>,
    Option<git_core::ChangeKind>,
    Option<git_core::ChangeKind>,
    bool,
);

fn rename_rows(s: &Sandbox) -> Vec<RenameRow> {
    let mut rows: Vec<_> = SystemGit
        .status(&s.repo)
        .unwrap()
        .changes
        .into_iter()
        .map(|c| {
            (
                c.path.to_string_lossy().into_owned(),
                c.original_path.map(|o| o.to_string_lossy().into_owned()),
                c.staged,
                c.unstaged,
                c.untracked,
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn working_diff(s: &Sandbox, path: &str, original: Option<&str>) -> FileDiff {
    SystemGit
        .working_diff(
            &s.repo,
            Path::new(path),
            original.map(Path::new),
            DiffOptions::default(),
        )
        .unwrap()
}

#[test]
fn a_staged_git_mv_is_one_rename_row_whose_diff_is_empty() {
    let s = sandbox();
    s.commit_file("old.txt", BASE, "base");
    s.git(&["mv", "old.txt", "new.txt"]);
    assert_eq!(
        rename_rows(&s),
        [(
            "new.txt".to_string(),
            Some("old.txt".to_string()),
            Some(git_core::ChangeKind::Renamed),
            None,
            false
        )]
    );
    // Read with the original path, the rename has no content change; read
    // without it, it would be a whole new file.
    assert_eq!(
        working_diff(&s, "new.txt", Some("old.txt")),
        FileDiff::Text { hunks: vec![] }
    );
    assert!(lines(&working_diff(&s, "new.txt", None)).len() >= 12);
}

#[test]
fn a_git_mv_with_an_edit_shows_only_the_edit_and_commits_both_paths() {
    let s = sandbox();
    s.commit_file("old.txt", BASE, "base");
    s.git(&["mv", "old.txt", "new.txt"]);
    s.write("new.txt", &BASE.replace("five", "FIVE"));
    let diff = working_diff(&s, "new.txt", Some("old.txt"));
    let changed: Vec<_> = lines(&diff)
        .into_iter()
        .filter(|(k, _)| *k != DiffLineKind::Context)
        .collect();
    assert_eq!(
        changed,
        [
            (DiffLineKind::Removed, "five".to_string()),
            (DiffLineKind::Added, "FIVE".to_string()),
        ]
    );
    // Committing the row commits the rename: the old path goes with it.
    SystemGit
        .commit_selection(
            &s.repo,
            &[FileSelection {
                path: PathBuf::from("new.txt"),
                original_path: Some(PathBuf::from("old.txt")),
                lines: None,
            }],
            &message("rename"),
            &CommitOptions::default(),
        )
        .unwrap();
    assert_eq!(s.porcelain(), "");
    let files = SystemGit.commit_files(&s.repo, &s.head()).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].kind, git_core::ChangeKind::Renamed);
    assert_eq!(
        files[0].original_path.as_deref(),
        Some(Path::new("old.txt"))
    );
}

#[test]
fn a_committed_rename_is_one_file_in_history_with_only_its_edit() {
    let s = sandbox();
    s.commit_file("docs/a.md", BASE, "base");
    s.git(&["mv", "docs/a.md", "docs/b.md"]);
    s.git(&["commit", "-q", "-m", "pure rename"]);
    let files = SystemGit.commit_files(&s.repo, &s.head()).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].path, Path::new("docs/b.md"));
    assert_eq!(
        files[0].original_path.as_deref(),
        Some(Path::new("docs/a.md"))
    );
    let diff = SystemGit
        .commit_diff(&s.repo, &s.head(), &files[0], DiffOptions::default())
        .unwrap();
    assert_eq!(diff, FileDiff::Text { hunks: vec![] }, "no content change");
}

#[test]
fn an_unstaged_move_is_shown_as_git_shows_it_until_both_sides_are_staged() {
    let s = sandbox();
    s.commit_file("old.txt", BASE, "base");
    fs::rename(s.repo.join("old.txt"), s.repo.join("new.txt")).unwrap();
    // No guessing: a deletion and an untracked file.
    assert_eq!(
        rename_rows(&s),
        [
            ("new.txt".to_string(), None, None, None, true),
            (
                "old.txt".to_string(),
                None,
                None,
                Some(git_core::ChangeKind::Deleted),
                false
            ),
        ]
    );
    // Staging both sides makes it the rename it is.
    SystemGit
        .stage(
            &s.repo,
            &[PathBuf::from("old.txt"), PathBuf::from("new.txt")],
        )
        .unwrap();
    let rows = rename_rows(&s);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].1.as_deref(), Some("old.txt"));
    assert_eq!(rows[0].2, Some(git_core::ChangeKind::Renamed));
}

#[test]
fn discarding_a_rename_row_puts_the_old_path_back_and_removes_the_new() {
    let s = sandbox();
    s.commit_file("old.txt", BASE, "base");
    s.git(&["mv", "old.txt", "new.txt"]);
    s.write("new.txt", &BASE.replace("five", "FIVE"));
    SystemGit
        .discard_files(
            &s.repo,
            &[FileSelection {
                path: PathBuf::from("new.txt"),
                original_path: Some(PathBuf::from("old.txt")),
                lines: None,
            }],
        )
        .unwrap();
    assert_eq!(s.porcelain(), "", "both paths back as HEAD has them");
    assert_eq!(s.read("old.txt"), BASE);
    assert!(!s.repo.join("new.txt").exists());
}

#[test]
fn a_rename_left_out_of_a_commit_stays_a_rename_with_both_paths() {
    let s = sandbox();
    s.commit_file("old.txt", BASE, "base");
    s.commit_file("other.txt", "o\n", "other");
    s.git(&["mv", "old.txt", "new.txt"]);
    s.write("other.txt", "o2\n");
    SystemGit
        .commit_selection(
            &s.repo,
            &[FileSelection::whole("other.txt")],
            &message("only other"),
            &CommitOptions::default(),
        )
        .unwrap();
    let files = SystemGit.commit_files(&s.repo, &s.head()).unwrap();
    let paths: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(
        paths,
        [PathBuf::from("other.txt")],
        "neither side of the rename"
    );
    let rows = rename_rows(&s);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].0, "new.txt");
    assert_eq!(rows[0].1.as_deref(), Some("old.txt"));
}

#[test]
fn unstaging_a_rename_takes_both_paths_back_to_head() {
    let s = sandbox();
    s.commit_file("old.txt", BASE, "base");
    s.git(&["mv", "old.txt", "new.txt"]);
    SystemGit
        .unstage(
            &s.repo,
            &[PathBuf::from("new.txt"), PathBuf::from("old.txt")],
        )
        .unwrap();
    // As git shows an unstaged move: the old path deleted, the new untracked.
    let rows = rename_rows(&s);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.iter().any(|r| r.0 == "new.txt" && r.4));
    assert!(
        rows.iter()
            .any(|r| r.0 == "old.txt" && r.3 == Some(git_core::ChangeKind::Deleted))
    );
}

#[test]
fn a_rename_with_a_mode_change_is_one_row_and_a_copy_is_an_added_file() {
    use std::os::unix::fs::PermissionsExt;
    let s = sandbox();
    s.commit_file("run.sh", "echo hi\n", "base");
    s.git(&["mv", "run.sh", "start.sh"]);
    fs::set_permissions(s.repo.join("start.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    s.git(&["add", "start.sh"]);
    let rows = rename_rows(&s);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].1.as_deref(), Some("run.sh"));
    // The content is the same: no hunks, only the mode git notes.
    assert_eq!(
        working_diff(&s, "start.sh", Some("run.sh")),
        FileDiff::Text { hunks: vec![] }
    );
    s.git(&["commit", "-q", "-m", "rename and chmod"]);
    let files = SystemGit.commit_files(&s.repo, &s.head()).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].kind, git_core::ChangeKind::Renamed);

    // A copy: history reads renames only (-M), so the copy is the new file
    // it is, and the original is untouched.
    fs::copy(s.repo.join("start.sh"), s.repo.join("copy.sh")).unwrap();
    s.git(&["add", "copy.sh"]);
    s.git(&["commit", "-q", "-m", "copy"]);
    let files = SystemGit.commit_files(&s.repo, &s.head()).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].kind, git_core::ChangeKind::Added);
    assert_eq!(files[0].original_path, None);
}

/// Ticket 173's review: a submodule's row reached Discard, which copied the
/// whole submodule, `.git` included, into the trash and reset its commit.
/// The parent's Discard now refuses a submodule and changes nothing.
#[test]
fn discard_refuses_a_submodule_and_changes_nothing() {
    let s = sandbox();
    s.commit_file("r.txt", "root\n", "root");
    let inner = s.tmp.path().join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    harness::git_in(&inner, &["init", "-q", "-b", "main"]);
    std::fs::write(inner.join("a.txt"), "a\n").unwrap();
    harness::git_in(&inner, &["add", "a.txt"]);
    harness::git_in(&inner, &["commit", "-q", "-m", "inner"]);
    s.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        inner.to_str().unwrap(),
        "sub",
    ]);
    s.git(&["commit", "-q", "-m", "add the submodule"]);
    s.write("sub/a.txt", "changed in the submodule\n");

    let status = SystemGit.status(&s.repo).unwrap();
    assert!(
        status
            .changes
            .iter()
            .any(|change| change.path == Path::new("sub")),
        "the submodule shows as a change of the parent: {:?}",
        status.changes
    );
    let error = SystemGit
        .discard_files(&s.repo, &[FileSelection::whole("sub")])
        .unwrap_err();
    assert_eq!(error, GitError::Submodule(PathBuf::from("sub")));
    assert!(trashed(&s).is_empty(), "nothing went to the trash");
    assert_eq!(s.read("sub/a.txt"), "changed in the submodule\n");

    // A selection with a submodule in it refuses as a whole.
    s.write("r.txt", "edited\n");
    let error = SystemGit
        .discard_files(
            &s.repo,
            &[FileSelection::whole("r.txt"), FileSelection::whole("sub")],
        )
        .unwrap_err();
    assert_eq!(error, GitError::Submodule(PathBuf::from("sub")));
    assert_eq!(s.read("r.txt"), "edited\n");
}
