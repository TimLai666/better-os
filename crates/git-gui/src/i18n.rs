//! Every word Better Git shows, in zh-TW and English.
//!
//! The pattern is the other windows': one [`Copy`] struct, one constant per
//! language, so a string added to one and forgotten in the other does not
//! compile. Choosing a language is `--lang`, or the session's `LANG` the way
//! Better Files reads it. A macro writes both constants from one table, so a
//! string cannot exist in one language only.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    System,
    EnUs,
    ZhTw,
}

impl Locale {
    /// Resolves `System` against `LANG` once, so the rest of the window never
    /// reads the environment.
    pub fn resolved(self) -> Self {
        match self {
            Self::System => Self::from_lang(&std::env::var("LANG").unwrap_or_default()),
            locale => locale,
        }
    }

    /// The language a `LANG` value names: Traditional Chinese for `zh_TW` and
    /// any `Hant` locale, English for everything else.
    pub fn from_lang(lang: &str) -> Self {
        let lang = lang.to_ascii_lowercase();
        if lang.contains("zh_tw") || lang.contains("zh-tw") || lang.contains("hant") {
            Self::ZhTw
        } else {
            Self::EnUs
        }
    }

    /// A `--lang` value: `zh-TW` or `en-US`, in either case and with `_` or
    /// `-`; `system` for the environment's. Anything else is `None`.
    pub fn from_argument(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().replace('_', "-").as_str() {
            "zh-tw" => Some(Self::ZhTw),
            "en-us" | "en" => Some(Self::EnUs),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

use git_core::Refusal;
use git_platform::GitError;
use github_api::ApiError;

use crate::github::SignInProblem;
use crate::session::{Notice, OpKind, Problem};

macro_rules! strings {
    ($($name:ident: $en:expr, $zh:expr;)*) => {
        /// Every fixed string, one field each.
        pub struct Copy {
            $(pub $name: &'static str,)*
        }

        pub static EN_US: Copy = Copy { $($name: $en,)* };
        pub static ZH_TW: Copy = Copy { $($name: $zh,)* };

        impl Copy {
            /// Every string, for the test that no field is empty.
            pub fn all(&self) -> Vec<&'static str> {
                vec![$(self.$name,)*]
            }
        }
    };
}

strings! {
    app_name: "Better Git", "Better Git";
    // Toolbar
    current_repository: "Current repository", "目前的儲存庫";
    current_branch: "Current branch", "目前的分支";
    no_repository: "No repository", "沒有儲存庫";
    detached: "Detached HEAD", "分離的 HEAD";
    fetch_origin: "Fetch origin", "從 origin 擷取";
    fetching: "Fetching…", "正在擷取…";
    pull_origin: "Pull origin", "從 origin 拉取";
    pulling: "Pulling…", "正在拉取…";
    push_origin: "Push origin", "推送到 origin";
    pushing: "Pushing…", "正在推送…";
    publish_branch: "Publish branch", "發佈分支";
    publish_repository: "Publish repository", "發佈儲存庫";
    last_fetched: "Last fetched {when}", "上次擷取：{when}";
    never_fetched: "Never fetched", "還沒擷取過";
    just_now: "just now", "剛剛";
    minutes_ago: "{n} minutes ago", "{n} 分鐘前";
    hours_ago: "{n} hours ago", "{n} 小時前";
    menu: "Menu", "選單";
    settings: "Settings", "設定";
    // Menu groups
    menu_repository: "Repository", "儲存庫";
    menu_branch: "Branch", "分支";
    menu_view: "View", "檢視";
    menu_file: "File", "檔案";
    new_repository: "New repository…", "新增儲存庫…";
    add_local: "Add local repository…", "加入本機儲存庫…";
    clone_repository: "Clone repository…", "複製儲存庫…";
    remove_repository: "Remove…", "移除…";
    repository_settings: "Remotes…", "遠端設定…";
    view_on_github: "View on GitHub", "在 GitHub 上檢視";
    open_in_terminal: "Open in terminal", "在終端機開啟";
    show_in_files: "Show in Better Files", "在 Better Files 顯示";
    open_in_editor: "Open in external editor", "在外部編輯器開啟";
    create_issue: "Create issue on GitHub…", "在 GitHub 建立議題…";
    new_branch: "New branch…", "新增分支…";
    rename_branch: "Rename…", "重新命名…";
    delete_branch: "Delete…", "刪除…";
    discard_all: "Discard all changes…", "捨棄所有變更…";
    stash_all: "Stash all changes", "收起所有變更";
    update_from: "Update from {branch}", "從 {branch} 更新";
    compare_to_branch: "Compare to branch…", "與分支比較…";
    merge_into_current: "Merge into current branch…", "合併到目前的分支…";
    squash_merge: "Squash and merge into current branch…", "壓縮並合併到目前的分支…";
    rebase_current: "Rebase current branch…", "重定基底目前的分支…";
    compare_on_github: "Compare on GitHub", "在 GitHub 上比較";
    view_branch_on_github: "View branch on GitHub", "在 GitHub 上檢視分支";
    create_pull: "Create pull request…", "建立拉取要求…";
    view_pull: "View pull request on GitHub", "在 GitHub 上檢視拉取要求";
    worktrees: "Worktrees…", "工作樹…";
    new_worktree: "New worktree…", "新增工作樹…";
    show_changes: "Changes", "變更";
    show_history: "History", "歷程記錄";
    repository_list: "Repository list", "儲存庫清單";
    branch_list: "Branches list", "分支清單";
    go_to_summary: "Go to summary", "移到摘要";
    show_stash: "Show stashed changes", "顯示收起的變更";
    toggle_filter: "Filter changes", "篩選變更";
    refresh: "Refresh", "重新整理";
    // Empty and welcome
    welcome_title: "Let's get started!", "開始使用吧！";
    welcome_body: "Add a repository to Better Git to start collaborating.", "把儲存庫加入 Better Git，開始協作。";
    welcome_signed_out: "Sign in to GitHub to clone your repositories and see pull requests.", "登入 GitHub 就能複製你的儲存庫、查看拉取要求。";
    recent: "Recent", "最近使用";
    all_repositories: "Repositories", "儲存庫";
    filter_repositories: "Filter", "篩選";
    add: "Add", "加入";
    missing: "Can't be found", "找不到";
    missing_body: "This folder was moved or deleted.", "這個資料夾已被移動或刪除。";
    locate: "Locate…", "尋找…";
    alias: "Alias…", "別名…";
    remove_alias: "Remove alias", "移除別名";
    // Changes
    changes_count: "{n} changed files", "{n} 個檔案已變更";
    one_change: "1 changed file", "1 個檔案已變更";
    no_changes_title: "No local changes", "沒有本機變更";
    no_changes_body: "There are no uncommitted changes in this repository.", "這個儲存庫沒有尚未提交的變更。";
    filter_changes: "Filter changed files", "篩選已變更的檔案";
    stashed_changes: "Stashed changes", "收起的變更";
    stash_for_branch: "Stashed changes for {branch}", "{branch} 收起的變更";
    restore: "Restore", "還原";
    discard: "Discard", "捨棄";
    discard_changes: "Discard changes…", "捨棄變更…";
    discard_selected: "Discard {n} selected files…", "捨棄 {n} 個選取的檔案…";
    discard_lines: "Discard selected lines…", "捨棄選取的行…";
    ignore_file: "Ignore file", "忽略這個檔案";
    ignore_extension: "Ignore all {ext} files", "忽略所有 {ext} 檔案";
    copy_path: "Copy file path", "複製檔案路徑";
    copy_relative: "Copy relative file path", "複製相對檔案路徑";
    show_in_folder: "Show in Better Files", "在 Better Files 顯示";
    open_file_in_editor: "Open in external editor", "在外部編輯器開啟";
    // Diff
    select_file: "Select a file to see its changes", "選一個檔案來查看它的變更";
    binary_file: "This binary file has changed.", "這個二進位檔案已變更。";
    no_content_changes: "No content changes.", "內容沒有變更。";
    large_diff: "This diff is large.", "這個差異很大。";
    large_diff_body: "Showing it may make Better Git slow.", "顯示它可能讓 Better Git 變慢。";
    show_diff: "Show diff", "顯示差異";
    unified: "Unified", "單欄";
    split: "Split", "並排";
    hide_whitespace: "Hide whitespace changes", "隱藏空白變更";
    expand_file: "Expand whole file", "展開整個檔案";
    whitespace_hidden_note: "Lines can't be chosen while whitespace changes are hidden.", "隱藏空白變更時無法選擇個別行。";
    image_before: "Before", "之前";
    image_after: "After", "之後";
    image_none: "No image", "沒有圖片";
    conflicted_diff: "This file has conflicts. Resolve them in the conflicts list.", "這個檔案有衝突，請在衝突清單裡解決。";
    // Commit
    summary_placeholder: "Summary (required)", "摘要（必填）";
    description_placeholder: "Description", "描述";
    co_authors_placeholder: "Name <email>, one per line", "姓名 <email>，一行一位";
    add_co_authors: "Add co-authors", "加入共同作者";
    commit_to: "Commit to {branch}", "提交到 {branch}";
    commit: "Commit", "提交";
    amend: "Amend last commit", "修改上一個提交";
    amending: "Amending the last commit", "正在修改上一個提交";
    bypass_hooks: "Bypass commit hooks", "略過提交 hook";
    allow_empty: "Allow an empty commit", "允許空提交";
    push_after: "Push after committing", "提交後推送";
    commit_options: "Commit options", "提交選項";
    protected_warning: "{branch} is a protected branch. Pushing to it may be refused; create a branch instead.", "{branch} 是受保護的分支，推送可能被拒絕，建議改建立分支。";
    summary_long: "Great commit summaries are 50 characters or less.", "好的提交摘要不超過 50 個字元。";
    why_summary: "Write a summary to commit.", "要寫摘要才能提交。";
    why_nothing: "Tick at least one file to commit.", "至少要勾選一個檔案才能提交。";
    why_busy: "Wait for the current operation to finish.", "請等目前的操作完成。";
    why_operation: "Finish resolving conflicts first.", "請先解決完衝突。";
    why_co_author: "A co-author line is not Name <email>.", "有一行共同作者不是「姓名 <email>」的格式。";
    // History
    no_commits: "No commits yet", "還沒有任何提交";
    load_more: "Load more", "載入更多";
    undo: "Undo", "復原";
    undo_commit: "Undo commit", "復原提交";
    reset_to: "Reset to commit…", "重設到這個提交…";
    revert: "Revert changes in commit", "還原這個提交的變更";
    cherry_pick_to: "Cherry-pick onto", "摘取到";
    create_branch_here: "Create branch from commit…", "從這個提交建立分支…";
    create_tag: "Create tag…", "建立標籤…";
    delete_tag: "Delete tag {tag}", "刪除標籤 {tag}";
    checkout_commit: "Check out commit", "簽出這個提交";
    copy_sha: "Copy SHA", "複製 SHA";
    copy_tag: "Copy tag {tag}", "複製標籤 {tag}";
    amend_commit: "Amend commit…", "修改提交…";
    view_on_github_commit: "View on GitHub", "在 GitHub 上檢視";
    drag_hint: "Drag a commit onto a branch to cherry-pick, between commits to reorder, or onto another to squash.", "把提交拖到分支上可摘取，拖到提交之間可重新排序，拖到另一個提交上可壓縮。";
    files_in_commit: "{n} changed files", "{n} 個檔案已變更";
    by_author: "{name} committed {when}", "{name} 於 {when} 提交";
    merge_commit: "Merge commit", "合併提交";
    compare_title: "Compared with {branch}", "與 {branch} 比較";
    compare_ahead: "{n} commits on {branch} not on yours", "{branch} 有 {n} 個你沒有的提交";
    compare_behind: "{n} commits on yours not on {branch}", "你有 {n} 個 {branch} 沒有的提交";
    merge_into: "Merge into {branch}", "合併到 {branch}";
    close: "Close", "關閉";
    // Branches
    filter_branches: "Filter branches", "篩選分支";
    default_branch: "Default branch", "預設分支";
    recent_branches: "Recent branches", "最近的分支";
    other_branches: "Other branches", "其他分支";
    pull_requests: "Pull requests", "拉取要求";
    branches: "Branches", "分支";
    no_pulls: "No open pull requests", "沒有開啟中的拉取要求";
    sign_in_for_pulls: "Sign in to GitHub to see pull requests.", "登入 GitHub 才能看到拉取要求。";
    checkout_pull: "Check out", "簽出";
    view_pull_short: "View on GitHub", "在 GitHub 上檢視";
    detached_warning: "You are not on a branch. Commits made now belong to no branch; create a branch to keep them.", "你現在不在任何分支上，這時提交的內容不屬於任何分支，請建立分支保存。";
    // Checks
    checks: "Checks", "檢查";
    checks_passed: "All checks passed", "所有檢查都通過";
    checks_failed: "Some checks failed", "有檢查失敗";
    checks_pending: "Checks are running", "檢查進行中";
    rerun_checks: "Re-run failed checks", "重新執行失敗的檢查";
    // Conflicts
    conflicts_title: "Resolve conflicts before {operation}", "在{operation}之前先解決衝突";
    conflicts_remaining: "{n} conflicted files", "{n} 個檔案有衝突";
    conflicts_none: "All conflicts resolved", "所有衝突都解決了";
    conflict_markers: "{n} conflicts", "{n} 處衝突";
    conflict_deleted_in: "Deleted in {deleted}, changed in {changed}", "{deleted} 刪除了，{changed} 有修改";
    conflict_both_added: "Added on both sides", "雙方都新增了";
    conflict_both_deleted: "Deleted on both sides", "雙方都刪除了";
    use_side: "Use the version from {side}", "使用 {side} 的版本";
    side_stash: "the stash", "收起的變更";
    mark_resolved: "Mark as resolved", "標示為已解決";
    open_editor: "Open in editor", "在編輯器開啟";
    continue_op: "Continue", "繼續";
    abort_op: "Abort", "中止";
    op_merge: "merging", "合併";
    op_squash_merge: "squash merging", "壓縮合併";
    op_rebase: "rebasing", "重定基底";
    op_cherry_pick: "cherry-picking", "摘取";
    op_revert: "reverting", "還原";
    op_stash: "restoring the stash", "還原收起的變更";
    // Dialogs
    cancel: "Cancel", "取消";
    cancel_network: "Stop talking to the remote", "停止和遠端的連線";
    ok: "OK", "確定";
    save: "Save", "儲存";
    create: "Create", "建立";
    trust_title: "Do you trust this repository?", "你信任這個儲存庫嗎？";
    trust_body: "A repository you trust can run any program its own settings or hooks name, with your permissions, when Better Git reads it, commits, or syncs. Open it only if you trust where it came from.", "信任的儲存庫，可以在 Better Git 讀取、提交或同步時，用你的權限執行它自己的設定或 hook 指定的任何程式。只有在信任來源時才開啟。";
    trust_programs: "Programs this repository names, among others it may run:", "這個儲存庫指定的程式（它還可能執行其他程式）：";
    trust_open: "Trust and open", "信任並開啟";
    add_local_title: "Add local repository", "加入本機儲存庫";
    folder: "Folder", "資料夾";
    choose: "Choose…", "選擇…";
    clone_title: "Clone a repository", "複製儲存庫";
    clone_url_tab: "URL", "網址";
    clone_github_tab: "GitHub", "GitHub";
    clone_url_label: "Repository URL or GitHub username/repository", "儲存庫網址，或 GitHub 使用者/儲存庫";
    clone_into: "Local path", "本機路徑";
    clone: "Clone", "複製";
    cloning: "Cloning…", "正在複製…";
    your_repositories: "Your repositories", "你的儲存庫";
    loading: "Loading…", "載入中…";
    new_repository_title: "Create a new repository", "建立新的儲存庫";
    name: "Name", "名稱";
    description: "Description", "描述";
    local_path: "Local path", "本機路徑";
    initialize_readme: "Initialize with a README", "建立 README";
    gitignore: "Git ignore", "Git 忽略檔";
    license: "License", "授權條款";
    none: "None", "無";
    publish_title: "Publish repository", "發佈儲存庫";
    keep_private: "Keep this code private", "保持程式碼私人";
    organization: "Organization", "組織";
    personal_account: "Your account", "你的帳號";
    publish: "Publish", "發佈";
    alias_title: "Repository alias", "儲存庫別名";
    remove_repository_title: "Remove repository", "移除儲存庫";
    remove_repository_body: "Remove {name} from the list? The folder and its files are left on disk.", "要把 {name} 從清單移除嗎？資料夾和檔案會留在磁碟上。";
    remove: "Remove", "移除";
    new_branch_title: "Create a branch", "建立分支";
    branch_name: "Name", "名稱";
    branch_from: "Create branch based on {start}", "以 {start} 為基礎建立分支";
    branch_will_be: "Will be created as {name}", "會建立成 {name}";
    rename_branch_title: "Rename branch", "重新命名分支";
    rename: "Rename", "重新命名";
    delete_branch_title: "Delete branch", "刪除分支";
    delete_branch_body: "Delete the branch {name}? Commits that are only on this branch are deleted with it. This cannot be undone.", "要刪除分支 {name} 嗎？只在這個分支上的提交會一起刪除。這個動作無法復原。";
    delete_remote_too: "Also delete the branch on the remote", "同時刪除遠端上的分支";
    delete: "Delete", "刪除";
    switch_title: "Switch branch", "切換分支";
    switch_body: "You have changes on this branch. What should happen to them?", "這個分支上有變更，要怎麼處理？";
    leave_changes: "Leave my changes on {branch}", "把變更留在 {branch}";
    bring_changes: "Bring my changes to {target}", "把變更帶到 {target}";
    switch: "Switch branch", "切換分支";
    pick_merge: "Merge into {branch}", "合併到 {branch}";
    pick_squash: "Squash and merge into {branch}", "壓縮並合併到 {branch}";
    pick_rebase: "Rebase {branch}", "重定基底 {branch}";
    pick_compare: "Compare to branch", "與分支比較";
    merge: "Merge", "合併";
    squash_and_merge: "Squash and merge", "壓縮並合併";
    rebase: "Rebase", "重定基底";
    compare: "Compare", "比較";
    discard_title: "Discard changes", "捨棄變更";
    discard_body: "Discard the changes to these files? Their current content is moved to the trash, where you can get it back.", "要捨棄這些檔案的變更嗎？目前的內容會移到垃圾桶，之後還能找回來。";
    discard_lines_body: "Discard the selected lines of {name}? A copy of the file goes to the trash first.", "要捨棄 {name} 選取的行嗎？會先把檔案複本放進垃圾桶。";
    discard_more: "and {n} more", "以及另外 {n} 個";
    undo_title: "Undo commit", "復原提交";
    undo_body: "Undo the last commit? Its changes stay in your working copy and its message goes back into the form.", "要復原上一個提交嗎？變更會留在工作複本，訊息會放回表單。";
    reset_title: "Reset to commit", "重設到提交";
    reset_body: "Reset the branch to \"{summary}\"? Later commits are removed from the branch; their changes stay in your working copy.", "要把分支重設到「{summary}」嗎？之後的提交會從分支移除，變更會留在工作複本。";
    reset: "Reset", "重設";
    force_push_title: "Force push?", "要強制推送嗎？";
    force_push_body: "This replaces the branch on the remote with yours. Only the commits you last saw there are replaced: if anyone pushed since, the push is refused.", "這會用你的分支取代遠端上的分支。只會取代你上次看到的那些提交：之後只要有人推送過，推送就會被拒絕。";
    force_push: "Force push", "強制推送";
    tag_title: "Create a tag", "建立標籤";
    tag_name: "Name", "名稱";
    delete_tag_title: "Delete tag", "刪除標籤";
    delete_tag_body: "Delete the tag {tag}?", "要刪除標籤 {tag} 嗎？";
    squash_title: "Squash commits", "壓縮提交";
    squash_body: "The two commits become one with this message:", "這兩個提交會合成一個，訊息如下：";
    squash: "Squash", "壓縮";
    worktrees_title: "Worktrees", "工作樹";
    worktree_main: "Main worktree", "主要工作樹";
    worktree_open: "Open", "開啟";
    add_worktree: "Add worktree…", "加入工作樹…";
    add_worktree_title: "Add a worktree", "加入工作樹";
    worktree_branch: "Branch", "分支";
    worktree_new_branch: "Create a new branch", "建立新分支";
    rename_worktree_title: "Move worktree", "移動工作樹";
    new_folder: "New folder", "新資料夾";
    remove_worktree_title: "Remove worktree", "移除工作樹";
    remove_worktree_body: "Remove the worktree at {path}? Its folder is deleted.", "要移除 {path} 的工作樹嗎？它的資料夾會被刪除。";
    force_remove: "Remove even with changes", "即使有變更也移除";
    remotes_title: "Remotes", "遠端";
    remote_name: "Name", "名稱";
    remote_url: "URL", "網址";
    add_remote: "Add remote", "加入遠端";
    change_url: "Change URL", "變更網址";
    pull_title: "Create a pull request", "建立拉取要求";
    pull_from_to: "From {head} into", "從 {head} 合併到";
    title: "Title", "標題";
    body: "Description", "描述";
    draft: "Create as draft", "建立為草稿";
    issue_title: "Create an issue", "建立議題";
    sign_in_title: "Sign in to GitHub", "登入 GitHub";
    sign_in: "Sign in", "登入";
    sign_out: "Sign out", "登出";
    sign_in_enterprise: "Sign in to GitHub Enterprise…", "登入 GitHub Enterprise…";
    enterprise_host: "Enterprise address", "Enterprise 位址";
    enterprise_client_id: "OAuth App client ID (optional)", "OAuth App 用戶端 ID（選填）";
    enterprise_client_id_hint: "Your server's administrator registers an OAuth App with device flow on and gives you its client ID. Leave it empty to use Better Git's.", "由伺服器管理員註冊一個開啟裝置流程的 OAuth App，再把它的用戶端 ID 給你。留空則使用 Better Git 的。";
    sign_in_starting: "Asking GitHub for a code…", "正在向 GitHub 取得代碼…";
    sign_in_code: "Enter this code on GitHub:", "在 GitHub 上輸入這個代碼：";
    sign_in_open: "Copy code and open GitHub", "複製代碼並開啟 GitHub";
    sign_in_waiting: "Waiting for you to approve Better Git…", "正在等你核准 Better Git…";
    sign_in_expired: "The code expired. Start again.", "代碼已過期，請重新開始。";
    sign_in_denied: "Sign-in was declined on GitHub.", "已在 GitHub 上拒絕登入。";
    sign_in_disabled: "This app cannot sign in with a device code.", "這個 App 無法用裝置代碼登入。";
    sign_in_unavailable: "Signing in to GitHub isn't available in this build.", "這個版本無法登入 GitHub。";
    try_again: "Try again", "重試";
    mark_resolved_title: "Mark as resolved?", "要標示為已解決嗎？";
    mark_resolved_body: "{name} still has conflict markers. Mark it resolved anyway?", "{name} 裡還有衝突標記，仍要標示為已解決嗎？";
    fork_title: "Fork this repository?", "要分叉這個儲存庫嗎？";
    fork_body: "You can't push to this repository. Fork it to your account and push there; the original stays as \"upstream\".", "你無法推送到這個儲存庫。分叉到你的帳號再推送，原本的會保留為「upstream」。";
    fork: "Fork", "分叉";
    drop_stash_title: "Discard stash?", "要捨棄收起的變更嗎？";
    drop_stash_body: "The stashed changes are deleted and cannot be restored.", "收起的變更會被刪除，無法還原。";
    // Settings
    settings_accounts: "Accounts", "帳號";
    settings_integrations: "Integrations", "整合";
    settings_git: "Git", "Git";
    settings_appearance: "Appearance", "外觀";
    settings_prompts: "Prompts", "確認提示";
    settings_advanced: "Advanced", "進階";
    signed_in_as: "Signed in as {login} on {host}", "已用 {login} 登入 {host}";
    not_signed_in: "Not signed in", "尚未登入";
    editor: "External editor", "外部編輯器";
    editor_hint: "A command, such as code or gnome-text-editor. Empty uses the first one found.", "指令，例如 code 或 gnome-text-editor。留空會用找到的第一個。";
    terminal: "Terminal", "終端機";
    terminal_hint: "A command, such as gnome-terminal. Empty uses the first one found.", "指令，例如 gnome-terminal。留空會用找到的第一個。";
    git_name: "Name", "名稱";
    git_email: "Email", "電子郵件";
    git_default_branch: "Default branch name for new repositories", "新儲存庫的預設分支名稱";
    git_hint: "Your global Git settings, used by every repository.", "全域 Git 設定，所有儲存庫都會用到。";
    theme: "Theme", "主題";
    theme_dark: "Dark", "深色";
    theme_light: "Light", "淺色";
    theme_system: "Use system setting", "跟隨系統";
    language: "Language", "語言";
    language_system: "Use system setting", "跟隨系統";
    confirm_discard: "Before discarding changes", "捨棄變更前";
    confirm_force_push: "Before force pushing", "強制推送前";
    confirm_remove: "Before removing a repository", "移除儲存庫前";
    confirm_undo: "Before undoing a commit", "復原提交前";
    background_fetch: "Fetch in the background every five minutes", "每五分鐘在背景擷取";
    keyboard_shortcuts: "Keyboard shortcuts", "鍵盤快速鍵";
    // Info bar
    error_title: "Something went wrong", "發生錯誤";
    error_details: "Details", "詳細資料";
    error_copy: "Copy", "複製";
    next_pull: "Pull", "拉取";
    next_force_push: "Force push…", "強制推送…";
    next_sign_in: "Sign in", "登入";
    next_fork: "Fork…", "分叉…";
    next_stash: "Stash changes", "收起變更";
    next_publish: "Publish branch", "發佈分支";
    open_link: "Open", "開啟";
    working: "Working…", "處理中…";
}

/// The window's language, with `System` resolved.
pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}

/// Fills `{name}` placeholders.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in values {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

impl Copy {
    fn zh(&self) -> bool {
        std::ptr::eq(self, &ZH_TW)
    }

    pub(crate) fn t(&self, en: &'static str, zh: &'static str) -> &'static str {
        if self.zh() { zh } else { en }
    }

    pub fn commit_label(&self, branch: Option<&str>) -> String {
        match branch {
            Some(branch) => fill(self.commit_to, &[("branch", branch)]),
            None => self.commit.to_string(),
        }
    }

    /// What an operation is called in a message.
    pub fn op_name(&self, kind: OpKind) -> &'static str {
        use OpKind::*;
        match kind {
            Refresh => self.t("Reading the repository", "讀取儲存庫"),
            Commit => self.t("Commit", "提交"),
            Discard => self.t("Discard", "捨棄"),
            Ignore => self.t("Ignore", "忽略"),
            UndoCommit => self.t("Undo commit", "復原提交"),
            Reset => self.t("Reset", "重設"),
            Revert => self.t("Revert", "還原"),
            CherryPick => self.t("Cherry-pick", "摘取"),
            Reorder => self.t("Reorder", "重新排序"),
            Squash => self.t("Squash", "壓縮"),
            CreateBranch => self.t("Create branch", "建立分支"),
            Switch => self.t("Switch branch", "切換分支"),
            RenameBranch => self.t("Rename branch", "重新命名分支"),
            DeleteBranch => self.t("Delete branch", "刪除分支"),
            Merge => self.t("Merge", "合併"),
            Rebase => self.t("Rebase", "重定基底"),
            StashAll => self.t("Stash", "收起變更"),
            RestoreStash => self.t("Restore stash", "還原收起的變更"),
            DropStash => self.t("Discard stash", "捨棄收起的變更"),
            CreateTag => self.t("Create tag", "建立標籤"),
            DeleteTag => self.t("Delete tag", "刪除標籤"),
            AddWorktree => self.t("Add worktree", "加入工作樹"),
            RenameWorktree => self.t("Move worktree", "移動工作樹"),
            RemoveWorktree => self.t("Remove worktree", "移除工作樹"),
            Fetch | BackgroundFetch => self.t("Fetch", "擷取"),
            Pull => self.t("Pull", "拉取"),
            Push => self.t("Push", "推送"),
            ForcePush => self.t("Force push", "強制推送"),
            AddRemote => self.t("Add remote", "加入遠端"),
            RemoveRemote => self.t("Remove remote", "移除遠端"),
            SetRemoteUrl => self.t("Change remote URL", "變更遠端網址"),
            Resolve => self.t("Resolve", "解決衝突"),
            Continue => self.t("Continue", "繼續"),
            Abort => self.t("Abort", "中止"),
            Clone => self.t("Clone", "複製"),
            Init => self.t("Create repository", "建立儲存庫"),
            GitConfig => self.t("Git settings", "Git 設定"),
        }
    }

    /// The plain-words explanation of a refused network operation.
    pub fn refusal(&self, refusal: Refusal, push: bool) -> &'static str {
        match refusal {
            Refusal::NotFastForward if push => self.t(
                "The remote has commits you don't have yet. Pull them first, then push.",
                "遠端有你還沒有的提交。先拉取，再推送。",
            ),
            Refusal::NotFastForward => self.t(
                "Your branch and the remote have both changed.",
                "你的分支和遠端都有變更。",
            ),
            Refusal::StaleLease => self.t(
                "Someone pushed to this branch since you last fetched, so the force push was stopped to keep their work. Fetch, look at what changed, and try again.",
                "上次擷取之後有人推送到這個分支，為了保護他們的內容，強制推送已停止。請先擷取、看看變更後再試。",
            ),
            Refusal::Authentication => self.t(
                "The remote didn't accept your sign-in. Sign in to GitHub, or check your SSH key or credential helper.",
                "遠端不接受你的登入資訊。請登入 GitHub，或檢查 SSH 金鑰與認證輔助程式。",
            ),
            Refusal::PermissionDenied => self.t(
                "Your account can't push to this repository. Fork it to push your work.",
                "你的帳號無法推送到這個儲存庫。分叉後再推送。",
            ),
            Refusal::ProtectedBranch => self.t(
                "This branch is protected on GitHub. Push to a new branch and open a pull request.",
                "這個分支在 GitHub 上受保護。請推送到新分支並建立拉取要求。",
            ),
            Refusal::HookDeclined => self.t(
                "The server's rules refused this push. Its message below says why.",
                "伺服器的規則拒絕了這次推送，原因請看下面的訊息。",
            ),
            Refusal::FileTooLarge => self.t(
                "A file is larger than the host accepts. Remove it from the commits, or use Git LFS.",
                "有檔案超過主機允許的大小。請從提交中移除，或改用 Git LFS。",
            ),
            Refusal::RepositoryNotFound => self.t(
                "The repository wasn't found. It may have moved, or your account can't see it.",
                "找不到儲存庫。它可能已搬移，或你的帳號看不到它。",
            ),
            Refusal::Network => self.t(
                "The remote couldn't be reached. Check your network connection.",
                "連不到遠端，請檢查網路連線。",
            ),
            Refusal::NoUpstream => self.t(
                "This branch isn't on the remote yet. Publish it first.",
                "這個分支還不在遠端上，請先發佈。",
            ),
            Refusal::LocalChangesWouldBeOverwritten => self.t(
                "Your uncommitted changes would be overwritten. Commit or stash them, then try again.",
                "尚未提交的變更會被覆蓋。請先提交或收起，再試一次。",
            ),
            Refusal::Conflicts => self.t(
                "There are conflicts to resolve.",
                "有衝突需要解決。",
            ),
            Refusal::Diverged => self.t(
                "Your branch and the remote have both changed and Git needs to know how to combine them.",
                "你的分支和遠端都有變更，Git 需要知道要怎麼合在一起。",
            ),
            Refusal::Other => self.t("Git refused.", "Git 拒絕了。"),
        }
    }

    /// The info bar's title and message for a problem, and git's own text
    /// for the details.
    pub fn problem(&self, problem: &Problem) -> (String, String, String) {
        let failed = |what: &str| fill(self.t("{what} failed", "{what}失敗"), &[("what", what)]);
        match problem {
            Problem::Git { kind, error } => {
                let details = git_details(error);
                let message = match error {
                    GitError::NotInstalled => self
                        .t(
                            "Git isn't installed. Install the git package to use Better Git.",
                            "沒有安裝 Git，請安裝 git 套件才能使用 Better Git。",
                        )
                        .to_string(),
                    GitError::NotARepository(path) => fill(
                        self.t(
                            "{path} isn't a Git repository.",
                            "{path} 不是 Git 儲存庫。",
                        ),
                        &[("path", &path.to_string_lossy())],
                    ),
                    GitError::Conflicts(_) => self
                        .t(
                            "It stopped with conflicts. Resolve them in the list, then continue.",
                            "因為有衝突而停下，請在清單裡解決後繼續。",
                        )
                        .to_string(),
                    GitError::ChangedMeanwhile
                        if matches!(kind, OpKind::Reorder | OpKind::Squash) =>
                    {
                        self.t(
                            "The history changed since you looked at it (a new commit, or another program). Nothing was rewritten; look at it again and retry.",
                            "你查看之後歷程記錄變了（有新的提交，或其他程式改過）。沒有改寫任何東西，請重新查看後再試。",
                        )
                        .to_string()
                    }
                    GitError::Cancelled => self
                        .t("You stopped it. Nothing more was sent.", "已經停止，不會再傳送任何東西。")
                        .to_string(),
                    GitError::Stalled => self
                        .t(
                            "The remote stopped answering, so Better Git stopped waiting. Check the connection and try again.",
                            "遠端沒有回應，Better Git 已停止等待。請檢查網路連線後再試。",
                        )
                        .to_string(),
                    GitError::ChangedMeanwhile => self
                        .t(
                            "The file changed while you were choosing lines. Look at it again and retry.",
                            "選擇行的時候檔案變了，請重新查看後再試。",
                        )
                        .to_string(),
                    GitError::Failed { message, .. } if kind.is_network() => {
                        let push = matches!(kind, OpKind::Push | OpKind::ForcePush);
                        self.refusal(git_core::classify_refusal(message), push)
                            .to_string()
                    }
                    GitError::Failed { .. } => self
                        .t("Git's message says why.", "Git 的訊息說明了原因。")
                        .to_string(),
                    GitError::UnreadableOutput(_) => self
                        .t(
                            "Git printed something Better Git can't read.",
                            "Git 輸出了 Better Git 讀不懂的內容。",
                        )
                        .to_string(),
                    GitError::Io(text) => text.clone(),
                    GitError::Unsupported(_) => String::new(),
                };
                let title = if matches!(error, GitError::Conflicts(_)) {
                    fill(
                        self.t("{what} stopped with conflicts", "{what}因衝突而停下"),
                        &[("what", self.op_name(*kind))],
                    )
                } else {
                    failed(self.op_name(*kind))
                };
                (title, message, details)
            }
            Problem::GitHub(error) => {
                let message = match error {
                    ApiError::Unauthorized => self.t(
                        "GitHub no longer accepts your sign-in. Sign in again.",
                        "GitHub 不再接受你的登入，請重新登入。",
                    ),
                    ApiError::Network(_) => {
                        self.t("GitHub couldn't be reached.", "連不到 GitHub。")
                    }
                    ApiError::Status { .. } => self.t("GitHub refused.", "GitHub 拒絕了。"),
                    ApiError::Unreadable(_) => self.t(
                        "GitHub answered something Better Git can't read.",
                        "GitHub 回應了 Better Git 讀不懂的內容。",
                    ),
                    ApiError::RateLimited { retry_after } => {
                        let minutes = retry_after.map(|s| s.div_ceil(60).max(1));
                        let message = match minutes {
                            Some(m) => fill(
                                self.t(
                                    "GitHub asks Better Git to wait: too many requests. Try again in {n} min.",
                                    "GitHub 要求 Better Git 稍候：要求太多了。請在 {n} 分鐘後再試。",
                                ),
                                &[("n", &m.to_string())],
                            ),
                            None => self
                                .t(
                                    "GitHub asks Better Git to wait: too many requests. Try again later.",
                                    "GitHub 要求 Better Git 稍候：要求太多了。請稍後再試。",
                                )
                                .to_string(),
                        };
                        return (failed("GitHub"), message, error_details(error));
                    }
                };
                (failed("GitHub"), message.to_string(), error_details(error))
            }
            Problem::SignIn(problem) => (
                failed(self.sign_in),
                self.sign_in_problem(problem).to_string(),
                String::new(),
            ),
            Problem::Platform(text) => (
                self.error_title.to_string(),
                self.t(
                    "The desktop couldn't show the folder picker.",
                    "桌面無法顯示資料夾選擇器。",
                )
                .to_string(),
                text.clone(),
            ),
            Problem::Launch(text) => (
                self.error_title.to_string(),
                self.t(
                    "The program couldn't be started. Choose another one in Settings.",
                    "無法啟動程式，請在設定裡選擇其他程式。",
                )
                .to_string(),
                text.clone(),
            ),
            Problem::Keyring(text) => (
                self.error_title.to_string(),
                self.t(
                    "The desktop keyring refused. Your sign-in won't be remembered.",
                    "桌面鑰匙圈拒絕了，你的登入不會被記住。",
                )
                .to_string(),
                text.clone(),
            ),
            Problem::Io(text) => (
                self.error_title.to_string(),
                self.t(
                    "Better Git couldn't save its own file.",
                    "Better Git 無法儲存自己的檔案。",
                )
                .to_string(),
                text.clone(),
            ),
            Problem::NoClientId => (
                failed(self.sign_in),
                self.sign_in_unavailable.to_string(),
                String::new(),
            ),
        }
    }

    pub fn sign_in_problem(&self, problem: &SignInProblem) -> &'static str {
        match problem {
            SignInProblem::Expired => self.sign_in_expired,
            SignInProblem::Denied => self.sign_in_denied,
            SignInProblem::Disabled => self.sign_in_disabled,
            SignInProblem::Api(ApiError::Network(_)) => {
                self.t("GitHub couldn't be reached.", "連不到 GitHub。")
            }
            SignInProblem::Api(_) => self.t("GitHub refused.", "GitHub 拒絕了。"),
        }
    }

    /// A toast's text.
    pub fn notice(&self, notice: &Notice) -> String {
        match notice {
            Notice::PullCreated { number, .. } => fill(
                self.t("Pull request #{n} created", "已建立拉取要求 #{n}"),
                &[("n", &number.to_string())],
            ),
            Notice::IssueCreated { number, .. } => fill(
                self.t("Issue #{n} created", "已建立議題 #{n}"),
                &[("n", &number.to_string())],
            ),
            Notice::Published { url } => fill(
                self.t("Published to {url}", "已發佈到 {url}"),
                &[("url", url)],
            ),
            Notice::Forked { name } => fill(
                self.t(
                    "Forked to {name}. Push again to send your work there.",
                    "已分叉到 {name}。再推送一次就會送到那裡。",
                ),
                &[("name", name)],
            ),
            Notice::ChecksRerun(n) => fill(
                self.t(
                    "Re-running {n} failed checks",
                    "正在重新執行 {n} 個失敗的檢查",
                ),
                &[("n", &n.to_string())],
            ),
            Notice::SignedIn(login) => fill(
                self.t("Signed in as {login}", "已用 {login} 登入"),
                &[("login", login)],
            ),
            Notice::StashWaiting(_) => self
                .t(
                    "This branch has stashed changes. Restore them from the changes list.",
                    "這個分支有收起的變更，可以在變更清單中還原。",
                )
                .to_string(),
            Notice::Pull(pull) => self.pull_notice(pull).1,
            Notice::WatchFallback => self.watch_fallback().to_string(),
        }
    }

    /// A pull-request notification's summary and body.
    pub fn pull_notice(&self, notice: &crate::notifications::Notice) -> (String, String) {
        use crate::notifications::Notice as N;
        match notice {
            N::ChecksFailed {
                number,
                title,
                failed,
                ..
            } => (
                fill(
                    self.t("Checks failed on #{n}", "#{n} 的檢查失敗"),
                    &[("n", &number.to_string())],
                ),
                format!("{title}: {}", failed.join(", ")),
            ),
            N::Reviewed {
                number,
                title,
                reviewer,
                state,
                ..
            } => {
                let verb = match state.as_str() {
                    "APPROVED" => self.t("approved", "核准了"),
                    "CHANGES_REQUESTED" => self.t("requested changes on", "要求修改"),
                    _ => self.t("commented on", "留言於"),
                };
                (
                    fill(
                        self.t("{who} {verb} #{n}", "{who}{verb} #{n}"),
                        &[
                            ("who", reviewer),
                            ("verb", verb),
                            ("n", &number.to_string()),
                        ],
                    ),
                    title.clone(),
                )
            }
        }
    }

    /// "5 minutes ago" for the last fetch.
    pub fn ago(&self, seconds: i64) -> String {
        if seconds < 60 {
            self.just_now.to_string()
        } else if seconds < 3600 {
            fill(self.minutes_ago, &[("n", &(seconds / 60).to_string())])
        } else {
            fill(self.hours_ago, &[("n", &(seconds / 3600).to_string())])
        }
    }
}

fn git_details(error: &GitError) -> String {
    match error {
        GitError::Failed { message, .. } | GitError::Conflicts(message) => message.clone(),
        GitError::UnreadableOutput(detail) => detail.clone(),
        GitError::Io(text) => text.clone(),
        GitError::NotARepository(path) => path.to_string_lossy().into_owned(),
        other => other.to_string(),
    }
}

fn error_details(error: &ApiError) -> String {
    match error {
        ApiError::Network(text) | ApiError::Unreadable(text) => text.clone(),
        ApiError::Status { status, message } => format!("{status}: {message}"),
        ApiError::Unauthorized => "401".to_string(),
        ApiError::RateLimited { retry_after } => match retry_after {
            Some(seconds) => format!("rate limited, retry after {seconds} s"),
            None => "rate limited".to_string(),
        },
    }
}
