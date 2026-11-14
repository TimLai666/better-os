//! What Better Git carries over when an update restarts it (ticket 105), with
//! no GPUI in it.
//!
//! The session part of the view is the open repository, the page and tab, the
//! file or commit selected, the filter, and the commit message being typed.
//! The repository opens through the job queue like any other, so the rest is
//! held until it has opened and put back then; a file that is no longer
//! changed, or a commit that is no longer in the first page of history, is
//! simply not selected.

use std::path::PathBuf;

use better_ui::restart::Blocker;
use serde::{Deserialize, Serialize};

use crate::session::{GitSession, Page, Tab};

/// The session's part of the window's view.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionView {
    pub repository: Option<PathBuf>,
    pub settings_page: bool,
    pub history_tab: bool,
    pub selected_file: Option<PathBuf>,
    pub selected_commit: Option<String>,
    pub filter: String,
    pub whole_file: bool,
    pub summary: String,
    pub description: String,
    pub co_authors: String,
    pub show_co_authors: bool,
    pub amend: bool,
    pub no_verify: bool,
    pub allow_empty: bool,
    pub push_after: bool,
}

/// Everything the window puts back: the session's view and the window's own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitView {
    pub session: SessionView,
    pub show_filter: bool,
    pub show_commit_options: bool,
    pub settings_section: usize,
    pub branch_tab: usize,
    pub diff_scroll: f32,
}

impl GitSession {
    /// The view as it is now.
    pub fn view(&self) -> SessionView {
        let mut view = SessionView {
            settings_page: self.page == Page::Settings,
            ..SessionView::default()
        };
        if let Some(repo) = &self.repo {
            view.repository = Some(repo.root.clone());
            view.history_tab = repo.tab == Tab::History;
            view.selected_file = repo.selected_file.clone();
            view.selected_commit = repo
                .history
                .selected
                .and_then(|index| repo.history.commits.get(index))
                .map(|commit| commit.sha.clone());
            view.filter = repo.filter.clone();
            view.whole_file = repo.whole_file;
            let form = &repo.form;
            view.summary = form.summary.clone();
            view.description = form.description.clone();
            view.co_authors = form.co_authors.clone();
            view.show_co_authors = form.show_co_authors;
            view.amend = form.amend;
            view.no_verify = form.no_verify;
            view.allow_empty = form.allow_empty;
            view.push_after = form.push_after;
        }
        view
    }

    /// Opens the repository the view was of and puts the rest back once it
    /// has opened. A repository that is not trusted any more asks first, as
    /// any open does.
    pub fn restore_view(&mut self, view: SessionView) {
        if view.settings_page {
            self.page = Page::Settings;
        }
        if let Some(path) = view.repository.clone() {
            self.restoring = Some(view);
            self.open(path);
        }
    }

    /// Called when a repository has opened: the held view is put back if it
    /// is of this repository.
    pub(crate) fn restore_after_open(&mut self) {
        let Some(view) = self.restoring.take() else {
            return;
        };
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        if view.repository.as_deref() != Some(repo.root.as_path()) {
            return;
        }
        repo.filter = view.filter.clone();
        repo.whole_file = view.whole_file;
        let form = &mut repo.form;
        form.summary = view.summary.clone();
        form.description = view.description.clone();
        form.co_authors = view.co_authors.clone();
        form.show_co_authors = view.show_co_authors;
        form.amend = view.amend;
        form.no_verify = view.no_verify;
        form.allow_empty = view.allow_empty;
        form.push_after = view.push_after;
        // The window copies the form into its text fields when this moves.
        form.generation += 1;
        if view.settings_page {
            self.page = Page::Settings;
        }
        if let Some(path) = &view.selected_file {
            self.select_file(path);
        }
        if view.history_tab {
            self.restoring_commit = view.selected_commit.clone();
            self.set_tab(Tab::History);
        }
    }

    /// Called when the first page of history has arrived.
    pub(crate) fn restore_after_log(&mut self) {
        let Some(sha) = self.restoring_commit.take() else {
            return;
        };
        let index = self.repo.as_ref().and_then(|repo| {
            repo.history
                .commits
                .iter()
                .position(|commit| commit.sha == sha)
        });
        if let Some(index) = index {
            self.select_commit(index);
        }
    }

    /// What a restart would lose, if anything: an open dialog, whose typed
    /// input has no saved form, or a git command running or waiting, or a
    /// background fetch.
    pub fn restart_blocked(&self) -> Option<Blocker> {
        if self.dialog.is_some() {
            Some(Blocker::Dialog)
        } else if self.is_busy() || self.is_background_fetching() {
            Some(Blocker::Task)
        } else {
            None
        }
    }
}

/// Whether the settings page's Git identity fields (name, email, default
/// branch) hold typed text that differs from what git's configuration says.
/// That text has no saved form, so a restart waits for it to be saved or
/// put back. Before the configuration has been read there is nothing to
/// differ from.
pub fn settings_unsaved(config_loaded: bool, typed: [&str; 3], saved: [&str; 3]) -> bool {
    config_loaded && typed != saved
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use git_core::{BranchHead, ChangeKind, Commit, FileChange, FileDiff, RepositoryStatus};
    use git_platform::{DiffSide, Git, GitError};

    use super::{GitView, SessionView, settings_unsaved};
    use crate::dialogs::Dialog;
    use crate::session::{Files, GitSession, Page, Tab};
    use better_ui::restart::Blocker;

    /// One repository with two changed files and three commits.
    struct Fake;

    fn commit(sha: &str) -> Commit {
        Commit {
            sha: sha.into(),
            parents: vec![],
            author_name: "A".into(),
            author_email: "a@example.invalid".into(),
            author_time: 0,
            refs: vec![],
            summary: format!("commit {sha}"),
            body: String::new(),
        }
    }

    impl Git for Fake {
        fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError> {
            Ok(path.to_path_buf())
        }
        fn status(&self, _: &Path) -> Result<RepositoryStatus, GitError> {
            let change = |path: &str| FileChange {
                path: PathBuf::from(path),
                original_path: None,
                staged: None,
                unstaged: Some(ChangeKind::Modified),
                untracked: false,
                conflicted: false,
                conflict: None,
            };
            Ok(RepositoryStatus {
                head: BranchHead::Branch("main".into()),
                unborn: false,
                head_oid: None,
                upstream: None,
                ahead_behind: None,
                changes: vec![change("a.txt"), change("b.txt")],
                stash_count: 0,
            })
        }
        fn diff(&self, _: &Path, _: &Path, _: DiffSide) -> Result<FileDiff, GitError> {
            Ok(FileDiff::Text { hunks: vec![] })
        }
        fn stage(&self, _: &Path, _: &[PathBuf]) -> Result<(), GitError> {
            Ok(())
        }
        fn unstage(&self, _: &Path, _: &[PathBuf]) -> Result<(), GitError> {
            Ok(())
        }
        fn commit(&self, _: &Path, _: &git_platform::CommitMessage) -> Result<(), GitError> {
            Ok(())
        }
        fn log(&self, _: &Path, _: &str, skip: usize, _: usize) -> Result<Vec<Commit>, GitError> {
            Ok(if skip == 0 {
                vec![commit("c3"), commit("c2"), commit("c1")]
            } else {
                vec![]
            })
        }
        fn info(&self, _: &Path) -> Result<git_platform::RepositoryInfo, GitError> {
            Ok(git_platform::RepositoryInfo::default())
        }
        fn commit_files(&self, _: &Path, _: &str) -> Result<Vec<git_core::CommitFile>, GitError> {
            Ok(vec![])
        }
    }

    fn opened() -> GitSession {
        let mut session = GitSession::new(Arc::new(Fake), Files::default());
        session.trust_and_open(PathBuf::from("/work/hello"));
        session.run_git_jobs();
        session
    }

    #[test]
    fn the_commit_message_and_the_selected_file_come_back() {
        let mut before = opened();
        before.select_file(Path::new("b.txt"));
        before.run_git_jobs();
        let repo = before.repo.as_mut().unwrap();
        repo.filter = "b".into();
        repo.form.summary = "Fix the thing".into();
        repo.form.description = "Half way through a sentence".into();
        repo.form.amend = true;
        let view = before.view();
        assert_eq!(view.repository, Some(PathBuf::from("/work/hello")));

        // Through the hand-off's own encoding, as the next program reads it.
        let text = serde_json::to_string(&GitView {
            session: view.clone(),
            ..GitView::default()
        })
        .unwrap();
        let carried: GitView = serde_json::from_str(&text).unwrap();

        let mut after = GitSession::new(Arc::new(Fake), Files::default());
        after.trust.trust(Path::new("/work/hello")).ok();
        let generation = 0;
        after.restore_view(carried.session);
        after.run_git_jobs();
        let repo = after.repo.as_ref().expect("the repository opened again");
        assert_eq!(repo.selected_file.as_deref(), Some(Path::new("b.txt")));
        assert_eq!(repo.filter, "b");
        assert_eq!(repo.form.summary, "Fix the thing");
        assert_eq!(repo.form.description, "Half way through a sentence");
        assert!(repo.form.amend);
        assert!(
            repo.form.generation > generation,
            "the window copies the text"
        );
        assert_eq!(after.view(), view);
    }

    #[test]
    fn the_history_tab_and_its_selected_commit_come_back() {
        let mut before = opened();
        before.set_tab(Tab::History);
        before.run_git_jobs();
        before.select_commit(1);
        before.run_git_jobs();
        let view = before.view();
        assert!(view.history_tab);
        assert_eq!(view.selected_commit.as_deref(), Some("c2"));

        let mut after = GitSession::new(Arc::new(Fake), Files::default());
        after.trust.trust(Path::new("/work/hello")).ok();
        after.restore_view(view.clone());
        after.run_git_jobs();
        let repo = after.repo.as_ref().unwrap();
        assert_eq!(repo.tab, Tab::History);
        assert_eq!(repo.history.selected, Some(1));
        assert_eq!(after.view().selected_commit, view.selected_commit);
    }

    #[test]
    fn a_file_that_is_no_longer_changed_is_not_selected() {
        let view = SessionView {
            repository: Some(PathBuf::from("/work/hello")),
            selected_file: Some(PathBuf::from("gone.txt")),
            ..SessionView::default()
        };
        let mut after = GitSession::new(Arc::new(Fake), Files::default());
        after.trust.trust(Path::new("/work/hello")).ok();
        after.restore_view(view);
        after.run_git_jobs();
        assert_eq!(after.repo.as_ref().unwrap().selected_file, None);
    }

    #[test]
    fn the_settings_page_comes_back_without_a_repository() {
        let mut after = GitSession::new(Arc::new(Fake), Files::default());
        after.restore_view(SessionView {
            settings_page: true,
            ..SessionView::default()
        });
        assert_eq!(after.page, Page::Settings);
        assert!(after.repo.is_none());
    }

    #[test]
    fn a_running_command_or_an_open_dialog_holds_the_restart() {
        let mut session = GitSession::new(Arc::new(Fake), Files::default());
        assert_eq!(session.restart_blocked(), None);
        session.trust_and_open(PathBuf::from("/work/hello"));
        assert_eq!(
            session.restart_blocked(),
            Some(Blocker::Task),
            "the open is waiting in the queue"
        );
        session.run_git_jobs();
        assert_eq!(session.restart_blocked(), None);
        session.dialog = Some(Dialog::Trust {
            programs: vec![],
            path: PathBuf::from("/work/other"),
        });
        assert_eq!(session.restart_blocked(), Some(Blocker::Dialog));
    }

    #[test]
    fn typed_but_unsaved_identity_settings_hold_the_restart() {
        let saved = ["Ada", "ada@example.invalid", "main"];
        assert!(!settings_unsaved(true, saved, saved));
        assert!(settings_unsaved(
            true,
            ["Ada L", "ada@example.invalid", "main"],
            saved
        ));
        assert!(
            !settings_unsaved(false, ["Ada L", "", ""], ["", "", ""]),
            "nothing to differ from before git's configuration is read"
        );
    }

    #[test]
    fn an_older_or_newer_view_still_reads() {
        let view: GitView = serde_json::from_str(r#"{"session":{"filter":"x"},"unknown":1}"#)
            .expect("missing and extra fields are fine");
        assert_eq!(view.session.filter, "x");
        assert_eq!(view.branch_tab, 0);
    }
}
