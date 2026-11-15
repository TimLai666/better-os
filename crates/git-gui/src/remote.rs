//! The one button GitHub Desktop puts beside the branch: publish, push,
//! pull, or fetch, with the counts; and what a refused network operation
//! means in plain words.

use git_core::{Refusal, RepositoryStatus};

use crate::i18n::{Copy, fill};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncAction {
    /// No remote at all: publish the repository to GitHub.
    PublishRepository,
    /// The branch has no upstream yet.
    PublishBranch,
    /// Behind: pull first (ahead may also be non-zero).
    Pull {
        behind: u32,
        ahead: u32,
    },
    Push {
        ahead: u32,
    },
    Fetch,
    /// Detached HEAD: nothing to sync.
    None,
}

pub fn sync_action(status: &RepositoryStatus, has_remote: bool, detached: bool) -> SyncAction {
    if !has_remote {
        return SyncAction::PublishRepository;
    }
    if detached {
        return SyncAction::None;
    }
    if status.upstream.is_none() {
        return SyncAction::PublishBranch;
    }
    match status.ahead_behind {
        Some((ahead, behind)) if behind > 0 => SyncAction::Pull { behind, ahead },
        Some((ahead, 0)) if ahead > 0 => SyncAction::Push { ahead },
        _ => SyncAction::Fetch,
    }
}

/// One action the sync button's arrow menu can offer (ticket 123).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    Fetch,
    /// `behind` and `ahead` are the counts the row names.
    Pull {
        behind: u32,
        ahead: u32,
    },
    Push {
        ahead: u32,
    },
    /// Only a branch that has diverged can be forced, and it replaces what
    /// the person last saw on the remote.
    ForcePush {
        ahead: u32,
        behind: u32,
    },
    PublishBranch,
    PublishRepository,
}

/// Why a menu row cannot be chosen now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuWhy {
    NoRemote,
    Detached,
    NoUpstream,
    /// Level with the upstream: nothing to pull or push.
    UpToDate,
    /// Behind only: pulling first is the way forward.
    Behind,
    /// Ahead only: nothing to pull.
    Ahead,
    /// Both sides have commits: a plain push would be refused.
    Diverged,
}

/// A row of the sync button's arrow menu: what it does, and why it cannot be
/// chosen when it cannot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuRow {
    pub action: MenuAction,
    pub why: Option<MenuWhy>,
}

impl MenuRow {
    pub fn enabled(&self) -> bool {
        self.why.is_none()
    }
}

/// The menu behind the sync button's arrow: Fetch always, Pull, Push, Force
/// push only when the branch has diverged, and Publish when the branch has no
/// upstream. A row that does not apply is shown disabled with its reason, so
/// the menu says why the thing a person expected is not there.
pub fn sync_menu(status: &RepositoryStatus, has_remote: bool, detached: bool) -> Vec<MenuRow> {
    let (ahead, behind) = status.ahead_behind.unwrap_or((0, 0));
    let row = |action, why| MenuRow { action, why };
    if !has_remote {
        return vec![
            row(MenuAction::Fetch, Some(MenuWhy::NoRemote)),
            row(MenuAction::Pull { behind, ahead }, Some(MenuWhy::NoRemote)),
            row(MenuAction::Push { ahead }, Some(MenuWhy::NoRemote)),
            row(MenuAction::PublishRepository, None),
        ];
    }
    if detached {
        return vec![
            row(MenuAction::Fetch, None),
            row(MenuAction::Pull { behind, ahead }, Some(MenuWhy::Detached)),
            row(MenuAction::Push { ahead }, Some(MenuWhy::Detached)),
        ];
    }
    if status.upstream.is_none() {
        return vec![
            row(MenuAction::Fetch, None),
            row(
                MenuAction::Pull { behind, ahead },
                Some(MenuWhy::NoUpstream),
            ),
            row(MenuAction::Push { ahead }, Some(MenuWhy::NoUpstream)),
            row(MenuAction::PublishBranch, None),
        ];
    }
    let diverged = ahead > 0 && behind > 0;
    let mut rows = vec![
        row(MenuAction::Fetch, None),
        row(
            MenuAction::Pull { behind, ahead },
            match (behind, ahead) {
                (0, 0) => Some(MenuWhy::UpToDate),
                (0, _) => Some(MenuWhy::Ahead),
                _ => None,
            },
        ),
        row(
            MenuAction::Push { ahead },
            match (ahead, behind) {
                (0, _) => Some(if behind > 0 {
                    MenuWhy::Behind
                } else {
                    MenuWhy::UpToDate
                }),
                (_, b) if b > 0 => Some(MenuWhy::Diverged),
                _ => None,
            },
        ),
    ];
    if diverged {
        rows.push(row(MenuAction::ForcePush { ahead, behind }, None));
    }
    rows
}

/// What a menu row says: what it does and how many commits it moves, and,
/// when it cannot be chosen, why. `upstream` is the branch's upstream
/// (`origin/main`), `remote` the remote fetch reads.
pub fn menu_label(row: &MenuRow, upstream: &str, remote: &str, c: &Copy) -> String {
    let n = |n: u32| n.to_string();
    let item = match row.action {
        MenuAction::Fetch => fill(c.fetch_from, &[("remote", remote)]),
        MenuAction::Pull { behind: 0, .. } => fill(c.pull_from, &[("upstream", upstream)]),
        MenuAction::Pull { behind: 1, .. } => fill(c.pull_commit, &[("upstream", upstream)]),
        MenuAction::Pull { behind, .. } => {
            fill(c.pull_commits, &[("n", &n(behind)), ("upstream", upstream)])
        }
        MenuAction::Push { ahead: 0 } => fill(c.push_to, &[("upstream", upstream)]),
        MenuAction::Push { ahead: 1 } => fill(c.push_commit, &[("upstream", upstream)]),
        MenuAction::Push { ahead } => {
            fill(c.push_commits, &[("n", &n(ahead)), ("upstream", upstream)])
        }
        MenuAction::ForcePush { ahead: 1, .. } => {
            fill(c.force_push_commit, &[("upstream", upstream)])
        }
        MenuAction::ForcePush { ahead, .. } => fill(
            c.force_push_commits,
            &[("n", &n(ahead)), ("upstream", upstream)],
        ),
        MenuAction::PublishBranch => c.publish_branch.to_string(),
        MenuAction::PublishRepository => c.publish_repository.to_string(),
    };
    let Some(why) = row.why else {
        return item;
    };
    let why = match why {
        MenuWhy::NoRemote => c.why_no_remote.to_string(),
        MenuWhy::Detached => c.why_detached.to_string(),
        MenuWhy::NoUpstream => c.why_no_upstream.to_string(),
        MenuWhy::UpToDate => fill(c.why_up_to_date, &[("upstream", upstream)]),
        MenuWhy::Behind => fill(c.why_behind, &[("upstream", upstream)]),
        MenuWhy::Ahead => c.why_ahead.to_string(),
        MenuWhy::Diverged => c.why_diverged.to_string(),
    };
    fill(c.menu_unavailable, &[("item", &item), ("why", &why)])
}

/// What to do next after a refusal, as the info bar's action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NextStep {
    Pull,
    ForcePush,
    SignIn,
    Fork,
    StashAndRetry,
    ResolveConflicts,
    PublishBranch,
    None,
}

/// The next step for a refusal of a push (`push`) or of a pull or fetch.
pub fn next_step(refusal: Refusal, push: bool, signed_in: bool, hosted: bool) -> NextStep {
    match refusal {
        Refusal::NotFastForward | Refusal::Diverged if push => NextStep::Pull,
        Refusal::StaleLease => NextStep::Pull,
        Refusal::Authentication if !signed_in && hosted => NextStep::SignIn,
        Refusal::PermissionDenied if hosted && signed_in && push => NextStep::Fork,
        Refusal::PermissionDenied if hosted && !signed_in => NextStep::SignIn,
        Refusal::LocalChangesWouldBeOverwritten => NextStep::StashAndRetry,
        Refusal::Conflicts => NextStep::ResolveConflicts,
        Refusal::NoUpstream => NextStep::PublishBranch,
        _ => NextStep::None,
    }
}

/// How often the background fetch runs, as GitHub Desktop does.
pub const BACKGROUND_FETCH_SECONDS: u64 = 5 * 60;
