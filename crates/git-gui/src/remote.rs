//! The one button GitHub Desktop puts beside the branch: publish, push,
//! pull, or fetch, with the counts; and what a refused network operation
//! means in plain words.

use git_core::{Refusal, RepositoryStatus};

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
