//! Conflicts: which operation stopped, what its two sides are called, take
//! one side for a file, mark it resolved, continue, and abort.

use std::fs;
use std::path::Path;

use git_core::Operation;

use crate::history::{SQUASH_MESSAGE, continue_editor, remove_private};
use crate::{GitError, Hooks, Run, branches, git_path, is_tracked};

/// The operation the repository is in the middle of, from the files git
/// keeps in its directory.
pub fn operation(repository: &Path) -> Result<Option<Operation>, GitError> {
    let exists =
        |name: &str| -> Result<bool, GitError> { Ok(git_path(repository, name)?.exists()) };
    let operation = Operation::from_markers(
        exists("rebase-merge")? || exists("rebase-apply")?,
        exists("MERGE_HEAD")?,
        exists("CHERRY_PICK_HEAD")?,
        exists("REVERT_HEAD")?,
    );
    if operation.is_none() && exists("SQUASH_MSG")? {
        return Ok(Some(Operation::SquashMerge));
    }
    Ok(operation)
}

/// What git's two sides of a conflict are, in words the person knows:
/// `ours` is what `--ours` takes and `theirs` what `--theirs` takes. They
/// swap in a rebase, where "ours" is the branch being rebased onto and
/// "theirs" is the person's own commit being replayed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConflictSides {
    pub ours: String,
    pub theirs: String,
}

fn read_git_file(repository: &Path, name: &str) -> Option<String> {
    let path = git_path(repository, name).ok()?;
    let meta = path.symlink_metadata().ok()?;
    if !meta.file_type().is_file() || meta.len() > 64 * 1024 {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn short(repository: &Path, revision: &str) -> Option<String> {
    let out = Run::new(
        repository,
        ["rev-parse", "--short", "--verify", "-q", "--end-of-options"],
    )
    .arg(revision)
    .output()
    .ok()?;
    let text = String::from_utf8_lossy(&out).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The name of the branch a merge message names: `Merge branch 'topic'`,
/// `Merge remote-tracking branch 'origin/x'`.
fn merged_branch(message: &str) -> Option<String> {
    let first = message.lines().next()?;
    let start = first.find('\'')? + 1;
    let end = start + first[start..].find('\'')?;
    Some(first[start..end].to_string())
}

/// The sides of the conflict `operation` stopped on; `None` for no
/// operation. A stash restore's sides are the branch and the stash, which
/// the window names itself.
pub fn sides(repository: &Path, operation: Operation) -> ConflictSides {
    let branch = branches::current(repository)
        .ok()
        .flatten()
        .unwrap_or_else(|| "HEAD".to_string());
    let (ours, theirs) = match operation {
        Operation::Merge => (
            branch,
            read_git_file(repository, "MERGE_MSG")
                .and_then(|m| merged_branch(&m))
                .or_else(|| short(repository, "MERGE_HEAD")),
        ),
        Operation::SquashMerge => (
            branch,
            // "Squashed commit of the following:" then "commit <sha>".
            read_git_file(repository, "SQUASH_MSG").and_then(|m| {
                m.lines()
                    .find_map(|l| l.strip_prefix("commit "))
                    .map(|sha| sha.trim().chars().take(12).collect())
            }),
        ),
        Operation::Rebase => {
            let dir = if git_path(repository, "rebase-merge").is_ok_and(|p| p.exists()) {
                "rebase-merge"
            } else {
                "rebase-apply"
            };
            let rebased = read_git_file(repository, &format!("{dir}/head-name"))
                .map(|h| h.trim().trim_start_matches("refs/heads/").to_string())
                .filter(|h| !h.is_empty() && h != "detached HEAD");
            let onto = read_git_file(repository, &format!("{dir}/onto"))
                .and_then(|sha| short(repository, sha.trim()));
            (
                onto.unwrap_or_else(|| "HEAD".to_string()),
                rebased.or_else(|| short(repository, "REBASE_HEAD")),
            )
        }
        Operation::CherryPick => (branch, short(repository, "CHERRY_PICK_HEAD")),
        Operation::Revert => (
            branch,
            short(repository, "REVERT_HEAD").map(|sha| format!("revert {sha}")),
        ),
    };
    ConflictSides {
        ours,
        theirs: theirs.unwrap_or_else(|| "?".to_string()),
    }
}

/// Takes our side (`--ours`) or theirs for a conflicted file and marks it
/// resolved. When the chosen side deleted the file, the file is removed.
pub fn resolve_with(repository: &Path, path: &Path, ours: bool) -> Result<(), GitError> {
    let side = if ours { "--ours" } else { "--theirs" };
    let checkout = Run::new(repository, ["checkout", side])
        .paths([path])
        .capture()?;
    if checkout.status.success() {
        return Run::new(repository, ["add"]).paths([path]).run();
    }
    // That side has no version of the file: it deleted it.
    Run::new(repository, ["rm", "-q", "-f"]).paths([path]).run()
}

/// `git add` for a file the person edited, or `git rm --cached` when they
/// deleted it.
pub fn mark_resolved(repository: &Path, path: &Path) -> Result<(), GitError> {
    if repository.join(path).symlink_metadata().is_ok() {
        Run::new(repository, ["add"]).paths([path]).run()
    } else if is_tracked(repository, path)? {
        Run::new(repository, ["rm", "--cached", "-q"])
            .paths([path])
            .run()
    } else {
        Ok(())
    }
}

/// Continues with git's prepared message: a merge (squashed or not) becomes
/// `git commit --no-edit`, which takes `MERGE_MSG` or `SQUASH_MSG`; a
/// rebase that stopped in a squash commits the message the person wrote;
/// the others are `git <operation> --continue`. The repository's hooks run:
/// these make commits.
pub fn continue_operation(repository: &Path, operation: Operation) -> Result<(), GitError> {
    let (run, editor) = match operation {
        Operation::Merge | Operation::SquashMerge => (
            Run::new(repository, ["commit", "--no-edit"]),
            "true".to_string(),
        ),
        Operation::Rebase => (
            Run::new(repository, ["rebase", "--continue"]),
            continue_editor(repository)?,
        ),
        other => (
            Run::new(repository, [other.command(), "--continue"]),
            "true".to_string(),
        ),
    };
    let result = run.hooks(Hooks::On).env("GIT_EDITOR", editor).run();
    if operation == Operation::Rebase && self::operation(repository)? != Some(Operation::Rebase) {
        remove_private(&git_path(repository, SQUASH_MESSAGE)?);
    }
    result
}

/// Undoes the operation. A squash merge has no `--abort`; `git reset
/// --merge` puts back the files it touched and keeps the person's other
/// changes, and `SQUASH_MSG` goes with it.
pub fn abort_operation(repository: &Path, operation: Operation) -> Result<(), GitError> {
    match operation {
        Operation::SquashMerge => {
            Run::new(repository, ["reset", "--merge"]).run()?;
            remove_private(&git_path(repository, "SQUASH_MSG")?);
            Ok(())
        }
        Operation::Rebase => {
            let result = Run::new(repository, ["rebase", "--abort"]).run();
            remove_private(&git_path(repository, SQUASH_MESSAGE)?);
            result
        }
        other => Run::new(repository, [other.command(), "--abort"]).run(),
    }
}
