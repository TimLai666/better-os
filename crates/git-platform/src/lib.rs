//! Running the system `git` for Better Git.
//!
//! Every operation is one or more `git` processes started with an argument
//! vector and `-C <repository>`. Nothing goes through a shell, every path
//! argument follows `--` so a file named `-x` can never be read as an
//! option, and `LC_ALL=C` is set so that what little prose git prints is in
//! one language. The person's own configuration, credential helper, and SSH
//! keys are left as they are.
//!
//! Opening a folder must not run what the folder brings with it. A repository
//! copied from elsewhere carries its own `.git/config` and hooks, so every
//! command turns off `core.fsmonitor`, every command except the ones that
//! make commits turns off hooks, and `diff` is run with `--no-textconv` and
//! `--no-ext-diff`. A `filter.<name>.clean` command named by the repository's
//! attributes would still run when git reads a working-tree file; git has no
//! switch for it, so the window asks "Do you trust this repository?" before
//! the first read of a path ([`trust`]).
//!
//! Settings of the repository's or the person's own configuration that
//! would change what Better Git reads or writes are overridden on every
//! command: `diff.noprefix` and `diff.mnemonicPrefix` (a patch built from
//! a diff without `a/` and `b/` would be applied to the wrong path),
//! `log.showSignature` (which runs `gpg.program` for every commit listed),
//! and `rebase.missingCommitsCheck` is `error`, so a rewrite that would
//! drop a commit stops instead.
//!
//! The `GIT_DIR` family of variables is dropped so `-C` alone names the
//! repository. Network commands get `GIT_TERMINAL_PROMPT=0` and an `ssh`
//! that asks no one (`SSH_ASKPASS_REQUIRE=force` with `false`), so git
//! never waits on a terminal nobody sees; they run in their own process
//! group, which is killed when the person cancels or when nothing has been
//! printed for [`NETWORK_STALL`]. A GitHub token, when the person signed
//! in, reaches git through a credential helper that reads it from a file
//! only this user can read, made for the one command and removed after it
//! ([`credentials`]); it is never on a command line or in the environment
//! of the programs git starts.
//!
//! The parsing lives in `git-core`. This crate decides which commands to run
//! and hands the bytes over.

pub mod branches;
pub mod config;
pub mod conflicts;
pub mod credentials;
pub mod history;
pub mod keyring;
pub mod patch;
pub mod remote;
pub mod stash;
pub mod tags;
pub mod trust;
pub mod worktrees;

use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use git_core::{
    Branch, Commit, CommitFile, FileDiff, Operation, ParseError, Remote, RepositoryStatus, Stash,
    Tag, Worktree, parse_diff, parse_status,
};
use thiserror::Error;

pub use branches::SwitchChanges;
pub use history::{CommitOptions, HistoryEdit, TodoAction};
pub use patch::FileSelection;
pub use remote::{Lease, PushRequest};

/// Why an operation did not happen.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum GitError {
    /// No `git` program could be started on this machine.
    #[error("git.error.not_installed")]
    NotInstalled,
    /// The path is not inside a git working tree.
    #[error("git.error.not_a_repository:{0}")]
    NotARepository(PathBuf),
    /// git ran and refused. `message` is its own stderr, trimmed, for the
    /// person to read.
    #[error("git.error.failed:{message}")]
    Failed {
        exit_code: Option<i32>,
        message: String,
    },
    /// git printed something `git-core` could not read.
    #[error("git.error.unreadable_output:{0}")]
    UnreadableOutput(String),
    /// The operation stopped with conflicts the person has to resolve. The
    /// message is git's.
    #[error("git.error.conflicts:{0}")]
    Conflicts(String),
    /// What the window asked for no longer matches the repository (a
    /// selection made on a diff that changed meanwhile).
    #[error("git.error.changed_meanwhile")]
    ChangedMeanwhile,
    /// A file outside git could not be read or written. The text is the
    /// system's.
    #[error("git.error.io:{0}")]
    Io(String),
    /// The person cancelled a network command; it was stopped.
    #[error("git.error.cancelled")]
    Cancelled,
    /// A network command printed nothing for too long and was stopped.
    #[error("git.error.stalled")]
    Stalled,
    /// This `Git` does not do that (a test fake).
    #[error("git.error.unsupported:{0}")]
    Unsupported(&'static str),
}

impl GitError {
    /// git's own message, when there is one.
    pub fn message(&self) -> Option<&str> {
        match self {
            GitError::Failed { message, .. } | GitError::Conflicts(message) => Some(message),
            _ => None,
        }
    }
}

/// Which side of a file's change to show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffSide {
    /// Working tree against the index.
    Unstaged,
    /// Index against HEAD (or against nothing, in a repository with no commit).
    Staged,
    /// Working tree against HEAD: everything a commit of this file would
    /// record. This is what the changes list shows, as GitHub Desktop does.
    WorkingTree,
}

/// How a diff is printed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiffOptions {
    /// `-w`: whitespace-only changes are not shown.
    pub ignore_whitespace: bool,
    /// The whole file as context, not three lines.
    pub whole_file: bool,
}

impl DiffOptions {
    pub(crate) fn args(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        if self.ignore_whitespace {
            args.push(OsString::from("-w"));
        }
        if self.whole_file {
            args.push(OsString::from("--unified=1000000"));
        }
        args
    }
}

/// A file's bytes at HEAD and in the working tree; `None` where that side
/// has no file.
pub type FileVersions = (Option<Vec<u8>>, Option<Vec<u8>>);

/// What a commit needs from the person.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitMessage {
    pub summary: String,
    /// Optional; an empty string means no description.
    pub description: String,
}

impl CommitMessage {
    /// The full message git gets: summary, a blank line and the description
    /// when there is one, and the `Co-authored-by` trailers after another
    /// blank line.
    pub fn text(&self, co_authors: &[(String, String)]) -> String {
        let mut text = self.summary.trim().to_string();
        let description = self.description.trim();
        if !description.is_empty() {
            text.push_str("\n\n");
            text.push_str(description);
        }
        if !co_authors.is_empty() {
            text.push_str("\n\n");
            let trailers: Vec<String> = co_authors
                .iter()
                .map(|(name, email)| format!("Co-authored-by: {name} <{email}>"))
                .collect();
            text.push_str(&trailers.join("\n"));
        }
        text.push('\n');
        text
    }
}

/// A token for one GitHub host, handed to network commands.
#[derive(Clone, Eq, PartialEq)]
pub struct Credentials {
    /// `github.com`, or an Enterprise host.
    pub host: String,
    pub token: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("host", &self.host)
            .field("token", &"<hidden>")
            .finish()
    }
}

/// How long a network command may print nothing before it is stopped.
/// Network commands are run with `--progress`, so a transfer that is still
/// moving keeps printing.
pub const NETWORK_STALL: Duration = Duration::from_secs(120);

/// A switch the window flips to stop the network command it started.
/// Clones share the switch.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Cancel::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl PartialEq for Cancel {
    /// Two switches are the same when they are one switch.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Cancel {}

/// What a fetch, pull, or push is told.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkOptions {
    pub credentials: Option<Credentials>,
    /// Tags to push with the branch (made here and not yet pushed).
    pub tags: Vec<String>,
    /// Flipped by the window to stop the command.
    pub cancel: Cancel,
    /// How long the command may print nothing; `None` is [`NETWORK_STALL`].
    pub stall: Option<Duration>,
}

/// The repository's state that is not the changes list: which operation it
/// is in the middle of, its branches, remotes, tags, stashes, and worktrees.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepositoryInfo {
    pub operation: Option<Operation>,
    pub branches: Vec<Branch>,
    pub remotes: Vec<Remote>,
    pub tags: Vec<Tag>,
    pub stashes: Vec<Stash>,
    pub worktrees: Vec<Worktree>,
    /// `origin/HEAD`'s target (`origin/main`), when known.
    pub remote_head: Option<String>,
    /// What the two sides of the operation's conflicts are called.
    pub sides: Option<conflicts::ConflictSides>,
    /// Conflict regions left in each conflicted file that has markers,
    /// counted off the window's thread (`None` when the file could not be
    /// read: missing, not a plain file, or over the size limit). Filled by
    /// the window's snapshot, not by `info`.
    pub conflict_markers: Vec<(PathBuf, Option<usize>)>,
}

/// What to make when creating a repository.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NewRepository {
    pub path: PathBuf,
    pub default_branch: String,
    /// The README's text, when one is wanted.
    pub readme: Option<String>,
    /// The `.gitignore` text, when one was chosen.
    pub gitignore: Option<String>,
    /// The `LICENSE` text, when one was chosen.
    pub license: Option<String>,
}

macro_rules! unsupported {
    ($name:literal) => {
        Err(GitError::Unsupported($name))
    };
}

/// The operations Better Git uses. A test gives the window a fake of this
/// trait; `SystemGit` is the real one and implements every method. A fake
/// implements what its test needs and leaves the rest answering
/// `Unsupported`.
#[allow(unused_variables)]
pub trait Git: Send + Sync {
    /// The root of the working tree that contains `path`.
    fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError>;
    fn status(&self, repository: &Path) -> Result<RepositoryStatus, GitError>;
    /// The diff of one path, relative to the repository root.
    fn diff(&self, repository: &Path, path: &Path, side: DiffSide) -> Result<FileDiff, GitError>;
    /// Stages the paths as they are in the working tree, deletions included.
    fn stage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError>;
    /// Takes the paths back out of the index and leaves the working tree alone.
    fn unstage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError>;
    /// Commits what is staged.
    fn commit(&self, repository: &Path, message: &CommitMessage) -> Result<(), GitError>;

    /// The diff of one changed file with options.
    fn diff_with(
        &self,
        repository: &Path,
        path: &Path,
        side: DiffSide,
        options: DiffOptions,
    ) -> Result<FileDiff, GitError> {
        self.diff(repository, path, side)
    }
    /// The file's bytes at HEAD (`None` when it is not there) and in the
    /// working tree (`None` when deleted), for an image diff.
    fn file_versions(
        &self,
        repository: &Path,
        path: &Path,
        original: Option<&Path>,
    ) -> Result<FileVersions, GitError> {
        unsupported!("file_versions")
    }
    /// Branches, remotes, tags, stashes, worktrees, and the operation in
    /// progress.
    fn info(&self, repository: &Path) -> Result<RepositoryInfo, GitError> {
        unsupported!("info")
    }

    // Changes
    /// Builds the index from the selection and commits it.
    fn commit_selection(
        &self,
        repository: &Path,
        files: &[FileSelection],
        message: &CommitMessage,
        options: &CommitOptions,
    ) -> Result<(), GitError> {
        unsupported!("commit_selection")
    }
    /// Discards the files' changes, copying each to the desktop's trash first.
    fn discard_files(&self, repository: &Path, files: &[FileSelection]) -> Result<(), GitError> {
        unsupported!("discard_files")
    }
    /// Discards the selected lines of one file, copying it to the trash first.
    fn discard_lines(&self, repository: &Path, file: &FileSelection) -> Result<(), GitError> {
        unsupported!("discard_lines")
    }
    /// Adds a line to the repository's `.gitignore`.
    fn ignore(&self, repository: &Path, pattern: &str) -> Result<(), GitError> {
        unsupported!("ignore")
    }

    // History
    fn log(
        &self,
        repository: &Path,
        revision: &str,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<Commit>, GitError> {
        unsupported!("log")
    }
    fn commit_files(&self, repository: &Path, sha: &str) -> Result<Vec<CommitFile>, GitError> {
        unsupported!("commit_files")
    }
    fn commit_diff(
        &self,
        repository: &Path,
        sha: &str,
        file: &CommitFile,
        options: DiffOptions,
    ) -> Result<FileDiff, GitError> {
        unsupported!("commit_diff")
    }
    fn edit_history(&self, repository: &Path, edit: &HistoryEdit) -> Result<(), GitError> {
        unsupported!("edit_history")
    }

    // Branches
    fn create_branch(
        &self,
        repository: &Path,
        name: &str,
        start: Option<&str>,
        checkout: bool,
    ) -> Result<(), GitError> {
        unsupported!("create_branch")
    }
    fn switch(
        &self,
        repository: &Path,
        target: &str,
        changes: SwitchChanges,
    ) -> Result<(), GitError> {
        unsupported!("switch")
    }
    fn rename_branch(&self, repository: &Path, from: &str, to: &str) -> Result<(), GitError> {
        unsupported!("rename_branch")
    }
    fn delete_branch(&self, repository: &Path, name: &str, force: bool) -> Result<(), GitError> {
        unsupported!("delete_branch")
    }
    fn merge(&self, repository: &Path, branch: &str, squash: bool) -> Result<(), GitError> {
        unsupported!("merge")
    }
    fn rebase(&self, repository: &Path, onto: &str) -> Result<(), GitError> {
        unsupported!("rebase")
    }
    /// The commits `head` has that `base` does not, and how many `base` has
    /// that `head` does not.
    fn compare(
        &self,
        repository: &Path,
        base: &str,
        head: &str,
    ) -> Result<(Vec<Commit>, usize), GitError> {
        unsupported!("compare")
    }

    // Stash
    fn stash_all(&self, repository: &Path, message: &str) -> Result<(), GitError> {
        unsupported!("stash_all")
    }
    /// Applies a stash and drops it when it applied cleanly. Conflicts come
    /// back as `GitError::Conflicts`, with the stash kept.
    fn stash_pop(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        unsupported!("stash_pop")
    }
    fn stash_drop(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        unsupported!("stash_drop")
    }
    fn stash_files(&self, repository: &Path, name: &str) -> Result<Vec<CommitFile>, GitError> {
        unsupported!("stash_files")
    }
    fn stash_diff(
        &self,
        repository: &Path,
        name: &str,
        file: &CommitFile,
    ) -> Result<FileDiff, GitError> {
        unsupported!("stash_diff")
    }

    // Tags
    fn create_tag(&self, repository: &Path, name: &str, sha: &str) -> Result<(), GitError> {
        unsupported!("create_tag")
    }
    fn delete_tag(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        unsupported!("delete_tag")
    }

    // Worktrees
    fn add_worktree(
        &self,
        repository: &Path,
        path: &Path,
        branch: &str,
        create: bool,
    ) -> Result<(), GitError> {
        unsupported!("add_worktree")
    }
    fn move_worktree(&self, repository: &Path, from: &Path, to: &Path) -> Result<(), GitError> {
        unsupported!("move_worktree")
    }
    fn remove_worktree(&self, repository: &Path, path: &Path, force: bool) -> Result<(), GitError> {
        unsupported!("remove_worktree")
    }

    // Remotes
    fn fetch(
        &self,
        repository: &Path,
        remote: Option<&str>,
        options: &NetworkOptions,
    ) -> Result<(), GitError> {
        unsupported!("fetch")
    }
    fn pull(&self, repository: &Path, options: &NetworkOptions) -> Result<(), GitError> {
        unsupported!("pull")
    }
    fn push(
        &self,
        repository: &Path,
        request: &PushRequest,
        options: &NetworkOptions,
    ) -> Result<(), GitError> {
        unsupported!("push")
    }
    fn delete_remote_branch(
        &self,
        repository: &Path,
        remote: &str,
        branch: &str,
        options: &NetworkOptions,
    ) -> Result<(), GitError> {
        unsupported!("delete_remote_branch")
    }
    fn add_remote(&self, repository: &Path, name: &str, url: &str) -> Result<(), GitError> {
        unsupported!("add_remote")
    }
    fn remove_remote(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        unsupported!("remove_remote")
    }
    fn set_remote_url(&self, repository: &Path, name: &str, url: &str) -> Result<(), GitError> {
        unsupported!("set_remote_url")
    }
    fn clone_repository(
        &self,
        url: &str,
        destination: &Path,
        options: &NetworkOptions,
    ) -> Result<(), GitError> {
        unsupported!("clone_repository")
    }
    fn init_repository(&self, new: &NewRepository) -> Result<(), GitError> {
        unsupported!("init_repository")
    }

    // Conflicts
    fn resolve_with(&self, repository: &Path, path: &Path, ours: bool) -> Result<(), GitError> {
        unsupported!("resolve_with")
    }
    fn mark_resolved(&self, repository: &Path, path: &Path) -> Result<(), GitError> {
        unsupported!("mark_resolved")
    }
    fn continue_operation(&self, repository: &Path, operation: Operation) -> Result<(), GitError> {
        unsupported!("continue_operation")
    }
    fn abort_operation(&self, repository: &Path, operation: Operation) -> Result<(), GitError> {
        unsupported!("abort_operation")
    }
    /// After a stash that stopped with conflicts was resolved: unstage the
    /// result and drop the stash.
    fn finish_stash_conflict(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        unsupported!("finish_stash_conflict")
    }
    /// Ends a conflict that no operation of git's owns (a stash restore
    /// whose stash is not known any more): the resolved files are left
    /// unstaged, as a clean restore leaves them, and no stash is touched.
    fn finish_loose_conflict(&self, repository: &Path) -> Result<(), GitError> {
        unsupported!("finish_loose_conflict")
    }
    /// Puts the files back as they were before a stash that stopped with
    /// conflicts; the stash stays.
    fn abort_stash_conflict(&self, repository: &Path) -> Result<(), GitError> {
        unsupported!("abort_stash_conflict")
    }

    // Configuration
    /// A global configuration value (`user.name`, `user.email`,
    /// `init.defaultBranch`).
    fn global_config(&self, key: &str) -> Result<Option<String>, GitError> {
        unsupported!("global_config")
    }
    fn set_global_config(&self, key: &str, value: &str) -> Result<(), GitError> {
        unsupported!("set_global_config")
    }
}

/// Runs the `git` found on `PATH`.
#[derive(Clone, Debug, Default)]
pub struct SystemGit;

/// What one `git` process printed, with its exit status.
pub(crate) struct Captured {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

impl Captured {
    pub(crate) fn code(&self) -> Option<i32> {
        self.status.code()
    }
}

/// Whether the repository's hooks may run for a command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Hooks {
    Off,
    /// For the commands that make commits, where the person expects their
    /// hooks.
    On,
}

/// Settings forced on every command; see the crate's documentation.
const OVERRIDES: [&str; 4] = [
    "diff.noprefix=false",
    "diff.mnemonicPrefix=false",
    "log.showSignature=false",
    "rebase.missingCommitsCheck=error",
];

fn spawn_writer(
    child: &mut std::process::Child,
    bytes: Vec<u8>,
) -> Option<std::thread::JoinHandle<io::Result<()>>> {
    let mut pipe = child.stdin.take()?;
    Some(std::thread::spawn(move || match pipe.write_all(&bytes) {
        Err(error) if error.kind() != ErrorKind::BrokenPipe => Err(error),
        _ => Ok(()),
    }))
}

/// Waits for a network command while reading what it prints, and kills its
/// process group (git and the `ssh` or helper it started) when the person
/// cancels or when it has printed nothing for the stall limit.
fn watch(mut child: std::process::Child, network: &NetworkOptions) -> Result<Captured, GitError> {
    use std::io::Read;
    use std::sync::Mutex;
    use std::time::Instant;

    let last_output = Arc::new(Mutex::new(Instant::now()));
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        let last_output = last_output.clone();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut pipe) = pipe {
                let mut chunk = [0u8; 8192];
                while let Ok(n) = pipe.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    out.extend_from_slice(&chunk[..n]);
                    *last_output.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
                }
            }
            out
        })
    };
    let stdout = reader(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = reader(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stall = network.stall.unwrap_or(NETWORK_STALL);
    let pid = child.id() as i32;
    let mut stopped = None;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(io_failed)? {
            break status;
        }
        let quiet = last_output
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .elapsed();
        if stopped.is_none() {
            if network.cancel.is_cancelled() {
                stopped = Some(GitError::Cancelled);
            } else if quiet > stall {
                stopped = Some(GitError::Stalled);
            }
            if stopped.is_some() {
                // SAFETY: `pid` is our child's, which leads its own process
                // group (`process_group(0)`) and has not been reaped.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if let Some(error) = stopped {
        return Err(error);
    }
    Ok(Captured {
        status,
        stdout,
        stderr,
    })
}

/// Variables that would point git at another repository than `-C` names.
const REPOSITORY_VARIABLES: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
];

/// One `git` process, described before it starts.
pub(crate) struct Run<'a> {
    repository: &'a Path,
    args: Vec<OsString>,
    stdin: Option<Vec<u8>>,
    hooks: Hooks,
    env: Vec<(OsString, OsString)>,
    network: Option<NetworkOptions>,
    literal: bool,
}

impl<'a> Run<'a> {
    pub(crate) fn new<I, S>(repository: &'a Path, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Run {
            repository,
            args: args.into_iter().map(|a| a.as_ref().to_owned()).collect(),
            stdin: None,
            hooks: Hooks::Off,
            env: Vec::new(),
            network: None,
            literal: true,
        }
    }

    /// Without `--literal-pathspecs`, for a command given no paths that git
    /// handles wrongly with it: `stash push --include-untracked` leaves
    /// untracked files behind under that flag (git 2.43).
    pub(crate) fn without_literal_pathspecs(mut self) -> Self {
        self.literal = false;
        self
    }

    pub(crate) fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    pub(crate) fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_owned()));
        self
    }

    /// `--` and then the paths.
    pub(crate) fn paths<'p>(mut self, paths: impl IntoIterator<Item = &'p Path>) -> Self {
        self.args.push(OsString::from("--"));
        self.args
            .extend(paths.into_iter().map(|p| p.as_os_str().to_owned()));
        self
    }

    pub(crate) fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(bytes.into());
        self
    }

    pub(crate) fn hooks(mut self, hooks: Hooks) -> Self {
        self.hooks = hooks;
        self
    }

    pub(crate) fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env
            .push((key.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    /// A command that reaches a remote: no prompts, cancellable, stopped
    /// when it stalls, and the person's token for its host, when there is
    /// one.
    pub(crate) fn network(mut self, options: &NetworkOptions) -> Self {
        self.network = Some(options.clone());
        self
    }

    /// Runs git and captures what it printed.
    pub(crate) fn capture(self) -> Result<Captured, GitError> {
        // `--literal-pathspecs` stops a file name such as `*.txt` from
        // matching other paths as a pattern.
        let mut command = Command::new("git");
        if self.literal {
            command.arg("--literal-pathspecs");
        }
        command.args(["-c", "core.fsmonitor=false"]);
        for setting in OVERRIDES {
            command.args(["-c", setting]);
        }
        if self.hooks == Hooks::Off {
            command.args(["-c", "core.hooksPath=/dev/null"]);
        }
        for variable in REPOSITORY_VARIABLES {
            command.env_remove(variable);
        }
        // Kept until git has exited: the helper reads the token from it.
        let mut _helper = None;
        if let Some(network) = &self.network {
            command
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GCM_INTERACTIVE", "never")
                .env("SSH_ASKPASS", "false")
                .env("SSH_ASKPASS_REQUIRE", "force");
            std::os::unix::process::CommandExt::process_group(&mut command, 0);
            if let Some(credentials) = &network.credentials {
                let helper = credentials::Helper::new(credentials).map_err(io_error)?;
                command.args(helper.config_args());
                _helper = Some(helper);
            }
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let mut child = command
            .arg("-C")
            .arg(self.repository)
            .args(&self.args)
            .env("LC_ALL", "C")
            .stdin(if self.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                ErrorKind::NotFound => GitError::NotInstalled,
                _ => io_failed(error),
            })?;
        if let Some(network) = &self.network {
            let writer = self.stdin.map(|bytes| spawn_writer(&mut child, bytes));
            let result = watch(child, network);
            if let Some(writer) = writer.flatten() {
                let _ = writer.join();
            }
            return result;
        }

        // The message is written from its own thread while this one reads
        // the output, so a hook that prints a lot before git reads its
        // message cannot deadlock against a full pipe.
        let writer = match (self.stdin, child.stdin.take()) {
            (Some(bytes), Some(mut pipe)) => Some(std::thread::spawn(move || {
                match pipe.write_all(&bytes) {
                    Ok(()) => Ok(()),
                    // git may stop reading early; its exit status decides.
                    Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
                    Err(error) => Err(error),
                }
            })),
            _ => None,
        };
        let output = child.wait_with_output().map_err(io_failed)?;
        if let Some(writer) = writer {
            match writer.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(io_failed(error)),
                Err(_) => return Err(GitError::Io("writing to git failed".to_string())),
            }
        }
        Ok(Captured {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// Runs git and returns its stdout, or its refusal.
    pub(crate) fn output(self) -> Result<Vec<u8>, GitError> {
        ensure_success(self.capture()?)
    }

    /// Runs git for its effect.
    pub(crate) fn run(self) -> Result<(), GitError> {
        self.output().map(|_| ())
    }
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub(crate) fn io_failed(error: io::Error) -> GitError {
    GitError::Failed {
        exit_code: None,
        message: error.to_string(),
    }
}

pub(crate) fn io_error(error: io::Error) -> GitError {
    GitError::Io(error.to_string())
}

/// The error for a run that git refused: its trimmed stderr, or its trimmed
/// stdout when stderr is empty, or both. A run that stopped on conflicts
/// (git says `CONFLICT (` on either) is `Conflicts`.
pub(crate) fn failed(captured: &Captured) -> GitError {
    let stderr = without_progress(&String::from_utf8_lossy(&captured.stderr));
    let stdout = String::from_utf8_lossy(&captured.stdout);
    let message = match (stderr.trim().is_empty(), stdout.trim().is_empty()) {
        (true, _) => stdout.trim().to_string(),
        (false, true) => stderr.trim().to_string(),
        (false, false) => format!("{}\n{}", stdout.trim(), stderr.trim()),
    };
    if stdout.contains("CONFLICT (") || stderr.contains("CONFLICT (") {
        return GitError::Conflicts(message);
    }
    GitError::Failed {
        exit_code: captured.code(),
        message,
    }
}

/// The stdout of a run that succeeded, or the failure it reported.
pub(crate) fn ensure_success(captured: Captured) -> Result<Vec<u8>, GitError> {
    if captured.status.success() {
        Ok(captured.stdout)
    } else {
        Err(failed(&captured))
    }
}

/// git's text without the progress lines `--progress` prints (each update
/// ends in a carriage return; the last of each kind ends in `done.`).
fn without_progress(text: &str) -> String {
    const PROGRESS: [&str; 10] = [
        "Enumerating objects:",
        "Counting objects:",
        "Compressing objects:",
        "Writing objects:",
        "Receiving objects:",
        "Resolving deltas:",
        "Unpacking objects:",
        "Updating files:",
        "Total ",
        "Delta compression",
    ];
    text.split(['\n', '\r'])
        .filter(|line| {
            let line = line.trim_start_matches("remote: ").trim();
            !line.is_empty() && !PROGRESS.iter().any(|p| line.starts_with(p))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn unreadable(ParseError::Malformed(detail): ParseError) -> GitError {
    GitError::UnreadableOutput(detail)
}

pub(crate) fn refused(message: impl Into<String>) -> GitError {
    GitError::Failed {
        exit_code: None,
        message: message.into(),
    }
}

/// Whether git knows the path in its index.
pub(crate) fn is_tracked(repository: &Path, path: &Path) -> Result<bool, GitError> {
    let captured = Run::new(repository, ["ls-files", "--error-unmatch"])
        .paths([path])
        .capture()?;
    match captured.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(failed(&captured)),
    }
}

/// Whether HEAD points at a commit. False in a repository with no commit yet.
pub(crate) fn has_commit(repository: &Path) -> Result<bool, GitError> {
    let captured = Run::new(repository, ["rev-parse", "--verify", "-q", "HEAD"]).capture()?;
    match captured.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(failed(&captured)),
    }
}

/// The empty tree's id in this repository's hash, to diff against when there
/// is no HEAD.
pub(crate) fn empty_tree(repository: &Path) -> Result<String, GitError> {
    let out = Run::new(repository, ["hash-object", "-t", "tree", "/dev/null"]).output()?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// A file in the repository's git directory (`MERGE_HEAD`, `rebase-merge`),
/// as git resolves it for this worktree.
pub(crate) fn git_path(repository: &Path, name: &str) -> Result<PathBuf, GitError> {
    let out = Run::new(repository, ["rev-parse", "--git-path", name]).output()?;
    let raw = out.strip_suffix(b"\n").unwrap_or(&out);
    let path = PathBuf::from(OsStr::from_bytes(raw));
    Ok(if path.is_absolute() {
        path
    } else {
        repository.join(path)
    })
}

/// Expected results, for every method:
/// - `git` cannot be started → `GitError::NotInstalled`.
/// - git exits non-zero → `GitError::Failed` with its trimmed output, except
///   where a method says otherwise; a stop on conflicts is
///   `GitError::Conflicts`.
/// - Arguments are passed as a vector; paths are passed as `OsStr`, never
///   converted through a string, and always after `--`.
impl Git for SystemGit {
    /// `git -C <path> rev-parse --show-toplevel`. A path that is not in a
    /// working tree is `NotARepository(path)`; any other exit 128 is
    /// `Failed` with git's message. A path that does not exist is
    /// `NotARepository(path)` too.
    fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError> {
        if !path.exists() {
            return Err(GitError::NotARepository(path.to_path_buf()));
        }
        let captured = Run::new(path, ["rev-parse", "--show-toplevel"]).capture()?;
        if captured.code() == Some(128) {
            // Only "not a git repository" means what it says; a repository
            // git refuses for its owner (`safe.directory`) keeps git's own
            // explanation.
            let stderr = String::from_utf8_lossy(&captured.stderr);
            if stderr.contains("not a git repository") {
                return Err(GitError::NotARepository(path.to_path_buf()));
            }
            return Err(failed(&captured));
        }
        let stdout = ensure_success(captured)?;
        let root = stdout.strip_suffix(b"\n").unwrap_or(&stdout);
        Ok(PathBuf::from(OsStr::from_bytes(root)))
    }

    /// `git status --porcelain=v2 -z --branch --show-stash
    /// --untracked-files=all`, parsed by `git_core::parse_status`.
    fn status(&self, repository: &Path) -> Result<RepositoryStatus, GitError> {
        let out = Run::new(
            repository,
            [
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--show-stash",
                "--untracked-files=all",
            ],
        )
        .output()?;
        parse_status(&out).map_err(unreadable)
    }

    fn diff(&self, repository: &Path, path: &Path, side: DiffSide) -> Result<FileDiff, GitError> {
        self.diff_with(repository, path, side, DiffOptions::default())
    }

    /// `git diff --no-color --no-ext-diff --no-textconv [--cached | HEAD] --
    /// <path>`, parsed by `git_core::parse_diff`; see [`patch::raw_diff`].
    fn diff_with(
        &self,
        repository: &Path,
        path: &Path,
        side: DiffSide,
        options: DiffOptions,
    ) -> Result<FileDiff, GitError> {
        let raw = patch::raw_diff(repository, path, None, side, options)?;
        parse_diff(&raw).map_err(unreadable)
    }

    fn file_versions(
        &self,
        repository: &Path,
        path: &Path,
        original: Option<&Path>,
    ) -> Result<FileVersions, GitError> {
        patch::file_versions(repository, path, original)
    }

    fn info(&self, repository: &Path) -> Result<RepositoryInfo, GitError> {
        let operation = conflicts::operation(repository)?;
        Ok(RepositoryInfo {
            sides: operation.map(|o| conflicts::sides(repository, o)),
            conflict_markers: Vec::new(),
            operation,
            branches: branches::list(repository)?,
            remotes: remote::list(repository)?,
            tags: tags::list(repository)?,
            stashes: stash::list(repository)?,
            worktrees: worktrees::list(repository)?,
            remote_head: branches::remote_head(repository),
        })
    }

    /// `git add -A -- <paths>`. No paths is a no-op that runs nothing.
    fn stage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError> {
        if paths.is_empty() {
            return Ok(());
        }
        Run::new(repository, ["add", "-A"])
            .paths(paths.iter().map(PathBuf::as_path))
            .run()
    }

    /// `git restore --staged -- <paths>`; in a repository with no commit yet,
    /// `git rm --cached -r -q -- <paths>`. No paths is a no-op.
    fn unstage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError> {
        if paths.is_empty() {
            return Ok(());
        }
        let args: &[&str] = if has_commit(repository)? {
            &["restore", "--staged"]
        } else {
            &["rm", "--cached", "-r", "-q"]
        };
        Run::new(repository, args)
            .paths(paths.iter().map(PathBuf::as_path))
            .run()
    }

    /// `git commit -F -` with the message on stdin. An empty summary is
    /// refused before git runs.
    fn commit(&self, repository: &Path, message: &CommitMessage) -> Result<(), GitError> {
        history::commit(repository, message, &CommitOptions::default())
    }

    fn commit_selection(
        &self,
        repository: &Path,
        files: &[FileSelection],
        message: &CommitMessage,
        options: &CommitOptions,
    ) -> Result<(), GitError> {
        history::check_message(message, options)?;
        patch::build_index(repository, files)?;
        history::commit(repository, message, options)
    }

    fn discard_files(&self, repository: &Path, files: &[FileSelection]) -> Result<(), GitError> {
        patch::discard_files(repository, files)
    }

    fn discard_lines(&self, repository: &Path, file: &FileSelection) -> Result<(), GitError> {
        patch::discard_lines(repository, file)
    }

    fn ignore(&self, repository: &Path, pattern: &str) -> Result<(), GitError> {
        patch::ignore(repository, pattern)
    }

    fn log(
        &self,
        repository: &Path,
        revision: &str,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<Commit>, GitError> {
        history::log(repository, revision, skip, limit)
    }

    fn commit_files(&self, repository: &Path, sha: &str) -> Result<Vec<CommitFile>, GitError> {
        history::commit_files(repository, sha)
    }

    fn commit_diff(
        &self,
        repository: &Path,
        sha: &str,
        file: &CommitFile,
        options: DiffOptions,
    ) -> Result<FileDiff, GitError> {
        history::commit_diff(repository, sha, file, options)
    }

    fn edit_history(&self, repository: &Path, edit: &HistoryEdit) -> Result<(), GitError> {
        history::edit(repository, edit)
    }

    fn create_branch(
        &self,
        repository: &Path,
        name: &str,
        start: Option<&str>,
        checkout: bool,
    ) -> Result<(), GitError> {
        branches::create(repository, name, start, checkout)
    }

    fn switch(
        &self,
        repository: &Path,
        target: &str,
        changes: SwitchChanges,
    ) -> Result<(), GitError> {
        branches::switch(repository, target, changes)
    }

    fn rename_branch(&self, repository: &Path, from: &str, to: &str) -> Result<(), GitError> {
        branches::rename(repository, from, to)
    }

    fn delete_branch(&self, repository: &Path, name: &str, force: bool) -> Result<(), GitError> {
        branches::delete(repository, name, force)
    }

    fn merge(&self, repository: &Path, branch: &str, squash: bool) -> Result<(), GitError> {
        branches::merge(repository, branch, squash)
    }

    fn rebase(&self, repository: &Path, onto: &str) -> Result<(), GitError> {
        branches::rebase(repository, onto)
    }

    fn compare(
        &self,
        repository: &Path,
        base: &str,
        head: &str,
    ) -> Result<(Vec<Commit>, usize), GitError> {
        branches::compare(repository, base, head)
    }

    fn stash_all(&self, repository: &Path, message: &str) -> Result<(), GitError> {
        stash::stash_all(repository, message)
    }

    fn stash_pop(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        stash::pop(repository, name)
    }

    fn stash_drop(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        stash::drop(repository, name)
    }

    fn stash_files(&self, repository: &Path, name: &str) -> Result<Vec<CommitFile>, GitError> {
        stash::files(repository, name)
    }

    fn stash_diff(
        &self,
        repository: &Path,
        name: &str,
        file: &CommitFile,
    ) -> Result<FileDiff, GitError> {
        stash::diff(repository, name, file)
    }

    fn create_tag(&self, repository: &Path, name: &str, sha: &str) -> Result<(), GitError> {
        tags::create(repository, name, sha)
    }

    fn delete_tag(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        tags::delete(repository, name)
    }

    fn add_worktree(
        &self,
        repository: &Path,
        path: &Path,
        branch: &str,
        create: bool,
    ) -> Result<(), GitError> {
        worktrees::add(repository, path, branch, create)
    }

    fn move_worktree(&self, repository: &Path, from: &Path, to: &Path) -> Result<(), GitError> {
        worktrees::rename(repository, from, to)
    }

    fn remove_worktree(&self, repository: &Path, path: &Path, force: bool) -> Result<(), GitError> {
        worktrees::remove(repository, path, force)
    }

    fn fetch(
        &self,
        repository: &Path,
        remote: Option<&str>,
        options: &NetworkOptions,
    ) -> Result<(), GitError> {
        remote::fetch(repository, remote, options)
    }

    fn pull(&self, repository: &Path, options: &NetworkOptions) -> Result<(), GitError> {
        remote::pull(repository, options)
    }

    fn push(
        &self,
        repository: &Path,
        request: &PushRequest,
        options: &NetworkOptions,
    ) -> Result<(), GitError> {
        remote::push(repository, request, options)
    }

    fn delete_remote_branch(
        &self,
        repository: &Path,
        remote_name: &str,
        branch: &str,
        options: &NetworkOptions,
    ) -> Result<(), GitError> {
        remote::delete_branch(repository, remote_name, branch, options)
    }

    fn add_remote(&self, repository: &Path, name: &str, url: &str) -> Result<(), GitError> {
        remote::add(repository, name, url)
    }

    fn remove_remote(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        remote::remove(repository, name)
    }

    fn set_remote_url(&self, repository: &Path, name: &str, url: &str) -> Result<(), GitError> {
        remote::set_url(repository, name, url)
    }

    fn clone_repository(
        &self,
        url: &str,
        destination: &Path,
        options: &NetworkOptions,
    ) -> Result<(), GitError> {
        remote::clone(url, destination, options)
    }

    fn init_repository(&self, new: &NewRepository) -> Result<(), GitError> {
        config::init(new)
    }

    fn resolve_with(&self, repository: &Path, path: &Path, ours: bool) -> Result<(), GitError> {
        conflicts::resolve_with(repository, path, ours)
    }

    fn mark_resolved(&self, repository: &Path, path: &Path) -> Result<(), GitError> {
        conflicts::mark_resolved(repository, path)
    }

    fn continue_operation(&self, repository: &Path, operation: Operation) -> Result<(), GitError> {
        conflicts::continue_operation(repository, operation)
    }

    fn abort_operation(&self, repository: &Path, operation: Operation) -> Result<(), GitError> {
        conflicts::abort_operation(repository, operation)
    }

    fn finish_stash_conflict(&self, repository: &Path, name: &str) -> Result<(), GitError> {
        stash::finish_conflict(repository, name)
    }

    fn abort_stash_conflict(&self, repository: &Path) -> Result<(), GitError> {
        stash::abort_conflict(repository)
    }

    fn finish_loose_conflict(&self, repository: &Path) -> Result<(), GitError> {
        Run::new(repository, ["reset", "-q"]).run()
    }

    fn global_config(&self, key: &str) -> Result<Option<String>, GitError> {
        config::get_global(key)
    }

    fn set_global_config(&self, key: &str, value: &str) -> Result<(), GitError> {
        config::set_global(key, value)
    }
}
