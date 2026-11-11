//! Everything the Better Git window decides, with no GPUI in it.
//!
//! The session holds the open repository, its status, the selected file and
//! its diff, the commit form, the recent repositories, and the error the
//! window is showing. Every change to the repository goes through a
//! [`Request`], and every answer comes back as an [`Outcome`]:
//!
//! - the session puts requests in a queue ([`GitSession::open`],
//!   [`GitSession::toggle_stage`], [`GitSession::commit`], ...);
//! - whoever drives it takes one at a time with
//!   [`GitSession::next_request`], runs it with [`Request::run`], and hands the
//!   result to [`GitSession::apply`].
//!
//! The window runs [`Request::run`] on a background thread, so no `git`
//! process ever blocks a frame. Tests run the same three steps in a loop with
//! [`GitSession::run_pending`]. One request is in flight at a time: two `git`
//! processes writing one index at once would fail on git's own lock.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use git_core::{BranchHead, ChangeKind, FileChange, FileDiff, RepositoryStatus};
use git_platform::{CommitMessage, DiffSide, Git, GitError};

use crate::recent;

/// Which file the diff pane is about, and which side of its change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    pub path: PathBuf,
    pub side: DiffSide,
}

/// A file's diff, with the selection it was read for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShownDiff {
    pub selection: Selection,
    pub diff: FileDiff,
}

/// What the window has to tell the person about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Problem {
    /// git, or the attempt to start it, refused.
    Git(GitError),
    /// The folder picker could not be shown. The text is the platform's.
    FolderPicker(String),
}

/// A checkbox's three states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Check {
    Unchecked,
    Checked,
    /// Staged, and changed again since.
    Indeterminate,
}

/// The letter in a row's state square.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Badge {
    Modified,
    Added,
    Deleted,
    Renamed,
    Conflicted,
}

impl Badge {
    pub fn letter(self) -> &'static str {
        match self {
            Badge::Modified => "M",
            Badge::Added => "A",
            Badge::Deleted => "D",
            Badge::Renamed => "R",
            Badge::Conflicted => "!",
        }
    }
}

/// One row of the changes list, ready to draw.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRow {
    pub path: PathBuf,
    /// The file name, decoded lossily for display.
    pub name: String,
    /// The parent folder relative to the repository root; empty at the root.
    pub folder: String,
    pub badge: Badge,
    pub check: Check,
    /// False for a conflicted file: staging it would mark the conflict
    /// resolved, and resolving conflicts is a later ticket.
    pub stageable: bool,
    pub selected: bool,
}

/// What the diff pane shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiffPane<'a> {
    NothingSelected,
    /// A conflicted file. git's combined diff for it is not read in this slice.
    Conflicted,
    Binary,
    NoChanges,
    Text(&'a [git_core::Hunk]),
}

/// One thing to ask git, with everything it needs captured, so it can run on
/// any thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    Open {
        path: PathBuf,
    },
    Refresh {
        repository: PathBuf,
        selection: Option<Selection>,
    },
    Diff {
        repository: PathBuf,
        selection: Selection,
    },
    Stage {
        repository: PathBuf,
        paths: Vec<PathBuf>,
        selection: Option<Selection>,
    },
    Unstage {
        repository: PathBuf,
        paths: Vec<PathBuf>,
        selection: Option<Selection>,
    },
    Commit {
        repository: PathBuf,
        message: CommitMessage,
    },
}

/// What kind of answer an [`Outcome`] is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutcomeKind {
    /// A repository opened. `Outcome::repository` is its root.
    Opened,
    /// A path could not be opened; the current repository stays.
    OpenFailed,
    /// A status, after a refresh, a stage, or an unstage.
    Refreshed,
    /// A commit was made, and the status after it.
    Committed,
    /// A commit was refused, and the status after it.
    CommitFailed,
    /// Only a diff.
    Diff,
}

/// The answer to one [`Request`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Outcome {
    pub kind: OutcomeKind,
    /// The repository the request was about. For `OpenFailed` it is the path
    /// that was asked for.
    pub repository: PathBuf,
    pub status: Option<RepositoryStatus>,
    pub diff: Option<(Selection, Result<FileDiff, GitError>)>,
    pub error: Option<GitError>,
}

impl Request {
    /// Runs the request against `git`. This is the only place the session's
    /// requests reach git, and it touches nothing but its own arguments, so it
    /// is safe to run on a background thread.
    ///
    /// A stage, an unstage, or a commit is always followed by a status, even
    /// when it failed, because a failure part-way can still have changed the
    /// index. The diff of the selected file is read in the same run.
    pub fn run(self, git: &dyn Git) -> Outcome {
        match self {
            Request::Open { path } => {
                let opened = git
                    .repository_root(&path)
                    .and_then(|root| git.status(&root).map(|status| (root, status)));
                match opened {
                    Ok((root, status)) => Outcome {
                        kind: OutcomeKind::Opened,
                        repository: root,
                        status: Some(status),
                        diff: None,
                        error: None,
                    },
                    Err(error) => Outcome {
                        kind: OutcomeKind::OpenFailed,
                        repository: path,
                        status: None,
                        diff: None,
                        error: Some(error),
                    },
                }
            }
            Request::Refresh {
                repository,
                selection,
            } => refreshed(git, repository, selection, OutcomeKind::Refreshed, None),
            Request::Diff {
                repository,
                selection,
            } => {
                let diff = git.diff(&repository, &selection.path, selection.side);
                Outcome {
                    kind: OutcomeKind::Diff,
                    repository,
                    status: None,
                    diff: Some((selection, diff)),
                    error: None,
                }
            }
            Request::Stage {
                repository,
                paths,
                selection,
            } => {
                let error = git.stage(&repository, &paths).err();
                refreshed(git, repository, selection, OutcomeKind::Refreshed, error)
            }
            Request::Unstage {
                repository,
                paths,
                selection,
            } => {
                let error = git.unstage(&repository, &paths).err();
                refreshed(git, repository, selection, OutcomeKind::Refreshed, error)
            }
            Request::Commit {
                repository,
                message,
            } => match git.commit(&repository, &message) {
                Ok(()) => refreshed(git, repository, None, OutcomeKind::Committed, None),
                Err(error) => refreshed(
                    git,
                    repository,
                    None,
                    OutcomeKind::CommitFailed,
                    Some(error),
                ),
            },
        }
    }
}

/// Reads the status, and the diff of the selection as it stands in that
/// status. A status that fails keeps the earlier error if there was one, since
/// that one is what the person did.
fn refreshed(
    git: &dyn Git,
    repository: PathBuf,
    selection: Option<Selection>,
    kind: OutcomeKind,
    error: Option<GitError>,
) -> Outcome {
    match git.status(&repository) {
        Ok(status) => {
            let diff = selection
                .and_then(|selection| resolve_selection(&status, &selection))
                .filter(|selection| !is_conflicted(&status, &selection.path))
                .map(|selection| {
                    let diff = git.diff(&repository, &selection.path, selection.side);
                    (selection, diff)
                });
            Outcome {
                kind,
                repository,
                status: Some(status),
                diff,
                error,
            }
        }
        Err(status_error) => Outcome {
            kind,
            repository,
            status: None,
            diff: None,
            error: Some(error.unwrap_or(status_error)),
        },
    }
}

fn find<'a>(status: &'a RepositoryStatus, path: &Path) -> Option<&'a FileChange> {
    status.changes.iter().find(|change| change.path == path)
}

fn is_conflicted(status: &RepositoryStatus, path: &Path) -> bool {
    find(status, path).is_some_and(|change| change.conflicted)
}

/// Whether the file has a change on this side.
fn has_side(change: &FileChange, side: DiffSide) -> bool {
    match side {
        DiffSide::Unstaged => change.untracked || change.unstaged.is_some(),
        DiffSide::Staged => change.staged.is_some(),
    }
}

/// The side a newly selected file shows: what is not staged yet when there is
/// any, because that is what the person is about to decide on, and otherwise
/// what is staged.
pub fn default_side(change: &FileChange) -> DiffSide {
    if has_side(change, DiffSide::Unstaged) || !has_side(change, DiffSide::Staged) {
        DiffSide::Unstaged
    } else {
        DiffSide::Staged
    }
}

/// The selection after a new status: gone when the file no longer has a
/// change, and moved to the other side when the side it showed has nothing
/// left (a file that was just staged in full shows its staged diff).
pub fn resolve_selection(status: &RepositoryStatus, selection: &Selection) -> Option<Selection> {
    let change = find(status, &selection.path)?;
    let side = if change.conflicted || has_side(change, selection.side) {
        selection.side
    } else {
        default_side(change)
    };
    Some(Selection {
        path: selection.path.clone(),
        side,
    })
}

/// A row's checkbox: checked when every change of the file is staged,
/// indeterminate when it is staged and changed again, unchecked otherwise.
pub fn check_of(change: &FileChange) -> Check {
    if change.conflicted || change.untracked || change.staged.is_none() {
        Check::Unchecked
    } else if change.unstaged.is_some() {
        Check::Indeterminate
    } else {
        Check::Checked
    }
}

/// The letter a row shows: what the index records when there is a staged
/// change, otherwise what the working tree shows.
pub fn badge_of(change: &FileChange) -> Badge {
    if change.conflicted {
        return Badge::Conflicted;
    }
    if change.untracked {
        return Badge::Added;
    }
    match change.staged.or(change.unstaged) {
        Some(ChangeKind::Added) | Some(ChangeKind::Copied) => Badge::Added,
        Some(ChangeKind::Deleted) => Badge::Deleted,
        Some(ChangeKind::Renamed) => Badge::Renamed,
        Some(ChangeKind::Modified) | Some(ChangeKind::TypeChanged) | None => Badge::Modified,
    }
}

/// `text` cut at its leading end to at most `max_chars` characters, with an
/// ellipsis in front, so the end of a folder path (the part nearest the file)
/// stays readable.
pub fn truncate_leading(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let tail: String = text.chars().skip(count - keep).collect();
    format!("…{tail}")
}

/// The paths an unstage of this change has to name. A staged rename is two
/// index entries, the new path and the removal of the old one; naming only
/// the new path would leave the old file deleted in the index.
fn unstage_paths(change: &FileChange) -> Vec<PathBuf> {
    let mut paths = vec![change.path.clone()];
    if change.staged.is_some() {
        paths.extend(change.original_path.clone());
    }
    paths
}

/// The window's state.
pub struct GitSession {
    git: Arc<dyn Git>,
    repository: Option<PathBuf>,
    status: Option<RepositoryStatus>,
    selection: Option<Selection>,
    diff: Option<ShownDiff>,
    /// The commit form. The window copies its text fields into these.
    pub summary: String,
    pub description: String,
    /// Goes up each time the session empties the form itself, so the window
    /// knows to empty its text fields too.
    form_generation: u64,
    recent: Vec<PathBuf>,
    recent_file: Option<PathBuf>,
    problem: Option<Problem>,
    queue: VecDeque<Request>,
    in_flight: bool,
    /// True while the request that is running opens a repository.
    opening: bool,
    committing: bool,
}

impl GitSession {
    /// A session with no repository open. `recent_file` is where the recent
    /// repositories are kept; `None` keeps them in memory only.
    pub fn new(git: Arc<dyn Git>, recent_file: Option<PathBuf>) -> Self {
        let recent = recent_file.as_deref().map(recent::load).unwrap_or_default();
        Self {
            git,
            repository: None,
            status: None,
            selection: None,
            diff: None,
            summary: String::new(),
            description: String::new(),
            form_generation: 0,
            recent,
            recent_file,
            problem: None,
            queue: VecDeque::new(),
            in_flight: false,
            opening: false,
            committing: false,
        }
    }

    // ---- Reading --------------------------------------------------------

    pub fn git(&self) -> Arc<dyn Git> {
        self.git.clone()
    }

    pub fn repository(&self) -> Option<&Path> {
        self.repository.as_deref()
    }

    /// The repository's folder name, for the command bar.
    pub fn repository_name(&self) -> Option<String> {
        let repository = self.repository.as_deref()?;
        Some(
            repository
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| repository.to_string_lossy().into_owned()),
        )
    }

    pub fn status(&self) -> Option<&RepositoryStatus> {
        self.status.as_ref()
    }

    /// The branch HEAD is on, or `None` when HEAD is detached or nothing is
    /// open.
    pub fn branch(&self) -> Option<&str> {
        match &self.status.as_ref()?.head {
            BranchHead::Branch(name) => Some(name),
            BranchHead::Detached => None,
        }
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    pub fn diff(&self) -> Option<&ShownDiff> {
        self.diff.as_ref()
    }

    pub fn recent(&self) -> &[PathBuf] {
        &self.recent
    }

    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    pub fn form_generation(&self) -> u64 {
        self.form_generation
    }

    pub fn is_busy(&self) -> bool {
        self.in_flight || !self.queue.is_empty()
    }

    fn changes(&self) -> &[FileChange] {
        self.status
            .as_ref()
            .map(|status| status.changes.as_slice())
            .unwrap_or_default()
    }

    /// The changes list, in git's order.
    pub fn rows(&self) -> Vec<ChangeRow> {
        self.changes()
            .iter()
            .map(|change| {
                let name = change
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| change.path.to_string_lossy().into_owned());
                let folder = change
                    .path
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ChangeRow {
                    path: change.path.clone(),
                    name,
                    folder,
                    badge: badge_of(change),
                    check: check_of(change),
                    stageable: !change.conflicted,
                    selected: self
                        .selection
                        .as_ref()
                        .is_some_and(|selection| selection.path == change.path),
                }
            })
            .collect()
    }

    /// The header checkbox: checked when every file that can be staged is
    /// staged in full, unchecked when none has anything staged.
    pub fn all_check(&self) -> Check {
        let stageable: Vec<Check> = self
            .changes()
            .iter()
            .filter(|change| !change.conflicted)
            .map(check_of)
            .collect();
        if stageable.is_empty() || stageable.iter().all(|check| *check == Check::Unchecked) {
            Check::Unchecked
        } else if stageable.iter().all(|check| *check == Check::Checked) {
            Check::Checked
        } else {
            Check::Indeterminate
        }
    }

    pub fn has_staged(&self) -> bool {
        self.changes().iter().any(|change| change.staged.is_some())
    }

    /// Commit is offered with something staged and a summary that is not
    /// blank, and not while a commit is already running. It is also withheld
    /// while another repository is being opened: that open drops whatever is
    /// queued for this one, and a commit must never vanish without a word.
    pub fn can_commit(&self) -> bool {
        self.repository.is_some()
            && self.has_staged()
            && !self.summary.trim().is_empty()
            && !self.committing
            && !self.opening
            && !self
                .queue
                .iter()
                .any(|request| matches!(request, Request::Open { .. }))
    }

    /// What the diff pane shows.
    pub fn diff_pane(&self) -> DiffPane<'_> {
        let Some(selection) = &self.selection else {
            return DiffPane::NothingSelected;
        };
        if self
            .status
            .as_ref()
            .is_some_and(|status| is_conflicted(status, &selection.path))
        {
            return DiffPane::Conflicted;
        }
        // The previous diff stays on screen until the new one arrives.
        match self.diff.as_ref().map(|shown| &shown.diff) {
            None => DiffPane::NothingSelected,
            Some(FileDiff::Binary) => DiffPane::Binary,
            Some(FileDiff::Text { hunks }) if hunks.is_empty() => DiffPane::NoChanges,
            Some(FileDiff::Text { hunks }) => DiffPane::Text(hunks),
        }
    }

    // ---- Asking ---------------------------------------------------------

    fn enqueue(&mut self, request: Request) {
        if matches!(request, Request::Diff { .. }) {
            // Only the newest selection's diff is worth reading.
            self.queue
                .retain(|queued| !matches!(queued, Request::Diff { .. }));
        }
        self.queue.push_back(request);
    }

    /// Opens the repository that contains `path`. A path that is not in a
    /// repository is an error and leaves the open repository as it was.
    pub fn open(&mut self, path: impl Into<PathBuf>) {
        self.enqueue(Request::Open { path: path.into() });
    }

    pub fn refresh(&mut self) {
        if let Some(repository) = self.repository.clone() {
            self.enqueue(Request::Refresh {
                repository,
                selection: self.selection.clone(),
            });
        }
    }

    /// Selects a file and reads its diff. A path that is not in the changes
    /// list is ignored.
    pub fn select(&mut self, path: &Path) {
        let Some(change) = self.changes().iter().find(|change| change.path == path) else {
            return;
        };
        let selection = Selection {
            path: change.path.clone(),
            side: default_side(change),
        };
        let conflicted = change.conflicted;
        self.select_side(selection, conflicted);
    }

    /// Shows the other side of the selected file, when it has one.
    pub fn show_side(&mut self, side: DiffSide) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        let Some(change) = self.status.as_ref().and_then(|s| find(s, &selection.path)) else {
            return;
        };
        if !has_side(change, side) {
            return;
        }
        let conflicted = change.conflicted;
        self.select_side(
            Selection {
                path: selection.path,
                side,
            },
            conflicted,
        );
    }

    fn select_side(&mut self, selection: Selection, conflicted: bool) {
        self.selection = Some(selection.clone());
        if conflicted {
            return;
        }
        if let Some(repository) = self.repository.clone() {
            self.enqueue(Request::Diff {
                repository,
                selection,
            });
        }
    }

    /// The row's checkbox: stages a file that is unstaged or staged and
    /// changed again, unstages a file that is staged in full. A conflicted
    /// file is left alone.
    pub fn toggle_stage(&mut self, path: &Path) {
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let Some(change) = self.changes().iter().find(|change| change.path == path) else {
            return;
        };
        if change.conflicted {
            return;
        }
        let selection = self.selection.clone();
        let request = match check_of(change) {
            Check::Checked => Request::Unstage {
                repository,
                paths: unstage_paths(change),
                selection,
            },
            Check::Unchecked | Check::Indeterminate => Request::Stage {
                repository,
                paths: vec![change.path.clone()],
                selection,
            },
        };
        self.enqueue(request);
    }

    /// The header checkbox: unstages everything when everything is staged,
    /// otherwise stages everything that can be.
    pub fn toggle_all(&mut self) {
        match self.all_check() {
            Check::Checked => self.unstage_all(),
            Check::Unchecked | Check::Indeterminate => self.stage_all(),
        }
    }

    /// Stages every file that is not staged in full, except conflicted ones.
    pub fn stage_all(&mut self) {
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let paths: Vec<PathBuf> = self
            .changes()
            .iter()
            .filter(|change| !change.conflicted && check_of(change) != Check::Checked)
            .map(|change| change.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        let selection = self.selection.clone();
        self.enqueue(Request::Stage {
            repository,
            paths,
            selection,
        });
    }

    /// Unstages every file that has something staged.
    pub fn unstage_all(&mut self) {
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let paths: Vec<PathBuf> = self
            .changes()
            .iter()
            .filter(|change| change.staged.is_some())
            .flat_map(unstage_paths)
            .collect();
        if paths.is_empty() {
            return;
        }
        let selection = self.selection.clone();
        self.enqueue(Request::Unstage {
            repository,
            paths,
            selection,
        });
    }

    /// Commits what is staged with the form's summary and description. Does
    /// nothing when [`Self::can_commit`] is false.
    pub fn commit(&mut self) {
        if !self.can_commit() {
            return;
        }
        let Some(repository) = self.repository.clone() else {
            return;
        };
        self.committing = true;
        self.enqueue(Request::Commit {
            repository,
            message: CommitMessage {
                summary: self.summary.trim().to_string(),
                description: self.description.trim().to_string(),
            },
        });
    }

    /// Goes back to the repository chooser. Requests already queued for the
    /// repository are dropped, and an answer still on its way is ignored.
    pub fn close_repository(&mut self) {
        self.repository = None;
        self.status = None;
        self.selection = None;
        self.diff = None;
        self.committing = false;
        self.clear_form();
        self.queue
            .retain(|request| matches!(request, Request::Open { .. }));
    }

    pub fn dismiss_error(&mut self) {
        self.problem = None;
    }

    /// The folder picker could not be shown.
    pub fn picker_failed(&mut self, message: impl Into<String>) {
        self.problem = Some(Problem::FolderPicker(message.into()));
    }

    // ---- Driving --------------------------------------------------------

    /// The next request to run, when none is running.
    pub fn next_request(&mut self) -> Option<Request> {
        if self.in_flight {
            return None;
        }
        let request = self.queue.pop_front()?;
        self.in_flight = true;
        self.opening = matches!(request, Request::Open { .. });
        Some(request)
    }

    /// Takes in the answer to the request [`Self::next_request`] handed out.
    pub fn apply(&mut self, outcome: Outcome) {
        self.in_flight = false;
        self.opening = false;
        if matches!(
            outcome.kind,
            OutcomeKind::Committed | OutcomeKind::CommitFailed
        ) {
            self.committing = false;
        }

        match outcome.kind {
            OutcomeKind::OpenFailed => {
                self.problem = outcome.error.map(Problem::Git);
                return;
            }
            OutcomeKind::Opened => {
                let reopened = self.repository.as_deref() == Some(outcome.repository.as_path());
                if !reopened {
                    self.selection = None;
                    self.diff = None;
                    self.clear_form();
                    // A commit queued for the old repository is dropped just
                    // below, so its answer will never come to clear this.
                    self.committing = false;
                    // Anything queued was for the repository that was open.
                    self.queue
                        .retain(|request| matches!(request, Request::Open { .. }));
                }
                self.repository = Some(outcome.repository.clone());
                self.problem = None;
                self.remember(outcome.repository.clone());
            }
            _ => {
                // An answer about a repository that is no longer open.
                if self.repository.as_deref() != Some(outcome.repository.as_path()) {
                    return;
                }
            }
        }

        if outcome.kind == OutcomeKind::Committed {
            self.clear_form();
        }
        if let Some(error) = outcome.error {
            self.problem = Some(Problem::Git(error));
        }

        if let Some(status) = outcome.status {
            self.selection = self
                .selection
                .as_ref()
                .and_then(|selection| resolve_selection(&status, selection));
            if self.selection.is_none() {
                self.diff = None;
            }
            self.status = Some(status);
        }

        if let Some((selection, diff)) = outcome.diff
            && self.selection.as_ref() == Some(&selection)
        {
            match diff {
                Ok(diff) => self.diff = Some(ShownDiff { selection, diff }),
                Err(error) => self.problem = Some(Problem::Git(error)),
            }
        }

        self.ensure_diff();
    }

    /// Asks for the selected file's diff when what is shown is for something
    /// else and nothing already asks for it.
    fn ensure_diff(&mut self) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        let conflicted = self
            .status
            .as_ref()
            .is_some_and(|status| is_conflicted(status, &selection.path));
        let current = self
            .diff
            .as_ref()
            .is_some_and(|shown| shown.selection == selection);
        let queued = self.queue.iter().any(|request| {
            matches!(
                request,
                Request::Diff { .. }
                    | Request::Refresh { .. }
                    | Request::Stage { .. }
                    | Request::Unstage { .. }
            )
        });
        if conflicted || current || queued {
            return;
        }
        if let Some(repository) = self.repository.clone() {
            self.queue.push_back(Request::Diff {
                repository,
                selection,
            });
        }
    }

    /// Runs every queued request on this thread, in order. Tests drive the
    /// session with this; the window runs requests in the background instead.
    pub fn run_pending(&mut self) {
        while let Some(request) = self.next_request() {
            let outcome = request.run(self.git.as_ref());
            self.apply(outcome);
        }
    }

    fn clear_form(&mut self) {
        self.summary.clear();
        self.description.clear();
        self.form_generation += 1;
    }

    fn remember(&mut self, repository: PathBuf) {
        self.recent = recent::remember(&self.recent, repository);
        if let Some(file) = &self.recent_file
            && let Err(error) = recent::save(file, &self.recent)
        {
            // Losing the list is an inconvenience, not a failure of what the
            // person asked for, so it is reported where a person running the
            // window from a terminal would look and not on screen.
            eprintln!(
                "better-git: the recent repositories could not be saved to {}: {error}",
                file.display()
            );
        }
    }
}
