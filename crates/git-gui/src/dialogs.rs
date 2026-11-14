//! Which dialog is open, what it holds besides its text fields, and the
//! rules for when its primary button is offered. The window owns up to
//! three text fields for a dialog; their values are handed in as `texts`.

use std::path::PathBuf;

use git_core::{Branch, Tag, check_branch_name, check_tag_name, sanitize_ref_name};

/// What a branch chooser is choosing for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BranchPurpose {
    Merge,
    SquashMerge,
    Rebase,
    Compare,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloneTab {
    GitHub,
    Url,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Dialog {
    /// Before the first read of a path.
    Trust {
        path: PathBuf,
        programs: Vec<String>,
    },
    /// texts: [folder]
    AddLocal,
    /// texts: [url or search, parent folder]
    Clone {
        tab: CloneTab,
        selected: Option<String>,
    },
    /// texts: [name, parent folder, description]
    NewRepository {
        gitignore: Option<usize>,
        license: Option<usize>,
        readme: bool,
    },
    /// texts: [name, description]
    Publish {
        private: bool,
        organization: Option<usize>,
    },
    /// texts: [alias]
    Alias {
        path: PathBuf,
    },
    RemoveRepository {
        path: PathBuf,
    },
    /// texts: [name]
    NewBranch {
        start: Option<String>,
        start_label: Option<String>,
    },
    /// texts: [new name]
    RenameBranch {
        from: String,
    },
    DeleteBranch {
        name: String,
        has_remote: bool,
        delete_remote: bool,
    },
    SwitchWithChanges {
        target: String,
        bring: bool,
    },
    /// texts: [search]
    PickBranch {
        purpose: BranchPurpose,
        selected: Option<String>,
    },
    DiscardFiles {
        paths: Vec<PathBuf>,
    },
    DiscardLines {
        path: PathBuf,
    },
    UndoCommit,
    ResetTo {
        sha: String,
        summary: String,
    },
    /// `expected`: the commit the remote branch was on when asked.
    ForcePush {
        expected: Option<String>,
    },
    /// texts: [name]
    CreateTag {
        sha: String,
    },
    DeleteTag {
        name: String,
    },
    /// texts: [message]
    Squash {
        from: usize,
        to: usize,
    },
    /// texts: [folder, branch]
    AddWorktree {
        create: bool,
    },
    /// texts: [new folder]
    RenameWorktree {
        path: PathBuf,
    },
    RemoveWorktree {
        path: PathBuf,
        force: bool,
    },
    /// texts: [reason, which may be blank]
    LockWorktree {
        path: PathBuf,
    },
    /// `git worktree prune`: every worktree it would clear from the list
    /// (Git cannot clear just one).
    ClearWorktrees {
        paths: Vec<PathBuf>,
    },
    /// texts: [name, url]
    Remotes {
        editing: Option<String>,
    },
    /// texts: [title, body]
    CreatePull {
        base: Option<String>,
        draft: bool,
    },
    /// texts: [title, body]
    CreateIssue,
    /// texts: [Enterprise host, its OAuth App's client ID (optional)]
    SignIn {
        enterprise: bool,
    },
    MarkResolved {
        path: PathBuf,
    },
    Fork,
    DropStash {
        name: String,
    },
    /// Git has no name or email yet (`identity`). texts: [name, email]
    Identity,
}

/// Why the primary button is off, for the caption under the fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Invalid {
    Empty,
    BranchExists,
    BranchReserved,
    TagExists,
    TagTooLong,
    NotAbsolute,
    NoSelection,
    /// The new repository's folder is a repository already.
    AlreadyRepository,
    /// The new repository's folder already holds files.
    NotEmpty,
    /// A file is where the new repository's folder would go.
    NotAFolder,
    /// The email field holds no address.
    NotAnEmail,
}

/// Checks a dialog's fields. `branches` and `tags` are the repository's.
pub fn validate(
    dialog: &Dialog,
    texts: &[String],
    branches: &[Branch],
    tags: &[Tag],
) -> Option<Invalid> {
    let text = |i: usize| texts.get(i).map(|t| t.trim()).unwrap_or("");
    let absolute = |i: usize| -> Option<Invalid> {
        let t = text(i);
        if t.is_empty() {
            Some(Invalid::Empty)
        } else if !std::path::Path::new(t).is_absolute() {
            Some(Invalid::NotAbsolute)
        } else {
            None
        }
    };
    let branch_name = |i: usize| -> Option<Invalid> {
        match check_branch_name(&sanitize_ref_name(text(i)), branches) {
            Some(git_core::NameProblem::Empty) => Some(Invalid::Empty),
            Some(git_core::NameProblem::Exists) => Some(Invalid::BranchExists),
            Some(git_core::NameProblem::Reserved) => Some(Invalid::BranchReserved),
            None => None,
        }
    };
    match dialog {
        Dialog::AddLocal => absolute(0),
        Dialog::Clone { tab, selected } => match tab {
            CloneTab::Url if text(0).is_empty() => Some(Invalid::Empty),
            CloneTab::GitHub if selected.is_none() => Some(Invalid::NoSelection),
            _ => absolute(1),
        },
        Dialog::NewRepository { .. } => {
            if crate::publish::sanitize_repository_name(text(0)).is_empty() {
                Some(Invalid::Empty)
            } else {
                absolute(1)
            }
        }
        Dialog::Publish { .. } => crate::publish::sanitize_repository_name(text(0))
            .is_empty()
            .then_some(Invalid::Empty),
        Dialog::NewBranch { .. } | Dialog::RenameBranch { .. } => branch_name(0),
        Dialog::AddWorktree { create } => absolute(0).or_else(|| {
            if *create {
                branch_name(1)
            } else {
                text(1).is_empty().then_some(Invalid::Empty)
            }
        }),
        Dialog::RenameWorktree { .. } => absolute(0),
        Dialog::CreateTag { .. } => match check_tag_name(&sanitize_ref_name(text(0)), tags) {
            Some(git_core::tag::TagProblem::Empty) => Some(Invalid::Empty),
            Some(git_core::tag::TagProblem::Exists) => Some(Invalid::TagExists),
            Some(git_core::tag::TagProblem::TooLong) => Some(Invalid::TagTooLong),
            None => None,
        },
        Dialog::Squash { .. } => text(2).is_empty().then_some(Invalid::Empty),
        Dialog::CreatePull { .. } | Dialog::CreateIssue => {
            text(0).is_empty().then_some(Invalid::Empty)
        }
        Dialog::PickBranch { selected, .. } => selected.is_none().then_some(Invalid::NoSelection),
        Dialog::Remotes { .. } => None,
        Dialog::Identity => match crate::identity::check(text(0), text(1)) {
            Some(crate::identity::Problem::NotAnEmail) => Some(Invalid::NotAnEmail),
            Some(_) => Some(Invalid::Empty),
            None => None,
        },
        Dialog::SignIn { enterprise: true } => github_api::Host::enterprise(text(0))
            .is_none()
            .then_some(Invalid::Empty),
        _ => None,
    }
}

/// How many text fields a dialog uses.
pub fn field_count(dialog: &Dialog) -> usize {
    match dialog {
        Dialog::AddLocal
        | Dialog::Alias { .. }
        | Dialog::NewBranch { .. }
        | Dialog::RenameBranch { .. }
        | Dialog::PickBranch { .. }
        | Dialog::CreateTag { .. }
        | Dialog::RenameWorktree { .. }
        | Dialog::LockWorktree { .. }
        | Dialog::SignIn { enterprise: false } => 1,
        Dialog::SignIn { enterprise: true } => 2,
        Dialog::Clone { .. }
        | Dialog::Publish { .. }
        | Dialog::AddWorktree { .. }
        | Dialog::Remotes { .. }
        | Dialog::CreatePull { .. }
        | Dialog::CreateIssue
        | Dialog::Identity => 2,
        Dialog::NewRepository { .. } | Dialog::Squash { .. } => 3,
        _ => 0,
    }
}
