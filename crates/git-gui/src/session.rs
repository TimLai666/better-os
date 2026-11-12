//! Everything the Better Git window decides, with no GPUI in it.
//!
//! The session holds the open repository and everything shown about it,
//! the dialogs, the settings, and the GitHub account. Every git command is
//! an [`Op`] in a queue, run one at a time by whoever drives the session
//! ([`GitSession::next_job`], [`Job::run`], [`GitSession::apply`]); every
//! GitHub request is a [`GhRequest`] in a second queue, so a slow network
//! never holds up a commit. The window runs both on background threads;
//! tests run them in a loop with [`GitSession::run_pending`].
//!
//! One git job is in flight at a time: two `git` processes writing one
//! index at once fail on git's own lock. An answer for a repository that was
//! closed or replaced meanwhile is dropped by its generation.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use git_core::{
    BranchHead, Commit, CommitFile, FileDiff, HostedRepository, Operation, RepositoryStatus,
    image_kind, parse_hosted, sanitize_ref_name,
};
use git_platform::trust::TrustStore;
use git_platform::{Cancel, Lease};
use git_platform::{
    Credentials, DiffOptions, DiffSide, FileSelection, Git, GitError, HistoryEdit, NetworkOptions,
    NewRepository, RepositoryInfo, SwitchChanges,
};
use github_api::{ApiError, CombinedChecks, Host, Poll, PullRequest, RepoInfo};

use crate::changes::{self, Include, Inclusion};
use crate::commit::CommitForm;
use crate::dialogs::{BranchPurpose, CloneTab, Dialog};
use crate::github::{Account, GhAnswer, GhCall, GhRequest, GhResult, SignIn, SignInProblem};
use crate::notifications;
use crate::repositories::RepositoryList;
use crate::settings::Settings;

/// Which list the left pane shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Tab {
    #[default]
    Changes,
    History,
}

/// Which page fills the window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Page {
    #[default]
    Main,
    Settings,
}

/// What a diff is of.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiffKey {
    Working(PathBuf),
    Commit { sha: String, path: PathBuf },
    Stash { name: String, path: PathBuf },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiffContent {
    Text(FileDiff),
    /// Too large to draw until the person asks.
    Large(FileDiff),
    /// The image before and after; `None` where that side has no file.
    Image {
        old: Option<Vec<u8>>,
        new: Option<Vec<u8>>,
    },
    Conflicted,
    Failed(GitError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShownDiff {
    pub key: DiffKey,
    pub content: DiffContent,
}

/// What kind of job a git answer was for, to pick messages and follow-ups.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpKind {
    Refresh,
    Commit,
    Discard,
    Ignore,
    UndoCommit,
    Reset,
    Revert,
    CherryPick,
    Reorder,
    Squash,
    CreateBranch,
    Switch,
    RenameBranch,
    DeleteBranch,
    Merge,
    Rebase,
    StashAll,
    RestoreStash,
    DropStash,
    CreateTag,
    DeleteTag,
    AddWorktree,
    RenameWorktree,
    RemoveWorktree,
    Fetch,
    BackgroundFetch,
    Pull,
    Push,
    ForcePush,
    AddRemote,
    RemoveRemote,
    SetRemoteUrl,
    Resolve,
    Continue,
    Abort,
    Clone,
    Init,
    GitConfig,
}

impl OpKind {
    /// Whether the job reaches a remote.
    pub fn is_network(self) -> bool {
        matches!(
            self,
            OpKind::Fetch
                | OpKind::BackgroundFetch
                | OpKind::Pull
                | OpKind::Push
                | OpKind::ForcePush
                | OpKind::Clone
        )
    }
}

/// One git command, with everything it needs captured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Op {
    Open,
    Refresh,
    Diff {
        key: DiffKey,
        original: Option<PathBuf>,
        options: DiffOptions,
        image: bool,
        conflicted: bool,
        file: Option<CommitFile>,
    },
    Commit {
        files: Vec<FileSelection>,
        message: git_platform::CommitMessage,
        options: git_platform::CommitOptions,
    },
    Discard(Vec<FileSelection>),
    DiscardLines(FileSelection),
    Ignore(String),
    Log {
        skip: usize,
    },
    CommitFiles(String),
    /// Undo the last commit, reading its message from git first.
    UndoCommit,
    Edit {
        kind: OpKind,
        edit: HistoryEdit,
    },
    CreateBranch {
        name: String,
        start: Option<String>,
        checkout: bool,
    },
    Switch {
        target: String,
        changes: SwitchChanges,
    },
    RenameBranch {
        from: String,
        to: String,
    },
    DeleteBranch {
        name: String,
        /// The remote and the branch's name there (its upstream's), to
        /// delete too.
        remote: Option<(String, String)>,
        network: NetworkOptions,
    },
    Merge {
        branch: String,
        squash: bool,
    },
    Rebase {
        onto: String,
    },
    Compare {
        branch: String,
        base: String,
    },
    StashAll {
        message: String,
    },
    RestoreStash(String),
    DropStash(String),
    StashFiles(String),
    CreateTag {
        name: String,
        sha: String,
    },
    DeleteTag(String),
    AddWorktree {
        path: PathBuf,
        branch: String,
        create: bool,
    },
    RenameWorktree {
        from: PathBuf,
        to: PathBuf,
    },
    RemoveWorktree {
        path: PathBuf,
        force: bool,
    },
    Fetch {
        background: bool,
        network: NetworkOptions,
    },
    Pull(NetworkOptions),
    Push {
        request: git_platform::PushRequest,
        network: NetworkOptions,
    },
    AddRemote {
        name: String,
        url: String,
    },
    RemoveRemote(String),
    SetRemoteUrl {
        name: String,
        url: String,
    },
    ResolveWith {
        path: PathBuf,
        ours: bool,
    },
    MarkResolved(PathBuf),
    Continue(Operation),
    Abort(Operation),
    FinishStashConflict(String),
    /// Conflicts no operation owns are resolved: unstage, drop nothing.
    FinishLooseConflict,
    AbortStashConflict,
    Clone {
        url: String,
        destination: PathBuf,
        network: NetworkOptions,
    },
    Init(NewRepository),
    ReadGitConfig,
    SetGitConfig(Vec<(String, String)>),
}

impl Op {
    pub fn kind(&self) -> OpKind {
        match self {
            Op::Commit { .. } => OpKind::Commit,
            Op::Discard(_) | Op::DiscardLines(_) => OpKind::Discard,
            Op::Ignore(_) => OpKind::Ignore,
            Op::Edit { kind, .. } => *kind,
            Op::UndoCommit => OpKind::UndoCommit,
            Op::CreateBranch { .. } => OpKind::CreateBranch,
            Op::Switch { .. } => OpKind::Switch,
            Op::RenameBranch { .. } => OpKind::RenameBranch,
            Op::DeleteBranch { .. } => OpKind::DeleteBranch,
            Op::Merge { .. } => OpKind::Merge,
            Op::Rebase { .. } => OpKind::Rebase,
            Op::StashAll { .. } => OpKind::StashAll,
            Op::RestoreStash(_) => OpKind::RestoreStash,
            Op::DropStash(_) => OpKind::DropStash,
            Op::CreateTag { .. } => OpKind::CreateTag,
            Op::DeleteTag(_) => OpKind::DeleteTag,
            Op::AddWorktree { .. } => OpKind::AddWorktree,
            Op::RenameWorktree { .. } => OpKind::RenameWorktree,
            Op::RemoveWorktree { .. } => OpKind::RemoveWorktree,
            Op::Fetch {
                background: true, ..
            } => OpKind::BackgroundFetch,
            Op::Fetch { .. } => OpKind::Fetch,
            Op::Pull(_) => OpKind::Pull,
            Op::Push { request, .. } if request.force.is_some() => OpKind::ForcePush,
            Op::Push { .. } => OpKind::Push,
            Op::AddRemote { .. } => OpKind::AddRemote,
            Op::RemoveRemote(_) => OpKind::RemoveRemote,
            Op::SetRemoteUrl { .. } => OpKind::SetRemoteUrl,
            Op::ResolveWith { .. } | Op::MarkResolved(_) => OpKind::Resolve,
            Op::Continue(_) | Op::FinishStashConflict(_) | Op::FinishLooseConflict => {
                OpKind::Continue
            }
            Op::Abort(_) | Op::AbortStashConflict => OpKind::Abort,
            Op::Clone { .. } => OpKind::Clone,
            Op::Init(_) => OpKind::Init,
            Op::ReadGitConfig | Op::SetGitConfig(_) => OpKind::GitConfig,
            _ => OpKind::Refresh,
        }
    }
}

/// A job: an op for one repository (or a destination, for clone and init),
/// with the generation of the repository it was asked for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    pub generation: u64,
    pub repository: PathBuf,
    pub op: Op,
}

/// The status and the rest, read again after anything changed.
pub type Snapshot = Result<(RepositoryStatus, RepositoryInfo), GitError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Answer {
    Opened {
        root: PathBuf,
        snapshot: Snapshot,
    },
    OpenFailed(GitError),
    Changed {
        kind: OpKind,
        result: Result<(), GitError>,
        snapshot: Snapshot,
    },
    Diff {
        key: DiffKey,
        content: DiffContent,
    },
    Log {
        skip: usize,
        result: Result<Vec<Commit>, GitError>,
    },
    CommitFiles {
        sha: String,
        result: Result<Vec<CommitFile>, GitError>,
    },
    Compare {
        branch: String,
        result: Result<(Vec<Commit>, usize), GitError>,
    },
    StashFiles {
        name: String,
        result: Result<Vec<CommitFile>, GitError>,
    },
    /// A clone or a new repository: open `path` next.
    Created {
        kind: OpKind,
        path: PathBuf,
        result: Result<(), GitError>,
    },
    GitConfig(Result<Vec<(String, Option<String>)>, GitError>),
    /// The last commit was undone (or not); `last` is the commit it was,
    /// read from git just before.
    Undone {
        last: Option<Commit>,
        result: Result<(), GitError>,
        snapshot: Snapshot,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Outcome {
    pub generation: u64,
    pub answer: Answer,
}

/// Largest conflicted file read to count its markers.
const MAX_CONFLICT_BYTES: u64 = 16 * 1024 * 1024;

/// The status and the rest, with each conflicted file's markers counted
/// here, on the job's thread, so drawing never reads a file.
fn snapshot(git: &dyn Git, root: &Path) -> Snapshot {
    let status = git.status(root)?;
    let mut info = git.info(root)?;
    info.conflict_markers = status
        .changes
        .iter()
        .filter(|c| c.conflict.is_some_and(|k| k.has_markers()))
        .map(|c| {
            let count = git_platform::patch::read_capped(&root.join(&c.path), MAX_CONFLICT_BYTES)
                .ok()
                .flatten()
                .map(|bytes| git_core::count_conflict_markers(&bytes));
            (c.path.clone(), count)
        })
        .collect();
    Ok((status, info))
}

const CONFIG_KEYS: [&str; 3] = ["user.name", "user.email", "init.defaultBranch"];

fn read_config(git: &dyn Git) -> Result<Vec<(String, Option<String>)>, GitError> {
    CONFIG_KEYS
        .iter()
        .map(|k| git.global_config(k).map(|v| (k.to_string(), v)))
        .collect()
}

impl Job {
    /// Runs the job. This is the only place the session's requests reach
    /// git, and it touches nothing but its own arguments, so it is safe on
    /// any thread.
    pub fn run(self, git: &dyn Git) -> Outcome {
        let root = self.repository.as_path();
        let changed = |kind: OpKind, result: Result<(), GitError>| Answer::Changed {
            kind,
            result,
            snapshot: snapshot(git, root),
        };
        let kind = self.op.kind();
        let answer = match self.op {
            Op::Open => match git.repository_root(root) {
                Ok(found) => Answer::Opened {
                    snapshot: snapshot(git, &found),
                    root: found,
                },
                Err(error) => Answer::OpenFailed(error),
            },
            Op::Refresh => changed(OpKind::Refresh, Ok(())),
            Op::Diff {
                key,
                original,
                options,
                image,
                conflicted,
                file,
            } => {
                let content = if conflicted {
                    DiffContent::Conflicted
                } else {
                    diff_content(git, root, &key, original.as_deref(), options, image, file)
                };
                Answer::Diff { key, content }
            }
            Op::Commit {
                files,
                message,
                options,
            } => changed(kind, git.commit_selection(root, &files, &message, &options)),
            Op::Discard(files) => changed(kind, git.discard_files(root, &files)),
            Op::DiscardLines(file) => changed(kind, git.discard_lines(root, &file)),
            Op::Ignore(pattern) => changed(kind, git.ignore(root, &pattern)),
            Op::Log { skip } => Answer::Log {
                skip,
                result: git.log(root, "HEAD", skip, LOG_PAGE),
            },
            Op::CommitFiles(sha) => Answer::CommitFiles {
                result: git.commit_files(root, &sha),
                sha,
            },
            Op::Edit { edit, .. } => changed(kind, git.edit_history(root, &edit)),
            Op::UndoCommit => {
                let last = git.log(root, "HEAD", 0, 1).map(|mut c| c.pop());
                match last {
                    Ok(last) => {
                        let result = git.edit_history(root, &HistoryEdit::UndoLast);
                        Answer::Undone {
                            last,
                            result,
                            snapshot: snapshot(git, root),
                        }
                    }
                    Err(error) => changed(kind, Err(error)),
                }
            }
            Op::CreateBranch {
                name,
                start,
                checkout,
            } => changed(
                kind,
                git.create_branch(root, &name, start.as_deref(), checkout),
            ),
            Op::Switch { target, changes } => changed(kind, git.switch(root, &target, changes)),
            Op::RenameBranch { from, to } => changed(kind, git.rename_branch(root, &from, &to)),
            Op::DeleteBranch {
                name,
                remote,
                network,
            } => {
                let mut result = git.delete_branch(root, &name, true);
                if result.is_ok()
                    && let Some((remote, remote_branch)) = remote
                {
                    result = git.delete_remote_branch(root, &remote, &remote_branch, &network);
                }
                changed(kind, result)
            }
            Op::Merge { branch, squash } => changed(kind, git.merge(root, &branch, squash)),
            Op::Rebase { onto } => changed(kind, git.rebase(root, &onto)),
            Op::Compare { branch, base } => Answer::Compare {
                result: git.compare(root, &base, &branch),
                branch,
            },
            Op::StashAll { message } => changed(kind, git.stash_all(root, &message)),
            Op::RestoreStash(name) => changed(kind, git.stash_pop(root, &name)),
            Op::DropStash(name) => changed(kind, git.stash_drop(root, &name)),
            Op::StashFiles(name) => Answer::StashFiles {
                result: git.stash_files(root, &name),
                name,
            },
            Op::CreateTag { name, sha } => changed(kind, git.create_tag(root, &name, &sha)),
            Op::DeleteTag(name) => changed(kind, git.delete_tag(root, &name)),
            Op::AddWorktree {
                path,
                branch,
                create,
            } => changed(kind, git.add_worktree(root, &path, &branch, create)),
            Op::RenameWorktree { from, to } => changed(kind, git.move_worktree(root, &from, &to)),
            Op::RemoveWorktree { path, force } => {
                changed(kind, git.remove_worktree(root, &path, force))
            }
            Op::Fetch { network, .. } => changed(kind, git.fetch(root, None, &network)),
            Op::Pull(network) => changed(kind, git.pull(root, &network)),
            Op::Push { request, network } => changed(kind, git.push(root, &request, &network)),
            Op::AddRemote { name, url } => changed(kind, git.add_remote(root, &name, &url)),
            Op::RemoveRemote(name) => changed(kind, git.remove_remote(root, &name)),
            Op::SetRemoteUrl { name, url } => changed(kind, git.set_remote_url(root, &name, &url)),
            Op::ResolveWith { path, ours } => changed(kind, git.resolve_with(root, &path, ours)),
            Op::MarkResolved(path) => changed(kind, git.mark_resolved(root, &path)),
            Op::Continue(operation) => changed(kind, git.continue_operation(root, operation)),
            Op::Abort(operation) => changed(kind, git.abort_operation(root, operation)),
            Op::FinishStashConflict(name) => changed(kind, git.finish_stash_conflict(root, &name)),
            Op::AbortStashConflict => changed(kind, git.abort_stash_conflict(root)),
            Op::FinishLooseConflict => changed(kind, git.finish_loose_conflict(root)),
            Op::Clone {
                url,
                destination,
                network,
            } => Answer::Created {
                kind,
                result: git.clone_repository(&url, &destination, &network),
                path: destination,
            },
            Op::Init(new) => Answer::Created {
                kind,
                result: git.init_repository(&new),
                path: new.path,
            },
            Op::ReadGitConfig => Answer::GitConfig(read_config(git)),
            Op::SetGitConfig(values) => Answer::GitConfig(
                values
                    .iter()
                    .try_for_each(|(k, v)| git.set_global_config(k, v))
                    .and_then(|()| read_config(git)),
            ),
        };
        Outcome {
            generation: self.generation,
            answer,
        }
    }
}

fn diff_content(
    git: &dyn Git,
    root: &Path,
    key: &DiffKey,
    original: Option<&Path>,
    options: DiffOptions,
    image: bool,
    file: Option<CommitFile>,
) -> DiffContent {
    let result = match key {
        DiffKey::Working(path) => {
            if image {
                return match git.file_versions(root, path, original) {
                    Ok((old, new)) => DiffContent::Image { old, new },
                    Err(error) => DiffContent::Failed(error),
                };
            }
            git.diff_with(root, path, DiffSide::WorkingTree, options)
        }
        DiffKey::Commit { sha, .. } => match &file {
            Some(file) => git.commit_diff(root, sha, file, options),
            None => Err(GitError::ChangedMeanwhile),
        },
        DiffKey::Stash { name, .. } => match &file {
            Some(file) => git.stash_diff(root, name, file),
            None => Err(GitError::ChangedMeanwhile),
        },
    };
    match result {
        Ok(diff) if git_core::is_large(&diff) => DiffContent::Large(diff),
        Ok(diff) => DiffContent::Text(diff),
        Err(error) => DiffContent::Failed(error),
    }
}

/// How many commits one page of history reads.
pub const LOG_PAGE: usize = 200;

/// What the window has to tell the person.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Problem {
    Git {
        kind: OpKind,
        error: GitError,
    },
    GitHub(ApiError),
    SignIn(SignInProblem),
    /// The folder picker or another platform service failed.
    Platform(String),
    /// A program could not be started.
    Launch(String),
    /// The keyring refused.
    Keyring(String),
    /// A file of Better Git's own could not be written.
    Io(String),
    /// Signing in is unavailable in this build.
    NoClientId,
}

/// Good news, shown briefly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    Done(OpKind),
    PullCreated {
        number: u64,
        url: String,
    },
    IssueCreated {
        number: u64,
        url: String,
    },
    Published {
        url: String,
    },
    Forked {
        name: String,
    },
    ChecksRerun(usize),
    SignedIn(String),
    Pull(notifications::Notice),
    /// A stash is waiting for the branch just switched to.
    StashWaiting(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HistoryView {
    pub commits: Vec<Commit>,
    pub loaded: bool,
    pub end: bool,
    pub selected: Option<usize>,
    pub files: Vec<CommitFile>,
    pub file: Option<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StashView {
    pub name: String,
    pub files: Vec<CommitFile>,
    pub file: Option<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompareView {
    pub branch: String,
    pub ahead: Vec<Commit>,
    pub behind: usize,
}

/// What GitHub said about this repository.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepoGitHub {
    pub pulls: Vec<PullRequest>,
    pub checks: Option<CombinedChecks>,
    pub protected: bool,
    pub can_push: bool,
    pub parent: Option<RepoInfo>,
    pub loaded: bool,
}

/// The open repository.
#[derive(Clone, Debug)]
pub struct Repo {
    pub root: PathBuf,
    pub status: RepositoryStatus,
    pub info: RepositoryInfo,
    pub hosted: Option<HostedRepository>,
    pub tab: Tab,
    pub inclusion: Inclusion,
    pub selected_file: Option<PathBuf>,
    pub filter: String,
    pub diff: Option<ShownDiff>,
    pub show_large: bool,
    pub whole_file: bool,
    pub form: CommitForm,
    pub history: HistoryView,
    pub stash: Option<StashView>,
    pub compare: Option<CompareView>,
    /// The job running or waiting, for the progress shown.
    pub busy: Option<OpKind>,
    /// Tags made here and not pushed yet; the next push sends them.
    pub unpushed_tags: Vec<String>,
    /// A stash restore that stopped with conflicts: its name.
    pub stash_conflict: Option<String>,
    pub recent_branches: Vec<String>,
    pub github: RepoGitHub,
    /// When the last fetch ran, seconds since the epoch.
    pub last_fetch: Option<i64>,
    /// Push after the next successful commit.
    push_after_commit: bool,
}

impl Repo {
    fn new(root: PathBuf, status: RepositoryStatus, info: RepositoryInfo) -> Repo {
        let hosted = hosted_of(&info);
        let mut repo = Repo {
            root,
            status,
            info,
            hosted,
            tab: Tab::Changes,
            inclusion: Inclusion::default(),
            selected_file: None,
            filter: String::new(),
            diff: None,
            show_large: false,
            whole_file: false,
            form: CommitForm::default(),
            history: HistoryView::default(),
            stash: None,
            compare: None,
            busy: None,
            unpushed_tags: Vec::new(),
            stash_conflict: None,
            recent_branches: Vec::new(),
            github: RepoGitHub::default(),
            last_fetch: None,
            push_after_commit: false,
        };
        repo.sync_inclusion();
        repo
    }

    pub fn branch(&self) -> Option<&str> {
        match &self.status.head {
            BranchHead::Branch(name) => Some(name),
            BranchHead::Detached => None,
        }
    }

    pub fn name(&self) -> String {
        crate::repositories::folder_name(&self.root)
    }

    /// The default branch's name, from the remote's HEAD or the usual names.
    pub fn default_branch(&self) -> Option<String> {
        git_core::branch::default_branch(&self.info.branches, self.info.remote_head.as_deref())
            .map(|b| b.name_on_remote().to_string())
    }

    /// `origin/feature/x` as (`origin`, `feature/x`), by the longest remote
    /// name it starts with (a remote's name may hold a `/`).
    pub fn split_remote_branch(&self, short: &str) -> Option<(String, String)> {
        self.info
            .remotes
            .iter()
            .filter_map(|r| {
                let rest = short.strip_prefix(&r.name)?.strip_prefix('/')?;
                (!rest.is_empty()).then(|| (r.name.clone(), rest.to_string()))
            })
            .max_by_key(|(remote, _)| remote.len())
    }

    /// Where a push of the current branch goes: its upstream (remote and
    /// name there) when it has one, else the same name on the push remote,
    /// setting the upstream.
    pub fn push_target(&self) -> Option<(String, String, bool)> {
        let branch = self.branch()?;
        if let Some((remote, name)) = self
            .status
            .upstream
            .as_deref()
            .and_then(|u| self.split_remote_branch(u))
        {
            return Some((remote, name, false));
        }
        Some((self.push_remote()?, branch.to_string(), true))
    }

    /// The commit the remote branch a push goes to was on when last seen
    /// (its remote-tracking branch), for a force push's lease; `None` when
    /// it is not there.
    pub fn seen_on_remote(&self) -> Option<String> {
        let (remote, name, _) = self.push_target()?;
        let full = format!("{remote}/{name}");
        self.info
            .branches
            .iter()
            .find(|b| b.remote && b.name == full)
            .map(|b| b.sha.clone())
    }

    /// The remote a push goes to: the upstream's, else `origin`, else the
    /// first.
    pub fn push_remote(&self) -> Option<String> {
        if let Some(upstream) = &self.status.upstream
            && let Some((remote, _)) = upstream.split_once('/')
            && self.info.remotes.iter().any(|r| r.name == remote)
        {
            return Some(remote.to_string());
        }
        self.info
            .remotes
            .iter()
            .find(|r| r.name == "origin")
            .or_else(|| self.info.remotes.first())
            .map(|r| r.name.clone())
    }

    /// A merge or a squash merge is in progress.
    pub fn merging(&self) -> bool {
        matches!(
            self.info.operation,
            Some(Operation::Merge | Operation::SquashMerge)
        )
    }

    /// Keeps the inclusion in step with the status, every file whole
    /// while merging.
    fn sync_inclusion(&mut self) {
        self.inclusion.sync(&self.status);
        if self.merging() {
            self.inclusion.include_everything(&self.status);
        }
    }

    pub fn has_changes(&self) -> bool {
        !self.status.changes.is_empty()
    }

    /// An operation stopped with conflicts, or a stash restore did, or
    /// files are conflicted with no operation of git's to own them.
    pub fn in_conflict(&self) -> bool {
        self.info.operation.is_some()
            || self.stash_conflict.is_some()
            || self.status.changes.iter().any(|c| c.conflicted)
    }

    pub fn rows(&self) -> Vec<changes::Row> {
        changes::rows(
            &self.status,
            &self.inclusion,
            self.selected_file.as_deref(),
            &self.filter,
        )
    }
}

fn hosted_of(info: &RepositoryInfo) -> Option<HostedRepository> {
    let remote = info
        .remotes
        .iter()
        .find(|r| r.name == "origin")
        .or_else(|| info.remotes.first())?;
    parse_hosted(&remote.fetch_url)
}

/// The GitHub side of the session.
#[derive(Clone, Debug, Default)]
pub struct GitHub {
    pub account: Option<Account>,
    pub sign_in: SignIn,
    /// The host sign-in is for.
    pub host: Option<Host>,
    pub repositories: Option<Vec<RepoInfo>>,
    pub organizations: Vec<String>,
    pub gitignores: Vec<String>,
    pub licenses: Vec<(String, String)>,
    /// A new repository waiting for its templates' text.
    pending_init: Option<NewRepository>,
    /// A publish waiting for GitHub to make the repository.
    pending_publish: Option<(PathBuf, u64)>,
}

/// The global git settings, as read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitConfig {
    pub values: Vec<(String, Option<String>)>,
    pub loaded: bool,
}

impl GitConfig {
    pub fn get(&self, key: &str) -> &str {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.as_deref())
            .unwrap_or("")
    }
}

/// What the window must do with the keyring after a session change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyringChange {
    Store(Account),
    Delete(String),
}

/// Where the session keeps its files. `None` remembers only for this run.
#[derive(Clone, Debug, Default)]
pub struct Files {
    pub settings: Option<PathBuf>,
    pub repositories: Option<PathBuf>,
    pub recent: Option<PathBuf>,
    pub trust: Option<PathBuf>,
}

impl Files {
    pub fn defaults() -> Files {
        Files {
            settings: Settings::default_file(),
            repositories: RepositoryList::default_file(),
            recent: crate::recent::default_path(),
            trust: TrustStore::default_file(),
        }
    }
}

pub struct GitSession {
    git: Arc<dyn Git>,
    pub settings: Settings,
    settings_file: Option<PathBuf>,
    pub repositories: RepositoryList,
    recent_file: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    pub trust: TrustStore,
    pub repo: Option<Repo>,
    pub page: Page,
    pub problem: Option<Problem>,
    pub notice: Option<Notice>,
    pub dialog: Option<Dialog>,
    pub github: GitHub,
    pub git_config: GitConfig,
    /// For the window: keyring writes and desktop notifications to send.
    pub keyring_changes: Vec<KeyringChange>,
    pub desktop_notices: Vec<notifications::Notice>,
    pull_notices: notifications::Seen,
    queue: VecDeque<Job>,
    running: bool,
    gh_queue: VecDeque<GhRequest>,
    gh_running: bool,
    /// The background fetch runs beside the queue, so a fetch stuck on a
    /// slow server never holds up a commit or a switch.
    background: Option<Job>,
    background_running: bool,
    background_cancel: Cancel,
    /// Stops the network jobs the person started (and those waiting);
    /// replaced after each cancel.
    cancel: Cancel,
    generation: u64,
    /// The client ID GitHub sign-in uses; empty disables sign-in.
    client_id: &'static str,
}

impl GitSession {
    pub fn new(git: Arc<dyn Git>, files: Files) -> Self {
        let recent = files
            .recent
            .as_deref()
            .map(crate::recent::load)
            .unwrap_or_default();
        GitSession {
            git,
            settings: Settings::load(files.settings.as_deref()),
            settings_file: files.settings,
            repositories: RepositoryList::load(files.repositories),
            recent_file: files.recent,
            recent,
            trust: TrustStore::load(files.trust),
            repo: None,
            page: Page::Main,
            problem: None,
            notice: None,
            dialog: None,
            github: GitHub::default(),
            git_config: GitConfig::default(),
            keyring_changes: Vec::new(),
            desktop_notices: Vec::new(),
            pull_notices: notifications::Seen::default(),
            queue: VecDeque::new(),
            running: false,
            gh_queue: VecDeque::new(),
            gh_running: false,
            background: None,
            background_running: false,
            background_cancel: Cancel::new(),
            cancel: Cancel::new(),
            generation: 0,
            client_id: github_api::CLIENT_ID,
        }
    }

    pub fn git(&self) -> Arc<dyn Git> {
        self.git.clone()
    }

    /// For tests of a build without a client ID.
    pub fn set_client_id(&mut self, id: &'static str) {
        self.client_id = id;
    }

    pub fn sign_in_available(&self) -> bool {
        !self.client_id.trim().is_empty()
    }

    pub fn is_busy(&self) -> bool {
        self.running || !self.queue.is_empty()
    }

    pub fn is_network_busy(&self) -> bool {
        self.repo
            .as_ref()
            .and_then(|r| r.busy)
            .is_some_and(OpKind::is_network)
    }

    /// The jobs waiting, for tests.
    pub fn queued(&self) -> Vec<&Op> {
        self.queue.iter().map(|j| &j.op).collect()
    }

    pub fn queued_gh(&self) -> Vec<&GhCall> {
        self.gh_queue.iter().map(|r| &r.call).collect()
    }

    fn push_op(&mut self, op: Op) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let kind = op.kind();
        if kind.is_network() {
            // The person's own fetch, pull, or push replaces a background
            // fetch rather than racing it.
            self.background_cancel.cancel();
        }
        if kind != OpKind::BackgroundFetch && !matches!(op, Op::Refresh | Op::Diff { .. }) {
            repo.busy = Some(kind);
        }
        self.queue.push_back(Job {
            generation: self.generation,
            repository: repo.root.clone(),
            op,
        });
    }

    fn push_gh(&mut self, call: GhCall) {
        let host = self
            .github
            .account
            .as_ref()
            .map(|a| a.host.clone())
            .or_else(|| self.github.host.clone())
            .unwrap_or_else(Host::github);
        let token = self.github.account.as_ref().map(|a| a.token.clone());
        self.gh_queue.push_back(GhRequest { host, token, call });
    }

    /// The background fetch, to run beside the queue, when one is waiting
    /// and none is running.
    pub fn next_background(&mut self) -> Option<Job> {
        if self.background_running {
            return None;
        }
        let job = self.background.take()?;
        self.background_running = true;
        Some(job)
    }

    /// The background fetch ended. Its snapshot is not used (a job in the
    /// queue may have changed the repository meanwhile); a refresh is
    /// queued instead, and GitHub is asked again, as after any fetch.
    pub fn apply_background(&mut self, outcome: Outcome) {
        self.background_running = false;
        if outcome.generation != self.generation {
            return;
        }
        // A background fetch fails quietly; the next one retries.
        if let Answer::Changed { result: Ok(()), .. } = outcome.answer {
            self.refresh();
            self.refresh_github();
        }
    }

    /// Whether a background fetch is running.
    pub fn is_background_fetching(&self) -> bool {
        self.background_running
    }

    /// Stops the network job running and those waiting: git and what it
    /// started are killed, and each answers that it was cancelled.
    pub fn cancel_network(&mut self) {
        self.cancel.cancel();
        self.cancel = Cancel::new();
        self.background_cancel.cancel();
    }

    /// The next git job, when none is running.
    pub fn next_job(&mut self) -> Option<Job> {
        if self.running {
            return None;
        }
        let job = self.queue.pop_front()?;
        self.running = true;
        Some(job)
    }

    pub fn next_gh(&mut self) -> Option<GhRequest> {
        if self.gh_running {
            return None;
        }
        let request = self.gh_queue.pop_front()?;
        self.gh_running = true;
        Some(request)
    }

    /// Runs every waiting job and request in this thread, for tests.
    pub fn run_pending(&mut self) {
        loop {
            if let Some(job) = self.next_background() {
                let outcome = job.run(self.git.as_ref());
                self.apply_background(outcome);
            } else if let Some(job) = self.next_job() {
                let outcome = job.run(self.git.as_ref());
                self.apply(outcome);
            } else if let Some(request) = self.next_gh() {
                let answer = request.run(crate::github::now());
                self.apply_gh(answer);
            } else {
                break;
            }
        }
    }

    /// Runs only the git jobs, for tests that leave GitHub alone.
    pub fn run_git_jobs(&mut self) {
        loop {
            if let Some(job) = self.next_background() {
                let outcome = job.run(self.git.as_ref());
                self.apply_background(outcome);
            } else if let Some(job) = self.next_job() {
                let outcome = job.run(self.git.as_ref());
                self.apply(outcome);
            } else {
                break;
            }
        }
    }

    // --- opening ----------------------------------------------------------

    /// Opens a path. A path not trusted yet asks first.
    pub fn open(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        if crate::trust::must_ask(&self.trust, &path) {
            self.dialog = Some(Dialog::Trust {
                programs: crate::trust::configured_programs(&path),
                path,
            });
            return;
        }
        self.open_trusted(path);
    }

    /// The person trusted the path: remember it and open it.
    pub fn trust_and_open(&mut self, path: PathBuf) {
        if let Err(error) = self.trust.trust(&path) {
            self.problem = Some(Problem::Io(error.to_string()));
        }
        self.dialog = None;
        self.open_trusted(path);
    }

    fn open_trusted(&mut self, path: PathBuf) {
        self.generation += 1;
        self.queue.clear();
        self.background = None;
        self.background_cancel.cancel();
        self.queue.push_back(Job {
            generation: self.generation,
            repository: path,
            op: Op::Open,
        });
    }

    pub fn close_repository(&mut self) {
        self.generation += 1;
        self.queue.clear();
        self.background = None;
        self.background_cancel.cancel();
        self.repo = None;
        self.page = Page::Main;
    }

    pub fn refresh(&mut self) {
        if self.repo.is_some() && !self.queue.iter().any(|j| j.op == Op::Refresh) {
            self.push_op(Op::Refresh);
        }
    }

    // --- the changes list -------------------------------------------------

    pub fn set_tab(&mut self, tab: Tab) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        repo.tab = tab;
        repo.diff = None;
        repo.stash = None;
        repo.compare = None;
        if tab == Tab::History {
            if !repo.history.loaded {
                self.push_op(Op::Log { skip: 0 });
            } else if let Some(index) = repo.history.selected {
                self.select_commit(index);
            }
        } else if let Some(path) = repo.selected_file.clone() {
            self.select_file(&path);
        }
    }

    fn diff_options(&self) -> DiffOptions {
        DiffOptions {
            ignore_whitespace: self.settings.hide_whitespace,
            whole_file: self.repo.as_ref().is_some_and(|r| r.whole_file),
        }
    }

    pub fn select_file(&mut self, path: &Path) {
        let options = self.diff_options();
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let Some(change) = repo.status.changes.iter().find(|c| c.path == path).cloned() else {
            return;
        };
        repo.selected_file = Some(path.to_path_buf());
        repo.show_large = false;
        repo.stash = None;
        self.push_op(Op::Diff {
            key: DiffKey::Working(path.to_path_buf()),
            original: change.original_path.clone(),
            options,
            image: image_kind(path).is_some(),
            conflicted: change.conflicted,
            file: None,
        });
    }

    /// Whether the choice of files and lines is locked: during a merge
    /// (squashed or not) everything is committed whole, as GitHub Desktop
    /// does, because leaving part out would drop the other side's changes.
    pub fn selection_locked(&self) -> bool {
        self.repo.as_ref().is_some_and(Repo::merging)
    }

    pub fn toggle_file(&mut self, path: &Path) {
        if self.selection_locked() {
            return;
        }
        if let Some(repo) = self.repo.as_mut() {
            repo.inclusion.toggle_file(path);
        }
    }

    pub fn toggle_all(&mut self) {
        if self.selection_locked() {
            return;
        }
        if let Some(repo) = self.repo.as_mut() {
            let rows = repo.rows();
            repo.inclusion.toggle_all(&rows);
        }
    }

    /// Whether lines can be ticked in the shown diff: a text diff of a
    /// working file with whitespace shown (with it hidden, the lines do not
    /// match what git applies), as GitHub Desktop does.
    pub fn lines_selectable(&self) -> bool {
        let Some(repo) = &self.repo else {
            return false;
        };
        !self.settings.hide_whitespace
            && !repo.merging()
            && matches!(
                &repo.diff,
                Some(ShownDiff {
                    key: DiffKey::Working(_),
                    content: DiffContent::Text(_)
                })
            )
    }

    /// Ticks the lines when any is unticked, else unticks them all.
    pub fn toggle_lines(&mut self, targets: &[(usize, usize)]) {
        if !self.lines_selectable() {
            return;
        }
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let Some(ShownDiff {
            key: DiffKey::Working(path),
            content: DiffContent::Text(diff),
        }) = &repo.diff
        else {
            return;
        };
        let FileDiff::Text { hunks } = diff else {
            return;
        };
        let all_on = targets.iter().all(|&(h, l)| {
            hunks
                .get(h)
                .and_then(|hunk| hunk.lines.get(l))
                .is_none_or(|line| repo.inclusion.line_included(path, line))
        });
        let (path, diff) = (path.clone(), diff.clone());
        repo.inclusion.set_lines(&path, &diff, targets, !all_on);
    }

    pub fn toggle_hunk(&mut self, hunk: usize) {
        let Some(ShownDiff {
            content: DiffContent::Text(diff),
            ..
        }) = self.repo.as_ref().and_then(|r| r.diff.as_ref())
        else {
            return;
        };
        let lines = changes::hunk_lines(diff, hunk);
        self.toggle_lines(&lines);
    }

    pub fn set_filter(&mut self, filter: &str) {
        if let Some(repo) = self.repo.as_mut() {
            repo.filter = filter.to_string();
        }
    }

    /// Re-reads the shown diff with the current options.
    fn reload_diff(&mut self) {
        let Some(repo) = &self.repo else {
            return;
        };
        match (repo.tab, &repo.stash) {
            (_, Some(stash)) => {
                if let Some(index) = stash.file {
                    self.select_stash_file(index);
                }
            }
            (Tab::Changes, None) => {
                if let Some(path) = repo.selected_file.clone() {
                    self.select_file(&path);
                }
            }
            (Tab::History, None) => {
                if let Some(index) = repo.history.file {
                    self.select_commit_file(index);
                }
            }
        }
    }

    pub fn set_hide_whitespace(&mut self, hide: bool) {
        self.settings.hide_whitespace = hide;
        self.save_settings();
        self.reload_diff();
    }

    pub fn set_split_diff(&mut self, split: bool) {
        self.settings.split_diff = split;
        self.save_settings();
    }

    pub fn set_whole_file(&mut self, whole: bool) {
        if let Some(repo) = self.repo.as_mut() {
            repo.whole_file = whole;
        }
        self.reload_diff();
    }

    pub fn show_large_diff(&mut self) {
        if let Some(repo) = self.repo.as_mut() {
            repo.show_large = true;
        }
    }

    // --- commit -----------------------------------------------------------

    pub fn can_commit(&self) -> bool {
        self.repo.is_some() && self.commit_blocked().is_none()
    }

    pub fn commit_blocked(&self) -> Option<crate::commit::Blocked> {
        let repo = self.repo.as_ref()?;
        repo.form.blocked(
            repo.inclusion.any_included(),
            self.is_busy(),
            repo.in_conflict(),
        )
    }

    pub fn commit(&mut self) {
        if !self.can_commit() {
            return;
        }
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let files = repo.inclusion.selections(&repo.status);
        repo.push_after_commit = repo.form.push_after;
        let op = Op::Commit {
            files,
            message: repo.form.message(),
            options: repo.form.options(),
        };
        self.push_op(op);
    }

    /// "Amend last commit": the form takes the last commit's message and
    /// the next commit replaces it.
    pub fn amend_last(&mut self) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let Some(last) = repo.history.commits.first().cloned() else {
            return;
        };
        repo.form.restore(&last.summary, &last.body);
        repo.form.amend = true;
        repo.tab = Tab::Changes;
    }

    // --- discarding and ignoring ------------------------------------------

    /// Asks before discarding when the settings say so.
    pub fn ask_discard(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if self.settings.confirm_discard {
            self.dialog = Some(Dialog::DiscardFiles { paths });
        } else {
            self.discard(paths);
        }
    }

    pub fn ask_discard_all(&mut self) {
        let paths = self
            .repo
            .as_ref()
            .map(|r| r.status.changes.iter().map(|c| c.path.clone()).collect())
            .unwrap_or_default();
        self.ask_discard(paths);
    }

    pub fn discard(&mut self, paths: Vec<PathBuf>) {
        self.dialog = None;
        let Some(repo) = &self.repo else {
            return;
        };
        let files: Vec<FileSelection> = repo
            .status
            .changes
            .iter()
            .filter(|c| paths.contains(&c.path))
            .map(|c| FileSelection {
                path: c.path.clone(),
                original_path: c.original_path.clone(),
                lines: None,
            })
            .collect();
        self.push_op(Op::Discard(files));
    }

    pub fn ask_discard_lines(&mut self) {
        let Some(path) = self.repo.as_ref().and_then(|r| r.selected_file.clone()) else {
            return;
        };
        if self.settings.confirm_discard {
            self.dialog = Some(Dialog::DiscardLines { path });
        } else {
            self.discard_lines();
        }
    }

    /// Discards the ticked lines of the shown file.
    pub fn discard_lines(&mut self) {
        self.dialog = None;
        let Some(repo) = &self.repo else {
            return;
        };
        let Some(path) = repo.selected_file.clone() else {
            return;
        };
        let Some(file) = repo
            .inclusion
            .selections(&repo.status)
            .into_iter()
            .find(|f| f.path == path)
        else {
            return;
        };
        if file.lines.is_none() {
            self.discard(vec![path]);
        } else {
            self.push_op(Op::DiscardLines(file));
        }
    }

    pub fn ignore_file(&mut self, path: &Path) {
        self.push_op(Op::Ignore(git_platform::patch::path_pattern(path)));
    }

    pub fn ignore_extension(&mut self, path: &Path) {
        if let Some(pattern) = git_platform::patch::extension_pattern(path) {
            self.push_op(Op::Ignore(pattern));
        }
    }

    // --- history ----------------------------------------------------------

    pub fn load_more_history(&mut self) {
        let Some(repo) = &self.repo else {
            return;
        };
        if repo.history.end || self.queue.iter().any(|j| matches!(j.op, Op::Log { .. })) {
            return;
        }
        let skip = repo.history.commits.len();
        self.push_op(Op::Log { skip });
    }

    pub fn select_commit(&mut self, index: usize) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let Some(commit) = repo.history.commits.get(index) else {
            return;
        };
        let sha = commit.sha.clone();
        repo.history.selected = Some(index);
        repo.history.files.clear();
        repo.history.file = None;
        repo.diff = None;
        self.push_op(Op::CommitFiles(sha));
    }

    pub fn select_commit_file(&mut self, index: usize) {
        let options = self.diff_options();
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let Some(commit) = repo
            .history
            .selected
            .and_then(|i| repo.history.commits.get(i))
        else {
            return;
        };
        let Some(file) = repo.history.files.get(index).cloned() else {
            return;
        };
        let key = DiffKey::Commit {
            sha: commit.sha.clone(),
            path: file.path.clone(),
        };
        repo.history.file = Some(index);
        self.push_op(Op::Diff {
            key,
            original: file.original_path.clone(),
            options,
            image: false,
            conflicted: false,
            file: Some(file),
        });
    }

    fn edit(&mut self, kind: OpKind, edit: HistoryEdit) {
        self.dialog = None;
        self.push_op(Op::Edit { kind, edit });
    }

    pub fn ask_undo_commit(&mut self) {
        if self.settings.confirm_undo_commit {
            self.dialog = Some(Dialog::UndoCommit);
        } else {
            self.undo_commit();
        }
    }

    /// Undoes the last commit and puts its message back in the form, as
    /// GitHub Desktop does. The message is read from git when the job runs,
    /// not from the history list, which may be older than HEAD.
    pub fn undo_commit(&mut self) {
        self.dialog = None;
        self.push_op(Op::UndoCommit);
    }

    pub fn reset_to(&mut self, sha: String) {
        self.edit(OpKind::Reset, HistoryEdit::ResetTo { sha });
    }

    pub fn revert(&mut self, index: usize) {
        let Some(commit) = self.commit_at(index) else {
            return;
        };
        self.edit(
            OpKind::Revert,
            HistoryEdit::Revert {
                merge: commit.is_merge(),
                sha: commit.sha,
            },
        );
    }

    pub fn cherry_pick(&mut self, index: usize, onto: Option<String>) {
        let Some(commit) = self.commit_at(index) else {
            return;
        };
        self.edit(
            OpKind::CherryPick,
            HistoryEdit::CherryPick {
                shas: vec![commit.sha],
                onto,
            },
        );
    }

    pub fn commit_at(&self, index: usize) -> Option<Commit> {
        self.repo.as_ref()?.history.commits.get(index).cloned()
    }

    /// Whether the commit at `index` (and everything newer) may be
    /// rewritten by reorder or squash.
    pub fn can_rewrite(&self, index: usize) -> bool {
        let Some(repo) = &self.repo else {
            return false;
        };
        let ahead = repo
            .status
            .upstream
            .as_ref()
            .map(|_| repo.status.ahead_behind.map(|(a, _)| a).unwrap_or(0));
        crate::history::can_rewrite(&repo.history.commits, index, ahead)
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if !self.can_rewrite(from.max(to)) {
            return;
        }
        let Some(repo) = &self.repo else {
            return;
        };
        if let Some(edit) = crate::history::reorder(&repo.history.commits, from, to) {
            self.edit(OpKind::Reorder, edit);
        }
    }

    /// Dropping a commit on another: opens the squash dialog.
    pub fn ask_squash(&mut self, from: usize, to: usize) {
        if from == to || !self.can_rewrite(from.max(to)) {
            return;
        }
        self.dialog = Some(Dialog::Squash { from, to });
    }

    pub fn squash_message(&self, from: usize, to: usize) -> String {
        match (self.commit_at(to), self.commit_at(from)) {
            (Some(target), Some(folded)) => crate::history::squash_message(&target, &folded),
            _ => String::new(),
        }
    }

    pub fn squash(&mut self, from: usize, to: usize, message: &str) {
        let Some(repo) = &self.repo else {
            return;
        };
        if let Some(edit) = crate::history::squash(&repo.history.commits, from, to, message) {
            self.edit(OpKind::Squash, edit);
        }
    }

    pub fn create_tag(&mut self, sha: String, name: &str) {
        self.dialog = None;
        let name = sanitize_ref_name(name);
        if let Some(repo) = self.repo.as_mut() {
            repo.unpushed_tags.push(name.clone());
        }
        self.push_op(Op::CreateTag { name, sha });
    }

    pub fn delete_tag(&mut self, name: String) {
        self.dialog = None;
        if let Some(repo) = self.repo.as_mut() {
            repo.unpushed_tags.retain(|t| *t != name);
        }
        self.push_op(Op::DeleteTag(name));
    }

    /// Checks out a commit; HEAD becomes detached, which the window warns
    /// about.
    pub fn checkout_commit(&mut self, sha: String) {
        self.push_op(Op::Switch {
            target: sha,
            changes: SwitchChanges::Carry,
        });
    }

    // --- branches ---------------------------------------------------------

    pub fn create_branch(&mut self, name: &str, start: Option<String>) {
        self.dialog = None;
        let name = sanitize_ref_name(name);
        if let Some(repo) = self.repo.as_mut() {
            repo.recent_branches = crate::branches::remember(&repo.recent_branches, &name);
        }
        self.push_op(Op::CreateBranch {
            name,
            start,
            checkout: true,
        });
    }

    /// Switching with uncommitted changes asks whether to leave them on this
    /// branch or bring them.
    pub fn switch_branch(&mut self, target: &str) {
        let Some(repo) = &self.repo else {
            return;
        };
        if repo.branch() == Some(target) {
            return;
        }
        if repo.has_changes() {
            self.dialog = Some(Dialog::SwitchWithChanges {
                target: target.to_string(),
                bring: false,
            });
            return;
        }
        self.switch_with(target, false);
    }

    pub fn switch_with(&mut self, target: &str, bring: bool) {
        self.dialog = None;
        let has_changes = self.repo.as_ref().is_some_and(Repo::has_changes);
        let changes = match (has_changes, bring) {
            (false, _) => SwitchChanges::Carry,
            (true, true) => SwitchChanges::Bring,
            (true, false) => SwitchChanges::Leave,
        };
        if let Some(repo) = self.repo.as_mut() {
            let local = match target.split_once('/') {
                Some((remote, name)) if repo.info.remotes.iter().any(|x| x.name == remote) => name,
                _ => target,
            };
            repo.recent_branches = crate::branches::remember(&repo.recent_branches, local);
        }
        self.push_op(Op::Switch {
            target: target.to_string(),
            changes,
        });
    }

    pub fn rename_branch(&mut self, from: String, to: &str) {
        self.dialog = None;
        self.push_op(Op::RenameBranch {
            from,
            to: sanitize_ref_name(to),
        });
    }

    pub fn ask_delete_branch(&mut self, name: String) {
        let has_remote = self.repo.as_ref().is_some_and(|r| {
            r.info
                .branches
                .iter()
                .any(|b| !b.remote && b.name == name && b.upstream.is_some() && !b.upstream_gone)
        });
        self.dialog = Some(Dialog::DeleteBranch {
            name,
            has_remote,
            delete_remote: false,
        });
    }

    pub fn delete_branch(&mut self, name: String, delete_remote: bool) {
        self.dialog = None;
        let network = self.network_options();
        // The branch's name on the remote is its upstream's, which need
        // not be its local name.
        let remote = if delete_remote {
            self.repo.as_ref().and_then(|r| {
                let upstream = r
                    .info
                    .branches
                    .iter()
                    .find(|b| !b.remote && b.name == name)?
                    .upstream
                    .clone()?;
                r.split_remote_branch(&upstream)
            })
        } else {
            None
        };
        self.push_op(Op::DeleteBranch {
            name,
            remote,
            network,
        });
    }

    pub fn pick_branch(&mut self, purpose: BranchPurpose) {
        self.dialog = Some(Dialog::PickBranch {
            purpose,
            selected: None,
        });
    }

    pub fn branch_picked(&mut self, purpose: BranchPurpose, branch: String) {
        self.dialog = None;
        match purpose {
            BranchPurpose::Merge => self.push_op(Op::Merge {
                branch,
                squash: false,
            }),
            BranchPurpose::SquashMerge => self.push_op(Op::Merge {
                branch,
                squash: true,
            }),
            BranchPurpose::Rebase => self.push_op(Op::Rebase { onto: branch }),
            BranchPurpose::Compare => self.compare(branch),
        }
    }

    pub fn compare(&mut self, branch: String) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let base = repo.branch().unwrap_or("HEAD").to_string();
        repo.tab = Tab::History;
        repo.compare = Some(CompareView {
            branch: branch.clone(),
            ..CompareView::default()
        });
        self.push_op(Op::Compare { branch, base });
    }

    pub fn close_compare(&mut self) {
        if let Some(repo) = self.repo.as_mut() {
            repo.compare = None;
        }
    }

    /// Merges the default branch into this one ("Update from main"),
    /// preferring the remote's copy of it.
    pub fn update_from_default(&mut self) {
        let Some(repo) = &self.repo else {
            return;
        };
        let Some(default) = repo.default_branch() else {
            return;
        };
        let branch = match repo.push_remote() {
            Some(remote)
                if repo
                    .info
                    .branches
                    .iter()
                    .any(|b| b.remote && b.name == format!("{remote}/{default}")) =>
            {
                format!("{remote}/{default}")
            }
            _ => default,
        };
        self.push_op(Op::Merge {
            branch,
            squash: false,
        });
    }

    // --- stash ------------------------------------------------------------

    pub fn stash_all(&mut self) {
        let Some(repo) = &self.repo else {
            return;
        };
        let branch = repo.branch().unwrap_or("HEAD").to_string();
        self.push_op(Op::StashAll {
            message: git_core::stash::stash_message(&branch),
        });
    }

    /// The stash this branch has, when Better Git or GitHub Desktop made it.
    pub fn branch_stash(&self) -> Option<&git_core::Stash> {
        let repo = self.repo.as_ref()?;
        git_core::stash::stash_for_branch(&repo.info.stashes, repo.branch()?)
    }

    pub fn view_stash(&mut self, name: String) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        repo.tab = Tab::Changes;
        repo.stash = Some(StashView {
            name: name.clone(),
            ..StashView::default()
        });
        repo.diff = None;
        self.push_op(Op::StashFiles(name));
    }

    pub fn close_stash(&mut self) {
        if let Some(repo) = self.repo.as_mut() {
            repo.stash = None;
            repo.diff = None;
        }
    }

    pub fn select_stash_file(&mut self, index: usize) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let Some(stash) = repo.stash.as_mut() else {
            return;
        };
        let Some(file) = stash.files.get(index).cloned() else {
            return;
        };
        stash.file = Some(index);
        let key = DiffKey::Stash {
            name: stash.name.clone(),
            path: file.path.clone(),
        };
        self.push_op(Op::Diff {
            key,
            original: file.original_path.clone(),
            options: DiffOptions::default(),
            image: false,
            conflicted: false,
            file: Some(file),
        });
    }

    pub fn restore_stash(&mut self, name: String) {
        if let Some(repo) = self.repo.as_mut() {
            repo.stash = None;
            repo.diff = None;
            repo.stash_conflict = Some(name.clone());
        }
        self.notice = None;
        self.push_op(Op::RestoreStash(name));
    }

    pub fn drop_stash(&mut self, name: String) {
        self.dialog = None;
        if let Some(repo) = self.repo.as_mut() {
            repo.stash = None;
            repo.diff = None;
        }
        self.push_op(Op::DropStash(name));
    }

    // --- worktrees --------------------------------------------------------

    pub fn add_worktree(&mut self, path: PathBuf, branch: &str, create: bool) {
        self.dialog = None;
        let branch = if create {
            sanitize_ref_name(branch)
        } else {
            branch.trim().to_string()
        };
        self.push_op(Op::AddWorktree {
            path,
            branch,
            create,
        });
    }

    pub fn rename_worktree(&mut self, from: PathBuf, to: PathBuf) {
        self.dialog = None;
        self.push_op(Op::RenameWorktree { from, to });
    }

    pub fn remove_worktree(&mut self, path: PathBuf, force: bool) {
        self.dialog = None;
        self.push_op(Op::RemoveWorktree { path, force });
    }

    // --- remotes ----------------------------------------------------------

    fn credentials_for(&self, hosted: Option<&HostedRepository>) -> Option<Credentials> {
        match (&self.github.account, hosted) {
            (Some(account), Some(hosted)) if crate::github::same_host(account, hosted) => {
                Some(Credentials {
                    host: account.host.name.clone(),
                    token: account.token.access_token.clone(),
                })
            }
            _ => None,
        }
    }

    /// The token for this repository's host, when the person signed in to
    /// it, and the tags waiting to be pushed.
    pub fn network_options(&self) -> NetworkOptions {
        let repo = self.repo.as_ref();
        NetworkOptions {
            credentials: self.credentials_for(repo.and_then(|r| r.hosted.as_ref())),
            tags: repo.map(|r| r.unpushed_tags.clone()).unwrap_or_default(),
            cancel: self.cancel.clone(),
            stall: None,
        }
    }

    pub fn fetch(&mut self, background: bool, now: i64) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        if repo.info.remotes.is_empty() {
            return;
        }
        repo.last_fetch = Some(now);
        if background {
            if self.background_running || self.background.is_some() {
                return;
            }
            self.background_cancel = Cancel::new();
            let network = NetworkOptions {
                cancel: self.background_cancel.clone(),
                ..self.network_options()
            };
            let Some(repo) = &self.repo else {
                return;
            };
            self.background = Some(Job {
                generation: self.generation,
                repository: repo.root.clone(),
                op: Op::Fetch {
                    background: true,
                    network,
                },
            });
            return;
        }
        let network = self.network_options();
        self.push_op(Op::Fetch {
            background,
            network,
        });
    }

    /// Whether the background fetch is due.
    pub fn fetch_due(&self, now: i64) -> bool {
        self.settings.background_fetch
            && self.repo.as_ref().is_some_and(|r| {
                !r.info.remotes.is_empty()
                    && r.last_fetch
                        .is_none_or(|t| now - t >= crate::remote::BACKGROUND_FETCH_SECONDS as i64)
            })
    }

    pub fn pull(&mut self) {
        let network = self.network_options();
        self.push_op(Op::Pull(network));
    }

    /// Pushes the current branch to its upstream. A force push overwrites
    /// only `lease`: the commit the person saw on the remote branch.
    pub fn push_with(&mut self, force: Option<Lease>) {
        self.dialog = None;
        let Some(repo) = &self.repo else {
            return;
        };
        let (Some(branch), Some((remote, remote_branch, set_upstream))) =
            (repo.branch().map(str::to_string), repo.push_target())
        else {
            return;
        };
        let network = self.network_options();
        self.push_op(Op::Push {
            request: git_platform::PushRequest {
                remote,
                branch,
                remote_branch,
                set_upstream,
                force,
            },
            network,
        });
    }

    pub fn push(&mut self, force: bool) {
        let lease = force.then(|| Lease {
            expected: self.repo.as_ref().and_then(Repo::seen_on_remote),
        });
        self.push_with(lease);
    }

    /// Asks before a force push, remembering what the remote branch was on
    /// when asked: a background fetch while the question is open must not
    /// widen what the push may overwrite.
    pub fn ask_force_push(&mut self) {
        let expected = self.repo.as_ref().and_then(Repo::seen_on_remote);
        if self.settings.confirm_force_push {
            self.dialog = Some(Dialog::ForcePush { expected });
        } else {
            self.push_with(Some(Lease { expected }));
        }
    }

    pub fn add_remote(&mut self, name: &str, url: &str) {
        self.push_op(Op::AddRemote {
            name: name.trim().to_string(),
            url: url.trim().to_string(),
        });
    }

    pub fn remove_remote(&mut self, name: String) {
        self.push_op(Op::RemoveRemote(name));
    }

    pub fn set_remote_url(&mut self, name: String, url: &str) {
        self.push_op(Op::SetRemoteUrl {
            name,
            url: url.trim().to_string(),
        });
    }

    // --- conflicts --------------------------------------------------------

    pub fn resolve_with(&mut self, path: PathBuf, ours: bool) {
        self.push_op(Op::ResolveWith { path, ours });
    }

    /// Marks a file resolved, asking first when markers are still in it.
    pub fn ask_mark_resolved(&mut self, path: PathBuf, markers_left: bool) {
        if markers_left {
            self.dialog = Some(Dialog::MarkResolved { path });
        } else {
            self.mark_resolved(path);
        }
    }

    pub fn mark_resolved(&mut self, path: PathBuf) {
        self.dialog = None;
        self.push_op(Op::MarkResolved(path));
    }

    pub fn continue_operation(&mut self) {
        let Some(repo) = &self.repo else {
            return;
        };
        if let Some(operation) = repo.info.operation {
            self.push_op(Op::Continue(operation));
        } else if let Some(name) = repo.stash_conflict.clone() {
            self.push_op(Op::FinishStashConflict(name));
        } else {
            self.push_op(Op::FinishLooseConflict);
        }
    }

    pub fn abort_operation(&mut self) {
        let Some(repo) = &self.repo else {
            return;
        };
        if let Some(operation) = repo.info.operation {
            self.push_op(Op::Abort(operation));
        } else if repo.in_conflict() {
            // `git reset --merge`: puts back what the restore touched and
            // touches no stash.
            self.push_op(Op::AbortStashConflict);
        }
    }

    // --- repositories -----------------------------------------------------

    pub fn add_local(&mut self, path: PathBuf) {
        self.dialog = None;
        self.open(path);
    }

    pub fn clone_repository(&mut self, url: String, destination: PathBuf) {
        self.dialog = None;
        let network = NetworkOptions {
            credentials: self.credentials_for(parse_hosted(&url).as_ref()),
            tags: Vec::new(),
            cancel: self.cancel.clone(),
            stall: None,
        };
        self.generation += 1;
        self.queue.push_back(Job {
            generation: self.generation,
            repository: destination.clone(),
            op: Op::Clone {
                url,
                destination,
                network,
            },
        });
    }

    /// Creates a repository. With a GitHub template chosen while signed in,
    /// the template's text is fetched first.
    pub fn create_repository(
        &mut self,
        new: NewRepository,
        gitignore: Option<String>,
        license: Option<String>,
    ) {
        self.dialog = None;
        if (gitignore.is_some() || license.is_some()) && self.github.account.is_some() {
            self.github.pending_init = Some(new);
            self.push_gh(GhCall::Template { gitignore, license });
            return;
        }
        self.queue_init(new);
    }

    fn queue_init(&mut self, new: NewRepository) {
        self.generation += 1;
        self.queue.push_back(Job {
            generation: self.generation,
            repository: new.path.clone(),
            op: Op::Init(new),
        });
    }

    pub fn remove_repository(&mut self, path: &Path) {
        self.dialog = None;
        if let Err(error) = self.repositories.remove(path) {
            self.problem = Some(Problem::Io(error.to_string()));
        }
        self.recent.retain(|p| p != path);
        if let Some(file) = &self.recent_file {
            let _ = crate::recent::save(file, &self.recent);
        }
        if self.repo.as_ref().is_some_and(|r| r.root == path) {
            self.close_repository();
        }
    }

    pub fn ask_remove_repository(&mut self, path: PathBuf) {
        if self.settings.confirm_remove_repository {
            self.dialog = Some(Dialog::RemoveRepository { path });
        } else {
            self.remove_repository(&path);
        }
    }

    pub fn set_alias(&mut self, path: &Path, alias: &str) {
        self.dialog = None;
        if let Err(error) = self.repositories.set_alias(path, alias) {
            self.problem = Some(Problem::Io(error.to_string()));
        }
    }

    /// A missing repository was found elsewhere.
    pub fn relocate(&mut self, from: &Path, to: PathBuf) {
        if let Err(error) = self.repositories.relocate(from, &to) {
            self.problem = Some(Problem::Io(error.to_string()));
        }
        self.open(to);
    }

    // --- settings ---------------------------------------------------------

    pub fn save_settings(&mut self) {
        if let Err(error) = self.settings.save(self.settings_file.as_deref()) {
            self.problem = Some(Problem::Io(error.to_string()));
        }
    }

    pub fn open_settings(&mut self) {
        self.page = Page::Settings;
        if !self.git_config.loaded {
            self.queue.push_back(Job {
                generation: self.generation,
                repository: PathBuf::from("/"),
                op: Op::ReadGitConfig,
            });
        }
    }

    pub fn set_git_config(&mut self, values: Vec<(String, String)>) {
        self.queue.push_back(Job {
            generation: self.generation,
            repository: PathBuf::from("/"),
            op: Op::SetGitConfig(values),
        });
    }

    pub fn dismiss_problem(&mut self) {
        self.problem = None;
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    pub fn cancel_dialog(&mut self) {
        if let Some(Dialog::SignIn { .. }) = self.dialog {
            self.github.sign_in = SignIn::Idle;
        }
        self.dialog = None;
    }

    pub fn platform_failed(&mut self, message: impl Into<String>) {
        self.problem = Some(Problem::Platform(message.into()));
    }

    pub fn launch_failed(&mut self, message: impl Into<String>) {
        self.problem = Some(Problem::Launch(message.into()));
    }

    pub fn keyring_failed(&mut self, message: impl Into<String>) {
        self.problem = Some(Problem::Keyring(message.into()));
    }

    // --- GitHub -----------------------------------------------------------

    /// Restores the account the window read from the keyring.
    pub fn restore_account(&mut self, account: Account) {
        self.github.account = Some(account);
        self.refresh_github();
    }

    pub fn start_sign_in(&mut self, host: Host) {
        // An Enterprise server may name its own OAuth App; GitHub.com and
        // a server that names none use this build's.
        let host = match host.client_id {
            Some(_) => host,
            None => host.with_client_id(self.client_id),
        };
        if host.client_id.is_none() {
            self.problem = Some(Problem::NoClientId);
            return;
        }
        self.github.sign_in = SignIn::Starting;
        self.dialog = Some(Dialog::SignIn { enterprise: false });
        // A sign-in request carries no old token.
        self.gh_queue.push_back(GhRequest {
            host: host.clone(),
            token: None,
            call: GhCall::DeviceCode,
        });
        self.github.host = Some(host);
    }

    /// One poll, when the window's timer says the interval passed.
    pub fn poll_sign_in(&mut self) {
        let SignIn::Waiting { code, .. } = &self.github.sign_in else {
            return;
        };
        if self.gh_running || !self.gh_queue.is_empty() {
            return;
        }
        let device_code = code.device_code.clone();
        let host = self.github.host.clone().unwrap_or_else(Host::github);
        self.gh_queue.push_back(GhRequest {
            host,
            token: None,
            call: GhCall::Poll { device_code },
        });
    }

    pub fn sign_out(&mut self) {
        if let Some(account) = self.github.account.take() {
            self.keyring_changes
                .push(KeyringChange::Delete(account.host.name.clone()));
        }
        self.github.repositories = None;
        self.github.organizations.clear();
        if let Some(repo) = self.repo.as_mut() {
            repo.github = RepoGitHub::default();
        }
    }

    /// Reads pull requests, checks, and protection for the open repository,
    /// and the person's own pull requests for notifications.
    pub fn refresh_github(&mut self) {
        let Some(account) = &self.github.account else {
            return;
        };
        let Some(repo) = &self.repo else {
            return;
        };
        let Some(hosted) = repo.hosted.clone() else {
            return;
        };
        if !crate::github::same_host(account, &hosted) {
            return;
        }
        let branch = repo.branch().map(str::to_string);
        self.push_gh(GhCall::RepositoryState {
            repo: hosted.clone(),
            branch,
        });
        self.push_gh(GhCall::MyPulls { repo: hosted });
    }

    pub fn load_account_repositories(&mut self) {
        if self.github.account.is_some() && self.github.repositories.is_none() {
            self.push_gh(GhCall::Repositories);
        }
    }

    pub fn load_organizations(&mut self) {
        if self.github.account.is_some() {
            self.push_gh(GhCall::Organizations);
        }
    }

    pub fn load_templates(&mut self) {
        if self.github.account.is_some() && self.github.licenses.is_empty() {
            self.push_gh(GhCall::Templates);
        }
    }

    pub fn rerun_checks(&mut self) {
        let Some(repo) = &self.repo else {
            return;
        };
        let (Some(hosted), Some(checks)) = (repo.hosted.clone(), repo.github.checks.clone()) else {
            return;
        };
        self.push_gh(GhCall::RerunChecks {
            repo: hosted,
            checks,
        });
    }

    /// The open pull request for the current branch, if any.
    pub fn current_pull(&self) -> Option<&PullRequest> {
        let repo = self.repo.as_ref()?;
        let branch = repo.branch()?;
        repo.github.pulls.iter().find(|p| p.head.name == branch)
    }

    pub fn create_pull(&mut self, title: &str, body: &str, base: String, draft: bool) {
        self.dialog = None;
        let Some(repo) = &self.repo else {
            return;
        };
        let (Some(hosted), Some(head)) = (repo.hosted.clone(), repo.branch().map(str::to_string))
        else {
            return;
        };
        self.push_gh(GhCall::CreatePull {
            repo: hosted,
            title: title.trim().to_string(),
            body: body.to_string(),
            head,
            base,
            draft,
        });
    }

    pub fn create_issue(&mut self, title: &str, body: &str) {
        self.dialog = None;
        let Some(hosted) = self.repo.as_ref().and_then(|r| r.hosted.clone()) else {
            return;
        };
        self.push_gh(GhCall::CreateIssue {
            repo: hosted,
            title: title.trim().to_string(),
            body: body.to_string(),
        });
    }

    pub fn publish(
        &mut self,
        name: &str,
        description: &str,
        private: bool,
        organization: Option<String>,
    ) {
        self.dialog = None;
        let Some(repo) = &self.repo else {
            return;
        };
        self.github.pending_publish = Some((repo.root.clone(), self.generation));
        self.push_gh(GhCall::Publish {
            name: crate::publish::sanitize_repository_name(name),
            description: description.trim().to_string(),
            private,
            organization,
        });
    }

    pub fn fork(&mut self) {
        self.dialog = None;
        let Some(hosted) = self.repo.as_ref().and_then(|r| r.hosted.clone()) else {
            return;
        };
        self.push_gh(GhCall::Fork { repo: hosted });
    }

    /// The clone dialog opens on the account's list when signed in.
    pub fn open_clone_dialog(&mut self) {
        let tab = if self.github.account.is_some() {
            CloneTab::GitHub
        } else {
            CloneTab::Url
        };
        self.dialog = Some(Dialog::Clone {
            tab,
            selected: None,
        });
        self.load_account_repositories();
    }

    pub fn include_state(&self, path: &Path) -> Include {
        self.repo
            .as_ref()
            .map(|r| r.inclusion.get(path))
            .unwrap_or(Include::Nothing)
    }

    // --- answers ----------------------------------------------------------

    fn remember_open(&mut self, root: &Path) {
        if let Err(error) = self.repositories.add(root) {
            self.problem = Some(Problem::Io(error.to_string()));
        }
        self.recent = crate::recent::remember(&self.recent, root.to_path_buf());
        if let Some(file) = &self.recent_file
            && let Err(error) = crate::recent::save(file, &self.recent)
        {
            self.problem = Some(Problem::Io(error.to_string()));
        }
    }

    fn idle(&mut self) {
        if self.queue.is_empty()
            && let Some(repo) = self.repo.as_mut()
        {
            repo.busy = None;
        }
    }

    pub fn apply(&mut self, outcome: Outcome) {
        self.running = false;
        if let Answer::GitConfig(result) = outcome.answer {
            match result {
                Ok(values) => {
                    self.git_config = GitConfig {
                        values,
                        loaded: true,
                    }
                }
                Err(error) => {
                    self.problem = Some(Problem::Git {
                        kind: OpKind::GitConfig,
                        error,
                    })
                }
            }
            return;
        }
        if outcome.generation != self.generation {
            return;
        }
        let read_failed = |error| Problem::Git {
            kind: OpKind::Refresh,
            error,
        };
        match outcome.answer {
            Answer::Opened { root, snapshot } => match snapshot {
                Ok((status, info)) => {
                    self.remember_open(&root);
                    let mut repo = Repo::new(root, status, info);
                    if let Some(branch) = repo.branch() {
                        repo.recent_branches = vec![branch.to_string()];
                    }
                    // Conflicts with no operation of git's (a stash restore
                    // that stopped before this window opened) are shown as
                    // conflicts too, but no stash is guessed at: finishing
                    // them unstages the files and touches no stash.
                    self.repo = Some(repo);
                    self.problem = None;
                    self.page = Page::Main;
                    self.refresh_github();
                }
                Err(error) => self.problem = Some(read_failed(error)),
            },
            Answer::OpenFailed(error) => self.problem = Some(read_failed(error)),
            Answer::Changed {
                kind,
                result,
                snapshot,
            } => self.changed(kind, result, snapshot),
            Answer::Undone {
                last,
                result,
                snapshot,
            } => {
                if result.is_ok()
                    && let (Some(repo), Some(last)) = (self.repo.as_mut(), last)
                {
                    repo.form.restore(&last.summary, &last.body);
                }
                self.changed(OpKind::UndoCommit, result, snapshot)
            }
            Answer::Diff { key, content } => {
                let Some(repo) = self.repo.as_mut() else {
                    return;
                };
                if let (DiffKey::Working(path), DiffContent::Text(diff)) = (&key, &content) {
                    repo.inclusion.reconcile(path, diff);
                }
                repo.diff = Some(ShownDiff { key, content });
                self.idle();
            }
            Answer::Log { skip, result } => {
                let Some(repo) = self.repo.as_mut() else {
                    return;
                };
                match result {
                    Ok(commits) => {
                        if skip == 0 {
                            repo.history.commits.clear();
                        }
                        repo.history.end = commits.len() < LOG_PAGE;
                        repo.history.commits.extend(commits);
                        repo.history.loaded = true;
                    }
                    Err(error) => self.problem = Some(read_failed(error)),
                }
                self.idle();
            }
            Answer::CommitFiles { sha, result } => {
                let Some(repo) = self.repo.as_mut() else {
                    return;
                };
                let current = repo
                    .history
                    .selected
                    .and_then(|i| repo.history.commits.get(i));
                let mut first = false;
                if current.is_some_and(|c| c.sha == sha) {
                    match result {
                        Ok(files) => {
                            first = !files.is_empty();
                            repo.history.files = files;
                        }
                        Err(error) => self.problem = Some(read_failed(error)),
                    }
                }
                self.idle();
                if first {
                    self.select_commit_file(0);
                }
            }
            Answer::Compare { branch, result } => {
                let Some(repo) = self.repo.as_mut() else {
                    return;
                };
                match result {
                    Ok((ahead, behind)) => {
                        repo.compare = Some(CompareView {
                            branch,
                            ahead,
                            behind,
                        })
                    }
                    Err(error) => {
                        repo.compare = None;
                        self.problem = Some(read_failed(error));
                    }
                }
                self.idle();
            }
            Answer::StashFiles { name, result } => {
                let Some(repo) = self.repo.as_mut() else {
                    return;
                };
                let mut first = false;
                match result {
                    Ok(files) => {
                        if let Some(stash) = repo.stash.as_mut().filter(|s| s.name == name) {
                            first = !files.is_empty();
                            stash.files = files;
                        }
                    }
                    Err(error) => self.problem = Some(read_failed(error)),
                }
                self.idle();
                if first {
                    self.select_stash_file(0);
                }
            }
            Answer::Created { kind, path, result } => match result {
                Ok(()) => {
                    // Made here, so trusted.
                    if let Err(error) = self.trust.trust(&path) {
                        self.problem = Some(Problem::Io(error.to_string()));
                    }
                    self.notice = Some(Notice::Done(kind));
                    self.open_trusted(path);
                }
                Err(error) => self.problem = Some(Problem::Git { kind, error }),
            },
            Answer::GitConfig(_) => {}
        }
    }

    fn changed(&mut self, kind: OpKind, result: Result<(), GitError>, snapshot: Snapshot) {
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        let before_branch = repo.branch().map(str::to_string);
        let before_head = repo.status.head_oid.clone();
        match snapshot {
            Ok((status, info)) => {
                repo.hosted = hosted_of(&info);
                repo.status = status;
                repo.info = info;
                repo.sync_inclusion();
            }
            Err(error) => {
                self.problem = Some(Problem::Git {
                    kind: OpKind::Refresh,
                    error,
                })
            }
        }
        let Some(repo) = self.repo.as_mut() else {
            return;
        };
        if let Some(path) = &repo.selected_file
            && !repo.status.changes.iter().any(|c| &c.path == path)
        {
            repo.selected_file = None;
            if matches!(
                repo.diff,
                Some(ShownDiff {
                    key: DiffKey::Working(_),
                    ..
                })
            ) {
                repo.diff = None;
            }
        }
        let ok = result.is_ok();
        let conflicts = matches!(result, Err(GitError::Conflicts(_)));
        match result {
            Ok(()) => {
                if !matches!(
                    kind,
                    OpKind::Refresh | OpKind::BackgroundFetch | OpKind::Resolve
                ) {
                    self.notice = Some(Notice::Done(kind));
                }
                if matches!(self.problem, Some(Problem::Git { .. })) {
                    self.problem = None;
                }
            }
            Err(error) => {
                // A background fetch fails quietly; the next one retries.
                if kind != OpKind::BackgroundFetch {
                    self.problem = Some(Problem::Git { kind, error });
                    self.notice = None;
                }
            }
        }
        let repo = self.repo.as_mut().expect("checked above");
        match kind {
            OpKind::Commit if ok => {
                repo.form.clear();
            }
            OpKind::Push | OpKind::ForcePush if ok => repo.unpushed_tags.clear(),
            OpKind::RestoreStash if !conflicts => repo.stash_conflict = None,
            OpKind::Continue | OpKind::Abort if ok => repo.stash_conflict = None,
            _ => {}
        }
        let push_after = kind == OpKind::Commit && std::mem::take(&mut repo.push_after_commit);
        let rewrites_history = matches!(
            kind,
            OpKind::UndoCommit
                | OpKind::Reset
                | OpKind::Revert
                | OpKind::CherryPick
                | OpKind::Reorder
                | OpKind::Squash
                | OpKind::CreateTag
                | OpKind::DeleteTag
                | OpKind::Switch
                | OpKind::CreateBranch
                | OpKind::Merge
                | OpKind::Rebase
                | OpKind::Pull
                | OpKind::Continue
                | OpKind::Abort
                | OpKind::Commit
        );
        // HEAD moved some other way (another window, a terminal): the list
        // is stale, and a drag on it would rewrite the wrong commits.
        let head_moved = repo.status.head_oid != before_head;
        if rewrites_history || head_moved {
            repo.history.loaded = false;
            repo.history.selected = None;
            repo.history.files.clear();
            if repo.tab == Tab::History {
                repo.diff = None;
            }
        }
        let after_branch = repo.branch().map(str::to_string);
        let reload_history = repo.tab == Tab::History && !repo.history.loaded;
        let reselect = (repo.tab == Tab::Changes && repo.stash.is_none())
            .then(|| repo.selected_file.clone())
            .flatten();
        self.idle();
        if push_after && ok {
            self.push(false);
        }
        if reload_history {
            self.push_op(Op::Log { skip: 0 });
        }
        if ok && before_branch != after_branch {
            if let Some(stash) = self.branch_stash() {
                self.notice = Some(Notice::StashWaiting(stash.name.clone()));
            }
            self.refresh_github();
        } else if ok
            && matches!(
                kind,
                OpKind::Push | OpKind::ForcePush | OpKind::Fetch | OpKind::Pull
            )
        {
            self.refresh_github();
        }
        if let Some(path) = reselect
            && kind != OpKind::BackgroundFetch
        {
            self.select_file(&path);
        }
    }

    pub fn apply_gh(&mut self, answer: GhAnswer) {
        self.gh_running = false;
        if let (Some(token), Some(account)) =
            (answer.refreshed.clone(), self.github.account.as_mut())
        {
            account.token = token;
            self.keyring_changes
                .push(KeyringChange::Store(account.clone()));
        }
        let result = match answer.result {
            Ok(result) => result,
            Err(error) => {
                self.gh_failed(&answer.call, error);
                return;
            }
        };
        match result {
            GhResult::DeviceCode(code) => {
                let interval = code.interval.max(5);
                self.github.sign_in = SignIn::Waiting { code, interval };
            }
            GhResult::Poll(poll) => self.polled(poll),
            GhResult::User(user) => {
                if let Some(account) = self.github.account.as_mut() {
                    account.login = user.login.clone();
                    self.keyring_changes
                        .push(KeyringChange::Store(account.clone()));
                }
                if matches!(self.github.sign_in, SignIn::Waiting { .. }) {
                    self.github.sign_in = SignIn::Idle;
                    if matches!(self.dialog, Some(Dialog::SignIn { .. })) {
                        self.dialog = None;
                    }
                    self.notice = Some(Notice::SignedIn(user.login));
                }
                self.refresh_github();
            }
            GhResult::Repositories(repos) => self.github.repositories = Some(repos),
            GhResult::Organizations(orgs) => self.github.organizations = orgs,
            GhResult::RepositoryState {
                pulls,
                checks,
                protected,
                can_push,
                parent,
            } => {
                if let Some(repo) = self.repo.as_mut() {
                    repo.github = RepoGitHub {
                        pulls,
                        checks,
                        protected,
                        can_push,
                        parent,
                        loaded: true,
                    };
                }
            }
            GhResult::Rerun(count) => {
                self.notice = Some(Notice::ChecksRerun(count));
                self.refresh_github();
            }
            GhResult::CreatedPull(pull) => {
                self.notice = Some(Notice::PullCreated {
                    number: pull.number,
                    url: pull.html_url,
                });
                self.refresh_github();
            }
            GhResult::CreatedIssue(issue) => {
                self.notice = Some(Notice::IssueCreated {
                    number: issue.number,
                    url: issue.html_url,
                })
            }
            GhResult::Published(info) => self.published(info),
            GhResult::Forked(info) => {
                // The fork becomes `origin`; the original stays as
                // `upstream`, as GitHub Desktop sets it up.
                let original = self
                    .repo
                    .as_ref()
                    .and_then(|r| r.info.remotes.iter().find(|x| x.name == "origin").cloned());
                if let Some(original) = original {
                    self.push_op(Op::AddRemote {
                        name: "upstream".into(),
                        url: original.fetch_url,
                    });
                }
                self.push_op(Op::SetRemoteUrl {
                    name: "origin".into(),
                    url: info.clone_url.clone(),
                });
                self.notice = Some(Notice::Forked {
                    name: info.full_name,
                });
            }
            GhResult::MyPulls(pulls) => {
                for notice in self.pull_notices.update(&pulls) {
                    self.desktop_notices.push(notice.clone());
                    self.notice = Some(Notice::Pull(notice));
                }
            }
            GhResult::Templates {
                gitignores,
                licenses,
            } => {
                self.github.gitignores = gitignores;
                self.github.licenses = licenses;
            }
            GhResult::Template { gitignore, license } => {
                if let Some(mut new) = self.github.pending_init.take() {
                    if gitignore.is_some() {
                        new.gitignore = gitignore;
                    }
                    if let Some(license) = license {
                        let name = self.git_config.get("user.name").to_string();
                        let year = crate::publish::current_year(crate::github::now());
                        new.license = Some(crate::publish::fill_license(&license, year, &name));
                    }
                    self.queue_init(new);
                }
            }
        }
    }

    fn gh_failed(&mut self, call: &GhCall, error: ApiError) {
        match call {
            GhCall::DeviceCode | GhCall::Poll { .. } => {
                self.github.sign_in = SignIn::Failed(SignInProblem::Api(error));
            }
            GhCall::MyPulls { .. } | GhCall::RepositoryState { .. } => {
                // Quiet: the repository may not be on GitHub, or the network
                // is down; the window just shows less. A revoked token is
                // worth saying.
                if error == ApiError::Unauthorized {
                    self.problem = Some(Problem::GitHub(error));
                }
            }
            GhCall::Template { .. } => {
                if let Some(new) = self.github.pending_init.take() {
                    self.queue_init(new);
                }
                self.problem = Some(Problem::GitHub(error));
            }
            GhCall::Publish { .. } => {
                self.github.pending_publish = None;
                self.problem = Some(Problem::GitHub(error));
            }
            _ => self.problem = Some(Problem::GitHub(error)),
        }
    }

    fn polled(&mut self, poll: Poll) {
        match poll {
            Poll::Pending => {}
            Poll::SlowDown(interval) => {
                if let SignIn::Waiting {
                    interval: current, ..
                } = &mut self.github.sign_in
                {
                    *current = interval.max(*current + 5);
                }
            }
            Poll::Expired => self.github.sign_in = SignIn::Failed(SignInProblem::Expired),
            Poll::Denied => self.github.sign_in = SignIn::Failed(SignInProblem::Denied),
            Poll::Disabled => self.github.sign_in = SignIn::Failed(SignInProblem::Disabled),
            Poll::Token(token) => {
                let host = self.github.host.clone().unwrap_or_else(Host::github);
                self.github.account = Some(Account {
                    host,
                    login: String::new(),
                    token,
                });
                self.push_gh(GhCall::User);
            }
        }
    }

    fn published(&mut self, info: RepoInfo) {
        let pending = self.github.pending_publish.take();
        let still_open = pending.is_some_and(|(root, generation)| {
            generation == self.generation && self.repo.as_ref().is_some_and(|r| r.root == root)
        });
        if !still_open {
            return;
        }
        self.notice = Some(Notice::Published {
            url: info.html_url.clone(),
        });
        self.push_op(Op::AddRemote {
            name: "origin".into(),
            url: info.clone_url.clone(),
        });
        let Some(branch) = self
            .repo
            .as_ref()
            .and_then(|r| r.branch().map(str::to_string))
        else {
            return;
        };
        let mut network = self.network_options();
        if let Some(account) = &self.github.account {
            network.credentials = Some(Credentials {
                host: account.host.name.clone(),
                token: account.token.access_token.clone(),
            });
        }
        self.push_op(Op::Push {
            request: git_platform::PushRequest {
                remote: "origin".into(),
                remote_branch: branch.clone(),
                branch,
                set_upstream: true,
                force: None,
            },
            network,
        });
    }
}
