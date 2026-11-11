//! Running the system `git` for Better Git.
//!
//! Every operation is one `git` process started with an argument vector and
//! `-C <repository>`. Nothing goes through a shell, every path argument follows
//! `--` so a file named `-x` can never be read as an option, and `LC_ALL=C` is
//! set so that what little prose git prints is in one language. The person's
//! own configuration, credential helper, and SSH keys are left as they are:
//! this crate adds no sign-in of its own.
//!
//! Opening a folder must not run what the folder brings with it. A repository
//! copied from elsewhere carries its own `.git/config` and hooks, so every
//! command turns off `core.fsmonitor`, every command except `commit` turns off
//! hooks, and `diff` is run with `--no-textconv` and `--no-ext-diff`. One gap
//! is left and known: a `filter.<name>.clean` command named by the
//! repository's attributes still runs when git reads a working-tree file.
//! The `GIT_DIR` family of variables is dropped so `-C` alone names the
//! repository.
//!
//! The parsing lives in `git-core`. This crate only decides which command to
//! run and hands the bytes over.

use std::ffi::{OsStr, OsString};
use std::io::{self, ErrorKind, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use git_core::{FileDiff, ParseError, RepositoryStatus, parse_diff, parse_status};
use thiserror::Error;

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
    /// person to read; nothing decides behaviour from it.
    #[error("git.error.failed:{message}")]
    Failed {
        exit_code: Option<i32>,
        message: String,
    },
    /// git printed something `git-core` could not read.
    #[error("git.error.unreadable_output:{0}")]
    UnreadableOutput(String),
}

/// Which side of a file's change to show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffSide {
    /// Working tree against the index.
    Unstaged,
    /// Index against HEAD (or against nothing, in a repository with no commit).
    Staged,
}

/// What a commit needs from the person.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitMessage {
    pub summary: String,
    /// Optional; an empty string means no description.
    pub description: String,
}

/// The operations the first Better Git window uses. A test gives the window a
/// fake of this trait; `SystemGit` is the real one.
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
}

/// Runs the `git` found on `PATH`.
#[derive(Clone, Debug, Default)]
pub struct SystemGit;

/// What one `git` process printed, with its exit status.
struct Captured {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Runs `git` with `-C <repository>` and the given arguments, with `LC_ALL=C`.
/// `stdin`, when given, is written to the process and then closed. Nothing
/// goes through a shell.
fn run_git<I, S>(repository: &Path, args: I, stdin: Option<&[u8]>) -> Result<Captured, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_git_with(repository, args, stdin, Hooks::Off)
}

/// Whether the repository's hooks may run for a command.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Hooks {
    Off,
    /// For `commit`, where the person expects their hooks.
    On,
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

fn run_git_with<I, S>(
    repository: &Path,
    args: I,
    stdin: Option<&[u8]>,
    hooks: Hooks,
) -> Result<Captured, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    // `--literal-pathspecs` stops a file name such as `*.txt` or `a[1].txt`
    // from matching other paths as a pattern.
    let mut command = Command::new("git");
    command
        .arg("--literal-pathspecs")
        .args(["-c", "core.fsmonitor=false"]);
    if hooks == Hooks::Off {
        command.args(["-c", "core.hooksPath=/dev/null"]);
    }
    for variable in REPOSITORY_VARIABLES {
        command.env_remove(variable);
    }
    let mut child = command
        .arg("-C")
        .arg(repository)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(if stdin.is_some() {
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

    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        match pipe.write_all(bytes) {
            Ok(()) => {}
            // git may stop reading early; its exit status decides the outcome.
            Err(error) if error.kind() == ErrorKind::BrokenPipe => {}
            Err(error) => return Err(io_failed(error)),
        }
    }

    let output = child.wait_with_output().map_err(io_failed)?;
    Ok(Captured {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn io_failed(error: io::Error) -> GitError {
    GitError::Failed {
        exit_code: None,
        message: error.to_string(),
    }
}

/// The error for a run that git refused: its trimmed stderr, or its trimmed
/// stdout when stderr is empty.
fn failed(captured: &Captured) -> GitError {
    let stderr = String::from_utf8_lossy(&captured.stderr);
    let message = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&captured.stdout).trim().to_string()
    } else {
        stderr.trim().to_string()
    };
    GitError::Failed {
        exit_code: captured.status.code(),
        message,
    }
}

/// The stdout of a run that succeeded, or the failure it reported.
fn ensure_success(captured: Captured) -> Result<Vec<u8>, GitError> {
    if captured.status.success() {
        Ok(captured.stdout)
    } else {
        Err(failed(&captured))
    }
}

fn unreadable(ParseError::Malformed(detail): ParseError) -> GitError {
    GitError::UnreadableOutput(detail)
}

fn os_args(leading: &[&str]) -> Vec<OsString> {
    leading.iter().map(OsString::from).collect()
}

/// Whether git knows the path in its index.
fn is_tracked(repository: &Path, path: &Path) -> Result<bool, GitError> {
    let mut args = os_args(&["ls-files", "--error-unmatch", "--"]);
    args.push(path.as_os_str().to_owned());
    let captured = run_git(repository, args, None)?;
    match captured.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(failed(&captured)),
    }
}

/// Whether HEAD points at a commit. False in a repository with no commit yet.
fn has_commit(repository: &Path) -> Result<bool, GitError> {
    let captured = run_git(repository, ["rev-parse", "--verify", "-q", "HEAD"], None)?;
    match captured.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(failed(&captured)),
    }
}

/// Expected results, for every method:
/// - `git` cannot be started → `GitError::NotInstalled`.
/// - git exits non-zero → `GitError::Failed` with its trimmed stderr, or its
///   trimmed stdout when stderr is empty (`git commit` reports "nothing to
///   commit" on stdout), except where a method below says otherwise.
/// - Arguments are passed as a vector; paths are passed as `OsStr`, never
///   converted through a string, and always after `--`.
impl Git for SystemGit {
    /// `git -C <path> rev-parse --show-toplevel`. A path that is not in a
    /// working tree (git exits 128 and says so) is `NotARepository(path)`; any
    /// other exit 128 is `Failed` with git's message. A path that does not
    /// exist is `NotARepository(path)` too.
    fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError> {
        if !path.exists() {
            return Err(GitError::NotARepository(path.to_path_buf()));
        }
        let captured = run_git(path, ["rev-parse", "--show-toplevel"], None)?;
        if captured.status.code() == Some(128) {
            // 128 is every fatal error. Only "not a git repository" means
            // what it says; a repository git refuses for its owner
            // (`safe.directory`) keeps git's own explanation.
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

    /// `git status --porcelain=v2 -z --branch --untracked-files=all`, parsed
    /// by `git_core::parse_status`. A parse failure is `UnreadableOutput`.
    fn status(&self, repository: &Path) -> Result<RepositoryStatus, GitError> {
        let captured = run_git(
            repository,
            [
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--untracked-files=all",
            ],
            None,
        )?;
        parse_status(&ensure_success(captured)?).map_err(unreadable)
    }

    /// `git diff --no-color --no-ext-diff --no-textconv [--cached] -- <path>`, parsed by
    /// `git_core::parse_diff`. An untracked file has no diff from git, so for
    /// `Unstaged` on an untracked path this shows the whole file as added:
    /// `git diff --no-color --no-ext-diff --no-textconv --no-index -- /dev/null <path>`,
    /// whose exit code 1 means "differences found" and is not a failure.
    fn diff(&self, repository: &Path, path: &Path, side: DiffSide) -> Result<FileDiff, GitError> {
        let untracked = side == DiffSide::Unstaged && !is_tracked(repository, path)?;
        let stdout = if untracked {
            let mut args = os_args(&[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-index",
                "--",
                "/dev/null",
            ]);
            args.push(path.as_os_str().to_owned());
            let captured = run_git(repository, args, None)?;
            match captured.status.code() {
                // Exit 1 with nothing printed is an error git reported on
                // stderr, as it does for an untracked folder that is a
                // repository of its own.
                Some(1) if captured.stdout.is_empty() && !captured.stderr.is_empty() => {
                    return Err(failed(&captured));
                }
                Some(0) | Some(1) => captured.stdout,
                _ => return Err(failed(&captured)),
            }
        } else {
            let mut args = os_args(&["diff", "--no-color", "--no-ext-diff", "--no-textconv"]);
            if side == DiffSide::Staged {
                args.push(OsString::from("--cached"));
            }
            args.push(OsString::from("--"));
            args.push(path.as_os_str().to_owned());
            ensure_success(run_git(repository, args, None)?)?
        };
        parse_diff(&stdout).map_err(unreadable)
    }

    /// `git add -A -- <paths>`. No paths is a no-op that runs nothing.
    fn stage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = os_args(&["add", "-A", "--"]);
        args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
        ensure_success(run_git(repository, args, None)?).map(|_| ())
    }

    /// `git restore --staged -- <paths>`; in a repository with no commit yet,
    /// `git rm --cached -r -q -- <paths>`, because there is no HEAD to restore
    /// from. No paths is a no-op that runs nothing.
    fn unstage(&self, repository: &Path, paths: &[PathBuf]) -> Result<(), GitError> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = if has_commit(repository)? {
            os_args(&["restore", "--staged", "--"])
        } else {
            os_args(&["rm", "--cached", "-r", "-q", "--"])
        };
        args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
        ensure_success(run_git(repository, args, None)?).map(|_| ())
    }

    /// `git commit -F -` with the message on stdin: the summary, and when the
    /// description is not empty, a blank line and the description. An empty or
    /// whitespace-only summary is refused with `Failed` before git runs.
    fn commit(&self, repository: &Path, message: &CommitMessage) -> Result<(), GitError> {
        let summary = message.summary.trim();
        if summary.is_empty() {
            return Err(GitError::Failed {
                exit_code: None,
                message: "the commit summary is empty".to_string(),
            });
        }
        let mut text = summary.to_string();
        let description = message.description.trim();
        if !description.is_empty() {
            text.push_str("\n\n");
            text.push_str(description);
        }
        text.push('\n');
        ensure_success(run_git_with(
            repository,
            ["commit", "-F", "-"],
            Some(text.as_bytes()),
            Hooks::On,
        )?)
        .map(|_| ())
    }
}
