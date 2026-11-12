//! Conflicts: which operation stopped, and whether a file still holds
//! git's markers.

/// The operation a repository is in the middle of, read from the files git
/// leaves in its directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    /// `MERGE_HEAD` exists.
    Merge,
    /// `rebase-merge/` or `rebase-apply/` exists.
    Rebase,
    /// `CHERRY_PICK_HEAD` exists.
    CherryPick,
    /// `REVERT_HEAD` exists.
    Revert,
    /// `git merge --squash` stopped (or its commit did not happen):
    /// `SQUASH_MSG` exists and none of the others. git keeps no
    /// `MERGE_HEAD` for it, so `git merge --continue` and `--abort` do not
    /// know it; it is finished with `git commit` and undone with `git reset
    /// --merge`.
    SquashMerge,
}

impl Operation {
    /// Picks the operation from which of git's marker files exist, in the
    /// order git itself checks them: a rebase can stop on a cherry-pick of
    /// its own, and is still a rebase.
    pub fn from_markers(
        rebase: bool,
        merge: bool,
        cherry_pick: bool,
        revert: bool,
    ) -> Option<Operation> {
        if rebase {
            Some(Operation::Rebase)
        } else if merge {
            Some(Operation::Merge)
        } else if cherry_pick {
            Some(Operation::CherryPick)
        } else if revert {
            Some(Operation::Revert)
        } else {
            None
        }
    }

    /// The git subcommand that continues or aborts it.
    pub fn command(self) -> &'static str {
        match self {
            Operation::Merge | Operation::SquashMerge => "merge",
            Operation::Rebase => "rebase",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
        }
    }
}

/// How many conflict regions a file's text still has: lines starting with
/// `<<<<<<< ` that are followed by `=======` and `>>>>>>> `.
///
/// Expected results: a file with no markers is 0; a region missing its end
/// is not counted; markers must be at the start of a line.
pub fn count_conflict_markers(content: &[u8]) -> usize {
    let mut count = 0;
    let mut state = 0u8;
    for line in content.split(|&b| b == b'\n') {
        match state {
            0 if line.starts_with(b"<<<<<<<") => state = 1,
            1 if line.starts_with(b"=======") => state = 2,
            2 if line.starts_with(b">>>>>>>") => {
                count += 1;
                state = 0;
            }
            _ => {}
        }
    }
    count
}
