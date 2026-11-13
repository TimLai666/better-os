//! Commits and the branch's history: commit with options, the log, one
//! commit's files and diff, and the edits GitHub Desktop offers on a
//! history (undo, reset, revert, cherry-pick, reorder, squash).

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use git_core::log::LOG_FORMAT;
use git_core::{Commit, CommitFile, FileDiff, parse_diff, parse_log, parse_name_status};

use crate::{
    CommitMessage, DiffOptions, GitError, Hooks, Run, credentials::shell_quote, git_path,
    has_commit, io_error, refused, unreadable,
};

/// How a commit is made.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitOptions {
    /// Replace the last commit (`--amend`).
    pub amend: bool,
    /// Skip the repository's `pre-commit` and `commit-msg` hooks
    /// (`--no-verify`).
    pub no_verify: bool,
    /// Commit even with nothing changed (`--allow-empty`).
    pub allow_empty: bool,
    /// `(name, email)` written as `Co-authored-by` trailers.
    pub co_authors: Vec<(String, String)>,
}

/// Refuses a blank summary before anything is touched.
pub(crate) fn check_message(message: &CommitMessage, _: &CommitOptions) -> Result<(), GitError> {
    if message.summary.trim().is_empty() {
        return Err(refused("the commit summary is empty"));
    }
    Ok(())
}

/// `git commit -F -` with the message on stdin, and the repository's hooks
/// on unless `no_verify` is set. A hook that fails leaves git's output
/// (the hook's own output included) as the error's message.
pub fn commit(
    repository: &Path,
    message: &CommitMessage,
    options: &CommitOptions,
) -> Result<(), GitError> {
    check_message(message, options)?;
    let mut run = Run::new(repository, ["commit", "-F", "-"]).hooks(Hooks::On);
    if options.amend {
        run = run.arg("--amend");
    }
    if options.no_verify {
        run = run.arg("--no-verify");
    }
    if options.allow_empty {
        run = run.arg("--allow-empty");
    }
    run.stdin(message.text(&options.co_authors).into_bytes())
        .run()
}

/// `git log -z` in `git_core`'s format, from `revision`, skipping `skip`
/// commits and reading at most `limit`. A repository with no commit has an
/// empty history.
pub fn log(
    repository: &Path,
    revision: &str,
    skip: usize,
    limit: usize,
) -> Result<Vec<Commit>, GitError> {
    if !has_commit(repository)? {
        return Ok(Vec::new());
    }
    let out = Run::new(repository, ["log", "-z", LOG_FORMAT])
        .arg(format!("--skip={skip}"))
        .arg(format!("--max-count={limit}"))
        .arg("--end-of-options")
        .arg(revision)
        .arg("--")
        .output()?;
    parse_log(&out).map_err(unreadable)
}

const DIFF_TREE: [&str; 7] = [
    "diff-tree",
    "-r",
    "--no-commit-id",
    "-M",
    "--root",
    "-m",
    "--first-parent",
];

/// The files one commit changed, against its first parent.
pub fn commit_files(repository: &Path, sha: &str) -> Result<Vec<CommitFile>, GitError> {
    let out = Run::new(repository, DIFF_TREE)
        .args(["-z", "--name-status", "--end-of-options", sha])
        .output()?;
    parse_name_status(&out).map_err(unreadable)
}

/// One file's diff in one commit, against its first parent.
pub fn commit_diff(
    repository: &Path,
    sha: &str,
    file: &CommitFile,
    options: DiffOptions,
) -> Result<FileDiff, GitError> {
    let mut paths = Vec::new();
    if let Some(original) = &file.original_path {
        paths.push(original.as_path());
    }
    paths.push(file.path.as_path());
    let out = Run::new(repository, DIFF_TREE)
        .args(["-p", "--no-color", "--no-ext-diff", "--no-textconv"])
        .args(options.args())
        .arg("--end-of-options")
        .arg(sha)
        .paths(paths)
        .output()?;
    parse_diff(&out).map_err(unreadable)
}

/// One step of a rewritten history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TodoAction {
    Pick,
    /// Fold into the commit before it, joining the messages.
    Squash,
}

/// An edit of the history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoryEdit {
    /// Takes back the last commit and keeps its changes in the working tree
    /// (`git reset --soft HEAD^`, or deleting HEAD's ref for the first
    /// commit).
    UndoLast,
    /// Moves the branch to a commit and keeps every later change in the
    /// working tree (`git reset --mixed`).
    ResetTo { sha: String },
    /// A new commit that undoes one (`git revert`, with `-m 1` for a merge).
    Revert { sha: String, merge: bool },
    /// Applies commits on top of the current branch, or of `onto` after
    /// switching to it.
    CherryPick {
        shas: Vec<String>,
        onto: Option<String>,
    },
    /// Rewrites the commits after `base` (all of them when `None`) as the
    /// todo list says, oldest first. A squashed group's message is
    /// `message` when given. `head` is the commit HEAD was on in the list
    /// the person dragged in: when HEAD has moved since, the list is stale
    /// and the rewrite is refused (`ChangedMeanwhile`) rather than dropping
    /// the commits that appeared.
    Rewrite {
        base: Option<String>,
        todo: Vec<(TodoAction, String)>,
        message: Option<String>,
        head: String,
    },
}

/// Runs one history edit. Conflicts stop it as `GitError::Conflicts`, with
/// git's operation left in progress for the conflicts list.
pub fn edit(repository: &Path, edit: &HistoryEdit) -> Result<(), GitError> {
    match edit {
        HistoryEdit::UndoLast => {
            let parent =
                Run::new(repository, ["rev-parse", "--verify", "-q", "HEAD^"]).capture()?;
            if parent.status.success() {
                Run::new(repository, ["reset", "--soft", "HEAD^"]).run()
            } else {
                // The first commit: the branch goes back to having none,
                // and its files stay staged.
                Run::new(repository, ["update-ref", "-d", "HEAD"]).run()
            }
        }
        HistoryEdit::ResetTo { sha } => {
            // `git reset` refuses `--end-of-options` (git 2.43), so the
            // revision is checked to be a commit id instead.
            check_sha(sha)?;
            Run::new(repository, ["reset", "--mixed", "-q", sha])
                .arg("--")
                .run()
        }
        HistoryEdit::Revert { sha, merge } => {
            let mut run = Run::new(repository, ["revert", "--no-edit"]).hooks(Hooks::On);
            if *merge {
                run = run.args(["-m", "1"]);
            }
            run.arg("--end-of-options").arg(sha).run()
        }
        HistoryEdit::CherryPick { shas, onto } => {
            if let Some(branch) = onto {
                Run::new(repository, ["switch", "--end-of-options", branch])
                    .hooks(Hooks::On)
                    .run()?;
            }
            Run::new(repository, ["cherry-pick", "--allow-empty"])
                .hooks(Hooks::On)
                .arg("--end-of-options")
                .args(shas)
                .run()
        }
        HistoryEdit::Rewrite {
            base,
            todo,
            message,
            head,
        } => rewrite(repository, base.as_deref(), todo, message.as_deref(), head),
    }
}

fn check_sha(sha: &str) -> Result<(), GitError> {
    if sha.len() < 4 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(refused("that is not a commit id"));
    }
    Ok(())
}

/// Where a squash's message waits while its rebase is stopped on
/// conflicts, so `--continue` commits it rather than git's joined messages.
pub const SQUASH_MESSAGE: &str = "better-git-squash-message";

/// Makes a new file in the git directory with `text`, refusing to follow or
/// replace anything already there (`create_new`): a symbolic link put at
/// that name cannot redirect the write.
fn create_private(path: &Path, text: &str) -> Result<(), GitError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(io_error)
}

/// Removes a file Better Git left in the git directory, never what a link
/// there points at.
pub(crate) fn remove_private(path: &Path) {
    if path.symlink_metadata().is_ok() {
        let _ = fs::remove_file(path);
    }
}

/// The editor a continued rebase uses: the squash message when one is
/// waiting, else git's prepared message unchanged.
pub(crate) fn continue_editor(repository: &Path) -> Result<String, GitError> {
    let message = git_path(repository, SQUASH_MESSAGE)?;
    Ok(match message.symlink_metadata() {
        Ok(meta) if meta.file_type().is_file() => format!("cp {}", shell_quote(&message)),
        _ => "true".to_string(),
    })
}

/// `git rebase -i` with the todo list written by Better Git. git starts its
/// sequence editor through the shell, so the editor is `cp` with the todo
/// file's path quoted, and the message editor for a squash is `cp` with the
/// message file; nothing the person typed reaches the shell. The rewrite
/// is refused when HEAD is no longer the commit the list was read at; and
/// `rebase.missingCommitsCheck=error` (set on every command) stops git if
/// the todo list leaves out a commit anyway.
fn rewrite(
    repository: &Path,
    base: Option<&str>,
    todo: &[(TodoAction, String)],
    message: Option<&str>,
    head: &str,
) -> Result<(), GitError> {
    if todo.is_empty() {
        return Ok(());
    }
    if todo[0].0 == TodoAction::Squash {
        return Err(refused("the first commit cannot be squashed into nothing"));
    }
    if resolve(repository, "HEAD")? != head {
        return Err(GitError::ChangedMeanwhile);
    }
    for sha in todo.iter().map(|(_, sha)| sha.as_str()).chain(base) {
        check_sha(sha)?;
    }
    let todo_path = git_path(repository, &format!("better-git-todo-{}", random_suffix()?))?;
    let message_path = git_path(repository, SQUASH_MESSAGE)?;
    let mut text = String::new();
    for (action, sha) in todo {
        let word = match action {
            TodoAction::Pick => "pick",
            TodoAction::Squash => "squash",
        };
        text.push_str(&format!("{word} {sha}\n"));
    }
    create_private(&todo_path, &text)?;
    remove_private(&message_path);
    let editor = match message {
        Some(message) => {
            let mut message = message.trim().to_string();
            message.push('\n');
            if let Err(error) = create_private(&message_path, &message) {
                remove_private(&todo_path);
                return Err(error);
            }
            format!("cp {}", shell_quote(&message_path))
        }
        None => "true".to_string(),
    };
    let mut run = Run::new(
        repository,
        ["rebase", "-i", "--autostash", "--no-autosquash"],
    )
    .hooks(Hooks::On)
    .env(
        "GIT_SEQUENCE_EDITOR",
        format!("cp {}", shell_quote(&todo_path)),
    )
    .env("GIT_EDITOR", editor);
    run = match base {
        Some(base) => run.arg("--end-of-options").arg(base),
        None => run.arg("--root"),
    };
    let result = run.run();
    if matches!(result, Err(GitError::Failed { .. }))
        && crate::conflicts::operation(repository)? == Some(git_core::Operation::Rebase)
    {
        // Stopped before any conflict (the todo list was refused, or a hook
        // failed): put the branch back as it was rather than leave a
        // half-done rewrite.
        let _ = Run::new(repository, ["rebase", "--abort"]).run();
    }
    remove_private(&todo_path);
    // Stopped on conflicts, the message stays for `--continue`.
    if !matches!(result, Err(GitError::Conflicts(_))) {
        remove_private(&message_path);
    }
    result
}

fn random_suffix() -> Result<String, GitError> {
    use std::io::Read;
    let mut bytes = [0u8; 8];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(io_error)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The full hash of a revision.
pub fn resolve(repository: &Path, revision: &str) -> Result<String, GitError> {
    let out = Run::new(
        repository,
        ["rev-parse", "--verify", "-q", "--end-of-options"],
    )
    .arg(OsString::from(format!("{revision}^{{commit}}")))
    .output()?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}
