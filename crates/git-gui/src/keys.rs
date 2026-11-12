//! GitHub Desktop's keyboard shortcuts, with Ctrl where macOS has Cmd
//! (desktop/desktop `app/src/main-process/menu/build-default-menu.ts`, read
//! 2026-11-12). The window looks every key press up here.

/// Something a shortcut does.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Action {
    Settings,
    NewRepository,
    AddLocalRepository,
    CloneRepository,
    ShowChanges,
    ShowHistory,
    RepositoryList,
    BranchList,
    WorktreeList,
    GoToSummary,
    ToggleStash,
    ToggleFilter,
    Refresh,
    Push,
    Pull,
    Fetch,
    RemoveRepository,
    ViewOnGitHub,
    OpenInTerminal,
    ShowInFiles,
    OpenInEditor,
    CreateIssue,
    NewWorktree,
    NewBranch,
    RenameBranch,
    DeleteBranch,
    DiscardAll,
    StashAll,
    UpdateFromDefault,
    CompareToBranch,
    MergeIntoCurrent,
    SquashAndMerge,
    RebaseCurrent,
    CompareOnGitHub,
    ViewBranchOnGitHub,
    PullRequest,
    Commit,
}

/// The table: action, then the keys as `ctrl-shift-p` (GPUI's notation),
/// then as people read them.
pub const SHORTCUTS: &[(Action, &str, &str)] = &[
    (Action::Settings, "ctrl-,", "Ctrl+,"),
    (Action::NewRepository, "ctrl-n", "Ctrl+N"),
    (Action::AddLocalRepository, "ctrl-o", "Ctrl+O"),
    (Action::CloneRepository, "ctrl-shift-o", "Ctrl+Shift+O"),
    (Action::ShowChanges, "ctrl-1", "Ctrl+1"),
    (Action::ShowHistory, "ctrl-2", "Ctrl+2"),
    (Action::RepositoryList, "ctrl-t", "Ctrl+T"),
    (Action::BranchList, "ctrl-b", "Ctrl+B"),
    (Action::WorktreeList, "ctrl-alt-w", "Ctrl+Alt+W"),
    (Action::GoToSummary, "ctrl-g", "Ctrl+G"),
    (Action::ToggleStash, "ctrl-h", "Ctrl+H"),
    (Action::ToggleFilter, "ctrl-l", "Ctrl+L"),
    (Action::Refresh, "ctrl-alt-r", "Ctrl+Alt+R"),
    (Action::Push, "ctrl-p", "Ctrl+P"),
    (Action::Pull, "ctrl-shift-p", "Ctrl+Shift+P"),
    (Action::Fetch, "ctrl-shift-t", "Ctrl+Shift+T"),
    (Action::RemoveRepository, "ctrl-backspace", "Ctrl+Backspace"),
    (Action::ViewOnGitHub, "ctrl-shift-g", "Ctrl+Shift+G"),
    (Action::OpenInTerminal, "ctrl-`", "Ctrl+`"),
    (Action::ShowInFiles, "ctrl-shift-f", "Ctrl+Shift+F"),
    (Action::OpenInEditor, "ctrl-shift-a", "Ctrl+Shift+A"),
    (Action::CreateIssue, "ctrl-i", "Ctrl+I"),
    (Action::NewWorktree, "ctrl-shift-w", "Ctrl+Shift+W"),
    (Action::NewBranch, "ctrl-shift-n", "Ctrl+Shift+N"),
    (Action::RenameBranch, "ctrl-shift-r", "Ctrl+Shift+R"),
    (Action::DeleteBranch, "ctrl-shift-d", "Ctrl+Shift+D"),
    (
        Action::DiscardAll,
        "ctrl-shift-backspace",
        "Ctrl+Shift+Backspace",
    ),
    (Action::StashAll, "ctrl-shift-s", "Ctrl+Shift+S"),
    (Action::UpdateFromDefault, "ctrl-shift-u", "Ctrl+Shift+U"),
    (Action::CompareToBranch, "ctrl-shift-b", "Ctrl+Shift+B"),
    (Action::MergeIntoCurrent, "ctrl-shift-m", "Ctrl+Shift+M"),
    (Action::SquashAndMerge, "ctrl-shift-h", "Ctrl+Shift+H"),
    (Action::RebaseCurrent, "ctrl-shift-e", "Ctrl+Shift+E"),
    (Action::CompareOnGitHub, "ctrl-shift-c", "Ctrl+Shift+C"),
    (Action::ViewBranchOnGitHub, "ctrl-alt-b", "Ctrl+Alt+B"),
    (Action::PullRequest, "ctrl-r", "Ctrl+R"),
    (Action::Commit, "ctrl-enter", "Ctrl+Enter"),
];

/// The action for a key press. `key` is GPUI's key name (`p`, `backspace`,
/// `` ` ``, `1`); letters match in either case.
pub fn action_for(key: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Action> {
    if !ctrl {
        return None;
    }
    let mut chord = String::from("ctrl-");
    if alt {
        chord.push_str("alt-");
    }
    if shift {
        chord.push_str("shift-");
    }
    chord.push_str(&key.to_ascii_lowercase());
    SHORTCUTS
        .iter()
        .find(|(_, keys, _)| *keys == chord)
        .map(|(action, _, _)| *action)
}

/// How a shortcut reads in a menu.
pub fn label(action: Action) -> &'static str {
    SHORTCUTS
        .iter()
        .find(|(a, _, _)| *a == action)
        .map(|(_, _, label)| *label)
        .unwrap_or("")
}
