//! The session's rules, driven through a fake `Git`, and the strings.
//!
//! Real `git` runs in `tests/session.rs`. Here the fake records every call and
//! keeps a status it changes the way git would, so each rule can be checked
//! without a repository.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use git_core::{
    BranchHead, ChangeKind, DiffLine, DiffLineKind, FileChange, FileDiff, Hunk, RepositoryStatus,
};
use git_platform::{CommitMessage, DiffSide, Git, GitError};

use crate::i18n::{self, Locale};
use crate::recent;
use crate::session::{
    Badge, Check, DiffPane, GitSession, OutcomeKind, Problem, Request, Selection, truncate_leading,
};

const ROOT: &str = "/work/repo";

#[derive(Default)]
struct FakeState {
    /// Paths that open, and the root they open as.
    roots: Vec<(PathBuf, PathBuf)>,
    status: Option<RepositoryStatus>,
    /// What `diff` answers for one path and side; anything else gets a
    /// one-line text diff.
    diffs: Vec<(PathBuf, DiffSide, FileDiff)>,
    calls: Vec<String>,
    commits: Vec<CommitMessage>,
    fail_commit: Option<GitError>,
    fail_stage: Option<GitError>,
    fail_status: Option<GitError>,
}

#[derive(Default)]
struct FakeGit {
    state: Mutex<FakeState>,
}

impl FakeGit {
    fn with(status: RepositoryStatus) -> Arc<Self> {
        let fake = FakeGit::default();
        {
            let mut state = fake.state.lock().unwrap();
            state.roots.push((PathBuf::from(ROOT), PathBuf::from(ROOT)));
            state
                .roots
                .push((PathBuf::from("/work/repo/src"), PathBuf::from(ROOT)));
            state.status = Some(status);
        }
        Arc::new(fake)
    }

    fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }

    fn clear_calls(&self) {
        self.state.lock().unwrap().calls.clear();
    }

    fn set<F: FnOnce(&mut FakeState)>(&self, change: F) {
        change(&mut self.state.lock().unwrap());
    }
}

fn names(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(",")
}

impl Git for FakeGit {
    fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(format!("root {}", path.display()));
        state
            .roots
            .iter()
            .find(|(asked, _)| asked == path)
            .map(|(_, root)| root.clone())
            .ok_or_else(|| GitError::NotARepository(path.to_path_buf()))
    }

    fn status(&self, _repository: &Path) -> Result<RepositoryStatus, GitError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push("status".to_string());
        if let Some(error) = state.fail_status.clone() {
            return Err(error);
        }
        Ok(state.status.clone().expect("the fake has a status"))
    }

    fn diff(&self, _repository: &Path, path: &Path, side: DiffSide) -> Result<FileDiff, GitError> {
        let mut state = self.state.lock().unwrap();
        state
            .calls
            .push(format!("diff {} {:?}", path.display(), side));
        let found = state
            .diffs
            .iter()
            .find(|(p, s, _)| p == path && *s == side)
            .map(|(_, _, diff)| diff.clone());
        Ok(found.unwrap_or_else(|| one_line_diff(&format!("{} {:?}", path.display(), side))))
    }

    fn stage(&self, _repository: &Path, paths: &[PathBuf]) -> Result<(), GitError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(format!("stage {}", names(paths)));
        if let Some(error) = state.fail_stage.clone() {
            return Err(error);
        }
        let status = state.status.as_mut().unwrap();
        for change in status.changes.iter_mut() {
            if paths.contains(&change.path) {
                if change.untracked {
                    change.staged = Some(ChangeKind::Added);
                    change.untracked = false;
                } else {
                    change.staged = change.staged.or(change.unstaged);
                }
                change.unstaged = None;
            }
        }
        Ok(())
    }

    fn unstage(&self, _repository: &Path, paths: &[PathBuf]) -> Result<(), GitError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(format!("unstage {}", names(paths)));
        let status = state.status.as_mut().unwrap();
        for change in status.changes.iter_mut() {
            if paths.contains(&change.path) {
                if change.staged == Some(ChangeKind::Added) && change.unstaged.is_none() {
                    change.untracked = true;
                } else {
                    change.unstaged = change.unstaged.or(change.staged);
                }
                change.staged = None;
            }
        }
        Ok(())
    }

    fn commit(&self, _repository: &Path, message: &CommitMessage) -> Result<(), GitError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push("commit".to_string());
        if let Some(error) = state.fail_commit.clone() {
            return Err(error);
        }
        state.commits.push(message.clone());
        let status = state.status.as_mut().unwrap();
        status
            .changes
            .retain(|change| change.staged.is_none() || change.unstaged.is_some());
        for change in status.changes.iter_mut() {
            change.staged = None;
        }
        status.unborn = false;
        Ok(())
    }
}

fn one_line_diff(text: &str) -> FileDiff {
    FileDiff::Text {
        hunks: vec![Hunk {
            header: "@@ -1 +1 @@".to_string(),
            lines: vec![DiffLine {
                kind: DiffLineKind::Added,
                text: text.to_string(),
                old_line: None,
                new_line: Some(1),
                no_newline_at_end: false,
            }],
        }],
    }
}

fn change(path: &str) -> FileChange {
    FileChange {
        path: PathBuf::from(path),
        original_path: None,
        staged: None,
        unstaged: None,
        untracked: false,
        conflicted: false,
    }
}

fn modified(path: &str) -> FileChange {
    FileChange {
        unstaged: Some(ChangeKind::Modified),
        ..change(path)
    }
}

fn staged(path: &str, kind: ChangeKind) -> FileChange {
    FileChange {
        staged: Some(kind),
        ..change(path)
    }
}

fn untracked(path: &str) -> FileChange {
    FileChange {
        untracked: true,
        ..change(path)
    }
}

fn conflicted(path: &str) -> FileChange {
    FileChange {
        conflicted: true,
        ..change(path)
    }
}

fn both(path: &str) -> FileChange {
    FileChange {
        unstaged: Some(ChangeKind::Modified),
        ..staged(path, ChangeKind::Modified)
    }
}

fn status(changes: Vec<FileChange>) -> RepositoryStatus {
    RepositoryStatus {
        head: BranchHead::Branch("main".to_string()),
        unborn: false,
        upstream: None,
        ahead_behind: None,
        changes,
    }
}

fn opened(fake: &Arc<FakeGit>) -> GitSession {
    let mut session = GitSession::new(fake.clone(), None);
    session.open(ROOT);
    session.run_pending();
    assert_eq!(session.repository(), Some(Path::new(ROOT)));
    fake.clear_calls();
    session
}

// ---- Opening ------------------------------------------------------------

#[test]
fn opening_a_folder_inside_a_repository_opens_its_root() {
    let fake = FakeGit::with(status(vec![modified("a.rs")]));
    let mut session = GitSession::new(fake.clone(), None);
    session.open("/work/repo/src");
    session.run_pending();
    assert_eq!(session.repository(), Some(Path::new(ROOT)));
    assert_eq!(session.repository_name().as_deref(), Some("repo"));
    assert_eq!(session.branch(), Some("main"));
    assert_eq!(session.rows().len(), 1);
    assert_eq!(session.recent(), &[PathBuf::from(ROOT)]);
}

#[test]
fn a_folder_that_is_not_a_repository_says_so_and_opens_nothing() {
    let fake = FakeGit::with(status(vec![modified("a.rs")]));
    let mut session = GitSession::new(fake.clone(), None);
    session.open("/tmp/plain");
    session.run_pending();
    assert_eq!(session.repository(), None);
    assert_eq!(
        session.problem(),
        Some(&Problem::Git(GitError::NotARepository(PathBuf::from(
            "/tmp/plain"
        ))))
    );
    assert!(session.recent().is_empty());
}

#[test]
fn a_failed_open_keeps_the_repository_that_was_open() {
    let fake = FakeGit::with(status(vec![modified("a.rs")]));
    let mut session = opened(&fake);
    session.select(Path::new("a.rs"));
    session.run_pending();
    session.open("/tmp/plain");
    session.run_pending();
    assert_eq!(session.repository(), Some(Path::new(ROOT)));
    assert_eq!(session.rows().len(), 1);
    assert!(session.selection().is_some());
    assert!(matches!(
        session.problem(),
        Some(Problem::Git(GitError::NotARepository(_)))
    ));
}

#[test]
fn git_missing_is_a_message_not_a_crash() {
    let fake = FakeGit::with(status(vec![]));
    fake.set(|state| state.fail_status = Some(GitError::NotInstalled));
    let mut session = GitSession::new(fake.clone(), None);
    session.open(ROOT);
    session.run_pending();
    assert_eq!(session.repository(), None);
    assert_eq!(
        session.problem(),
        Some(&Problem::Git(GitError::NotInstalled))
    );
    session.dismiss_error();
    assert_eq!(session.problem(), None);
}

#[test]
fn a_successful_open_clears_an_earlier_error() {
    let fake = FakeGit::with(status(vec![]));
    let mut session = GitSession::new(fake.clone(), None);
    session.open("/tmp/plain");
    session.run_pending();
    assert!(session.problem().is_some());
    session.open(ROOT);
    session.run_pending();
    assert_eq!(session.problem(), None);
}

// ---- The changes list ---------------------------------------------------

#[test]
fn rows_carry_each_state_and_checkbox() {
    let renamed = FileChange {
        original_path: Some(PathBuf::from("old.rs")),
        ..staged("src/new.rs", ChangeKind::Renamed)
    };
    let fake = FakeGit::with(status(vec![
        modified("src/deep/dir/m.rs"),
        staged("added.rs", ChangeKind::Added),
        staged("gone.rs", ChangeKind::Deleted),
        renamed,
        untracked("notes.txt"),
        conflicted("clash.rs"),
        both("both.rs"),
    ]));
    let session = opened(&fake);
    let rows = session.rows();
    let summary: Vec<(&str, &str, Badge, Check, bool)> = rows
        .iter()
        .map(|row| {
            (
                row.name.as_str(),
                row.folder.as_str(),
                row.badge,
                row.check,
                row.stageable,
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                "m.rs",
                "src/deep/dir",
                Badge::Modified,
                Check::Unchecked,
                true
            ),
            ("added.rs", "", Badge::Added, Check::Checked, true),
            ("gone.rs", "", Badge::Deleted, Check::Checked, true),
            ("new.rs", "src", Badge::Renamed, Check::Checked, true),
            ("notes.txt", "", Badge::Added, Check::Unchecked, true),
            ("clash.rs", "", Badge::Conflicted, Check::Unchecked, false),
            ("both.rs", "", Badge::Modified, Check::Indeterminate, true),
        ]
    );
    assert_eq!(Badge::Conflicted.letter(), "!");
}

#[test]
fn a_staged_file_changed_again_is_indeterminate_and_its_box_stages_the_rest() {
    let fake = FakeGit::with(status(vec![both("both.rs")]));
    let mut session = opened(&fake);
    assert_eq!(session.rows()[0].check, Check::Indeterminate);
    session.toggle_stage(Path::new("both.rs"));
    session.run_pending();
    assert_eq!(fake.calls(), vec!["stage both.rs", "status"]);
    assert_eq!(session.rows()[0].check, Check::Checked);
}

#[test]
fn the_checkbox_stages_and_unstages_exactly_that_file() {
    let fake = FakeGit::with(status(vec![modified("a.rs"), modified("b.rs")]));
    let mut session = opened(&fake);
    session.toggle_stage(Path::new("a.rs"));
    session.run_pending();
    assert_eq!(fake.calls(), vec!["stage a.rs", "status"]);
    let checks: Vec<Check> = session.rows().iter().map(|row| row.check).collect();
    assert_eq!(checks, vec![Check::Checked, Check::Unchecked]);

    fake.clear_calls();
    session.toggle_stage(Path::new("a.rs"));
    session.run_pending();
    assert_eq!(fake.calls(), vec!["unstage a.rs", "status"]);
}

#[test]
fn unstaging_a_rename_names_both_of_its_paths() {
    let renamed = FileChange {
        original_path: Some(PathBuf::from("old.rs")),
        ..staged("new.rs", ChangeKind::Renamed)
    };
    let fake = FakeGit::with(status(vec![renamed]));
    let mut session = opened(&fake);
    session.toggle_stage(Path::new("new.rs"));
    session.run_pending();
    assert_eq!(fake.calls()[0], "unstage new.rs,old.rs");
}

#[test]
fn a_conflicted_file_is_never_staged() {
    let fake = FakeGit::with(status(vec![conflicted("clash.rs"), modified("a.rs")]));
    let mut session = opened(&fake);
    session.toggle_stage(Path::new("clash.rs"));
    session.run_pending();
    assert!(fake.calls().is_empty());
    session.stage_all();
    session.run_pending();
    assert_eq!(fake.calls()[0], "stage a.rs");
}

#[test]
fn the_header_checkbox_stages_everything_then_unstages_everything() {
    let fake = FakeGit::with(status(vec![
        modified("a.rs"),
        staged("b.rs", ChangeKind::Added),
        untracked("c.txt"),
    ]));
    let mut session = opened(&fake);
    assert_eq!(session.all_check(), Check::Indeterminate);
    session.toggle_all();
    session.run_pending();
    assert_eq!(fake.calls()[0], "stage a.rs,c.txt");
    assert_eq!(session.all_check(), Check::Checked);

    fake.clear_calls();
    session.toggle_all();
    session.run_pending();
    assert_eq!(fake.calls()[0], "unstage a.rs,b.rs,c.txt");
    assert_eq!(session.all_check(), Check::Unchecked);
}

#[test]
fn nothing_to_stage_runs_nothing() {
    let fake = FakeGit::with(status(vec![]));
    let mut session = opened(&fake);
    assert_eq!(session.all_check(), Check::Unchecked);
    session.stage_all();
    session.unstage_all();
    session.toggle_stage(Path::new("missing.rs"));
    session.run_pending();
    assert!(fake.calls().is_empty());
}

#[test]
fn a_failed_stage_is_shown_and_the_status_is_read_again() {
    let fake = FakeGit::with(status(vec![modified("a.rs")]));
    fake.set(|state| {
        state.fail_stage = Some(GitError::Failed {
            exit_code: Some(128),
            message: "fatal: Unable to create '.git/index.lock': File exists.".to_string(),
        })
    });
    let mut session = opened(&fake);
    session.toggle_stage(Path::new("a.rs"));
    session.run_pending();
    assert_eq!(fake.calls(), vec!["stage a.rs", "status"]);
    let (_, message) = i18n::EN_US.problem(session.problem().unwrap());
    assert!(message.contains("index.lock"), "{message}");
}

// ---- Selection and the diff ---------------------------------------------

#[test]
fn selecting_a_row_reads_its_diff_and_the_checkbox_does_not_change_it() {
    let fake = FakeGit::with(status(vec![modified("a.rs"), modified("b.rs")]));
    let mut session = opened(&fake);
    assert_eq!(session.diff_pane(), DiffPane::NothingSelected);
    session.select(Path::new("a.rs"));
    session.run_pending();
    assert_eq!(fake.calls(), vec!["diff a.rs Unstaged"]);
    assert!(matches!(session.diff_pane(), DiffPane::Text(_)));
    assert!(session.rows()[0].selected);

    session.toggle_stage(Path::new("b.rs"));
    session.run_pending();
    assert_eq!(
        session.selection(),
        Some(&Selection {
            path: PathBuf::from("a.rs"),
            side: DiffSide::Unstaged
        })
    );
}

#[test]
fn a_fully_staged_selection_shows_its_staged_side() {
    let fake = FakeGit::with(status(vec![modified("a.rs")]));
    let mut session = opened(&fake);
    session.select(Path::new("a.rs"));
    session.run_pending();
    fake.clear_calls();
    session.toggle_stage(Path::new("a.rs"));
    session.run_pending();
    assert_eq!(
        fake.calls(),
        vec!["stage a.rs", "status", "diff a.rs Staged"]
    );
    assert_eq!(session.selection().unwrap().side, DiffSide::Staged);
    assert_eq!(
        session.diff().unwrap().selection.side,
        DiffSide::Staged,
        "the diff shown is the one for the side now selected"
    );
}

#[test]
fn a_selection_whose_file_is_gone_after_a_refresh_is_cleared() {
    let fake = FakeGit::with(status(vec![modified("a.rs"), modified("b.rs")]));
    let mut session = opened(&fake);
    session.select(Path::new("a.rs"));
    session.run_pending();
    fake.set(|state| {
        state.status.as_mut().unwrap().changes.remove(0);
    });
    session.refresh();
    session.run_pending();
    assert_eq!(session.selection(), None);
    assert_eq!(session.diff(), None);
    assert_eq!(session.diff_pane(), DiffPane::NothingSelected);
}

#[test]
fn binary_empty_and_conflicted_diffs_have_their_own_states() {
    let fake = FakeGit::with(status(vec![
        modified("image.png"),
        modified("mode.sh"),
        conflicted("clash.rs"),
    ]));
    fake.set(|state| {
        state.diffs.push((
            PathBuf::from("image.png"),
            DiffSide::Unstaged,
            FileDiff::Binary,
        ));
        state.diffs.push((
            PathBuf::from("mode.sh"),
            DiffSide::Unstaged,
            FileDiff::Text { hunks: vec![] },
        ));
    });
    let mut session = opened(&fake);
    session.select(Path::new("image.png"));
    session.run_pending();
    assert_eq!(session.diff_pane(), DiffPane::Binary);
    session.select(Path::new("mode.sh"));
    session.run_pending();
    assert_eq!(session.diff_pane(), DiffPane::NoChanges);

    fake.clear_calls();
    session.select(Path::new("clash.rs"));
    session.run_pending();
    assert_eq!(session.diff_pane(), DiffPane::Conflicted);
    assert!(
        fake.calls().is_empty(),
        "a conflicted file's diff is not read"
    );
}

#[test]
fn only_the_newest_selection_is_read() {
    let fake = FakeGit::with(status(vec![
        modified("a.rs"),
        modified("b.rs"),
        modified("c.rs"),
    ]));
    let mut session = opened(&fake);
    session.select(Path::new("a.rs"));
    session.select(Path::new("b.rs"));
    session.select(Path::new("c.rs"));
    session.run_pending();
    assert_eq!(fake.calls(), vec!["diff c.rs Unstaged"]);
}

#[test]
fn the_other_side_of_a_partly_staged_file_can_be_shown() {
    let fake = FakeGit::with(status(vec![both("both.rs"), modified("only.rs")]));
    let mut session = opened(&fake);
    session.select(Path::new("both.rs"));
    session.run_pending();
    assert_eq!(session.selection().unwrap().side, DiffSide::Unstaged);
    session.show_side(DiffSide::Staged);
    session.run_pending();
    assert_eq!(session.diff().unwrap().selection.side, DiffSide::Staged);

    session.select(Path::new("only.rs"));
    session.run_pending();
    fake.clear_calls();
    session.show_side(DiffSide::Staged);
    session.run_pending();
    assert!(fake.calls().is_empty(), "only.rs has nothing staged");
}

// ---- Committing ---------------------------------------------------------

#[test]
fn commit_needs_something_staged_and_a_summary() {
    let fake = FakeGit::with(status(vec![modified("a.rs")]));
    let mut session = opened(&fake);
    session.summary = "Fix the parser".to_string();
    assert!(!session.can_commit(), "nothing is staged");
    session.commit();
    session.run_pending();
    assert!(fake.calls().is_empty());

    session.toggle_stage(Path::new("a.rs"));
    session.run_pending();
    session.summary = "   ".to_string();
    assert!(!session.can_commit(), "the summary is blank");
    session.summary = "Fix the parser".to_string();
    assert!(session.can_commit());
}

#[test]
fn a_commit_empties_the_form_and_reads_the_status_again() {
    let fake = FakeGit::with(status(vec![staged("a.rs", ChangeKind::Modified)]));
    let mut session = opened(&fake);
    session.select(Path::new("a.rs"));
    session.run_pending();
    session.summary = "  Fix the parser ".to_string();
    session.description = "Reads the hunk header.\n".to_string();
    let generation = session.form_generation();
    fake.clear_calls();

    session.commit();
    assert!(
        !session.can_commit(),
        "a running commit is not offered twice"
    );
    session.run_pending();

    assert_eq!(fake.calls(), vec!["commit", "status"]);
    let commits = fake.state.lock().unwrap().commits.clone();
    assert_eq!(
        commits,
        vec![CommitMessage {
            summary: "Fix the parser".to_string(),
            description: "Reads the hunk header.".to_string(),
        }]
    );
    assert_eq!(session.summary, "");
    assert_eq!(session.description, "");
    assert!(session.form_generation() > generation);
    assert!(session.rows().is_empty());
    assert_eq!(session.selection(), None);
    assert_eq!(session.problem(), None);
}

#[test]
fn a_refused_commit_shows_gits_message_and_keeps_the_form() {
    let fake = FakeGit::with(status(vec![staged("a.rs", ChangeKind::Modified)]));
    fake.set(|state| {
        state.fail_commit = Some(GitError::Failed {
            exit_code: Some(1),
            message: "pre-commit hook failed".to_string(),
        })
    });
    let mut session = opened(&fake);
    session.summary = "Fix".to_string();
    session.commit();
    session.run_pending();
    assert_eq!(session.summary, "Fix");
    assert!(session.can_commit(), "it can be tried again");
    let (title, message) = i18n::ZH_TW.problem(session.problem().unwrap());
    assert_eq!(title, i18n::ZH_TW.failed_title);
    assert_eq!(message, "pre-commit hook failed");
}

#[test]
fn the_commit_button_names_the_branch() {
    assert_eq!(i18n::EN_US.commit_label(Some("main")), "Commit to main");
    assert_eq!(i18n::ZH_TW.commit_label(Some("main")), "提交到 main");
    assert_eq!(i18n::EN_US.commit_label(None), "Commit");
}

#[test]
fn a_detached_head_has_no_branch() {
    let mut detached = status(vec![]);
    detached.head = BranchHead::Detached;
    let fake = FakeGit::with(detached);
    let session = opened(&fake);
    assert_eq!(session.branch(), None);
}

#[test]
fn a_repository_with_no_commit_yet_opens_and_commits() {
    let mut first = status(vec![untracked("README.md")]);
    first.unborn = true;
    let fake = FakeGit::with(first);
    let mut session = opened(&fake);
    assert_eq!(session.branch(), Some("main"));
    session.toggle_stage(Path::new("README.md"));
    session.run_pending();
    session.summary = "First commit".to_string();
    session.commit();
    session.run_pending();
    assert!(session.rows().is_empty());
    assert_eq!(session.problem(), None);
}

// ---- Driving ------------------------------------------------------------

#[test]
fn one_request_runs_at_a_time_in_order() {
    let fake = FakeGit::with(status(vec![modified("a.rs"), modified("b.rs")]));
    let mut session = opened(&fake);
    session.toggle_stage(Path::new("a.rs"));
    session.toggle_stage(Path::new("b.rs"));
    let first = session.next_request().expect("a request is queued");
    assert!(session.next_request().is_none(), "one is already running");
    assert!(session.is_busy());
    assert!(matches!(first, Request::Stage { .. }));
    let outcome = first.run(fake.as_ref());
    session.apply(outcome);
    let second = session.next_request().expect("the second runs next");
    session.apply(second.run(fake.as_ref()));
    assert!(!session.is_busy());
    assert_eq!(
        fake.calls(),
        vec!["stage a.rs", "status", "stage b.rs", "status"]
    );
}

#[test]
fn an_answer_for_a_repository_that_was_closed_is_ignored() {
    let fake = FakeGit::with(status(vec![modified("a.rs")]));
    let mut session = opened(&fake);
    session.toggle_stage(Path::new("a.rs"));
    let request = session.next_request().unwrap();
    session.close_repository();
    let outcome = request.run(fake.as_ref());
    assert_eq!(outcome.kind, OutcomeKind::Refreshed);
    session.apply(outcome);
    assert_eq!(session.repository(), None);
    assert_eq!(session.status(), None);
    assert!(!session.is_busy());
}

#[test]
fn closing_drops_requests_for_the_closed_repository() {
    let fake = FakeGit::with(status(vec![modified("a.rs")]));
    let mut session = opened(&fake);
    session.toggle_stage(Path::new("a.rs"));
    session.close_repository();
    session.run_pending();
    assert!(fake.calls().is_empty());
}

// ---- Recent repositories ------------------------------------------------

#[test]
fn recent_repositories_are_remembered_newest_first_without_duplicates() {
    let a = PathBuf::from("/a");
    let b = PathBuf::from("/b");
    let list = recent::remember(&[], a.clone());
    let list = recent::remember(&list, b.clone());
    let list = recent::remember(&list, a.clone());
    assert_eq!(list, vec![a, b]);

    let mut long = Vec::new();
    for index in 0..(recent::LIMIT + 5) {
        long = recent::remember(&long, PathBuf::from(format!("/r{index}")));
    }
    assert_eq!(long.len(), recent::LIMIT);
    assert_eq!(long[0], PathBuf::from(format!("/r{}", recent::LIMIT + 4)));
}

#[test]
fn recent_repositories_survive_a_save_and_a_load() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file = tmp.path().join("state/better-os/git-recent.json");
    assert!(
        recent::load(&file).is_empty(),
        "a missing file is an empty list"
    );
    let list = vec![PathBuf::from("/work/one"), PathBuf::from("/work/two")];
    recent::save(&file, &list).unwrap();
    assert_eq!(recent::load(&file), list);
    assert!(
        !file.with_extension("json.partial").exists(),
        "the partial file is renamed into place"
    );
}

#[test]
fn an_unreadable_recent_file_is_an_empty_list() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file = tmp.path().join("git-recent.json");
    std::fs::write(&file, b"{ not json").unwrap();
    assert!(recent::load(&file).is_empty());
    std::fs::write(
        &file,
        br#"{"repositories": ["relative/path", "/abs", "/abs"]}"#,
    )
    .unwrap();
    assert_eq!(recent::load(&file), vec![PathBuf::from("/abs")]);
}

#[test]
fn the_session_writes_the_recent_list_to_its_own_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file = tmp.path().join("git-recent.json");
    let fake = FakeGit::with(status(vec![]));
    let mut session = GitSession::new(fake.clone(), Some(file.clone()));
    session.open(ROOT);
    session.run_pending();
    assert_eq!(recent::load(&file), vec![PathBuf::from(ROOT)]);
    let again = GitSession::new(fake, Some(file));
    assert_eq!(again.recent(), &[PathBuf::from(ROOT)]);
}

#[test]
fn the_recent_file_lives_in_the_xdg_state_directory() {
    assert_eq!(
        recent::state_path(Some("/state".into()), Some("/home/me".into())),
        Some(PathBuf::from("/state/better-os/git-recent.json"))
    );
    assert_eq!(
        recent::state_path(Some("relative".into()), Some("/home/me".into())),
        Some(PathBuf::from(
            "/home/me/.local/state/better-os/git-recent.json"
        ))
    );
    assert_eq!(recent::state_path(None, None), None);
}

// ---- Strings ------------------------------------------------------------

#[test]
fn both_languages_define_every_string_and_none_is_empty() {
    let english = i18n::EN_US.all();
    let chinese = i18n::ZH_TW.all();
    assert_eq!(english.len(), chinese.len());
    for text in english.iter().chain(chinese.iter()) {
        assert!(!text.trim().is_empty());
    }
    for (en, zh) in english.iter().zip(chinese.iter()) {
        for placeholder in ["{branch}", "{path}", "{detail}"] {
            assert_eq!(
                en.contains(placeholder),
                zh.contains(placeholder),
                "{en:?} and {zh:?} must carry the same placeholders"
            );
        }
    }
}

#[test]
fn the_zh_tw_copy_is_the_specs() {
    let c = &i18n::ZH_TW;
    assert_eq!(c.changes, "變更");
    assert_eq!(c.summary_placeholder, "摘要（必填）");
    assert_eq!(c.description_placeholder, "說明");
    assert_eq!(c.commit_to, "提交到 {branch}");
    assert_eq!(c.open, "開啟…");
    assert_eq!(c.open_title, "開啟儲存庫");
    assert_eq!(c.open_detail, "選擇含有 Git 儲存庫的資料夾。");
    assert_eq!(c.no_changes, "沒有變更");
    assert_eq!(c.select_file, "選擇檔案以檢視變更");
    assert_eq!(c.binary_file, "二進位檔");
    assert_eq!(c.conflicted, "衝突");
}

#[test]
fn every_problem_has_a_title_and_a_message() {
    let problems = [
        Problem::Git(GitError::NotInstalled),
        Problem::Git(GitError::NotARepository(PathBuf::from("/tmp/plain"))),
        Problem::Git(GitError::Failed {
            exit_code: Some(1),
            message: "nothing added to commit".to_string(),
        }),
        Problem::Git(GitError::UnreadableOutput("`1` record".to_string())),
        Problem::FolderPicker("no portal".to_string()),
    ];
    for c in [&i18n::EN_US, &i18n::ZH_TW] {
        for problem in &problems {
            let (title, message) = c.problem(problem);
            assert!(!title.is_empty() && !message.is_empty(), "{problem:?}");
            assert!(!message.contains('{'), "{message}");
        }
        let (_, message) = c.problem(&problems[1]);
        assert!(message.contains("/tmp/plain"));
    }
}

#[test]
fn the_language_comes_from_lang_or_the_flag() {
    assert_eq!(Locale::from_lang("zh_TW.UTF-8"), Locale::ZhTw);
    assert_eq!(Locale::from_lang("zh_Hant"), Locale::ZhTw);
    assert_eq!(Locale::from_lang("en_US.UTF-8"), Locale::EnUs);
    assert_eq!(Locale::from_lang(""), Locale::EnUs);
    assert_eq!(Locale::from_argument("zh-TW"), Some(Locale::ZhTw));
    assert_eq!(Locale::from_argument("en_us"), Some(Locale::EnUs));
    assert_eq!(Locale::from_argument("fr"), None);
}

#[test]
fn a_long_folder_is_cut_at_its_leading_end() {
    assert_eq!(truncate_leading("src", 10), "src");
    assert_eq!(truncate_leading("crates/git-core/src", 10), "…-core/src");
    assert_eq!(
        truncate_leading("crates/git-core/src", 10).chars().count(),
        10
    );
    assert_eq!(truncate_leading("資料夾/子資料夾/深處", 5), "…夾/深處");
}
