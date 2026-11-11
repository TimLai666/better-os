//! Every word Better Git shows, in zh-TW and English.
//!
//! The pattern is the other windows': one [`Copy`] struct, one constant per
//! language, so a string added to one and forgotten in the other does not
//! compile. Choosing a language is `--lang`, or the session's `LANG` the way
//! Better Files reads it.

use git_platform::GitError;

use crate::session::Problem;

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

pub struct Copy {
    pub app_name: &'static str,

    // Command bar
    pub open: &'static str,
    pub detached: &'static str,

    // Changes pane
    pub changes: &'static str,
    pub no_changes: &'static str,
    pub conflicted: &'static str,
    pub stage_all: &'static str,
    pub unstage_all: &'static str,

    // Commit form
    pub summary_placeholder: &'static str,
    pub description_placeholder: &'static str,
    /// `{branch}` is replaced with the branch name.
    pub commit_to: &'static str,
    /// The button when HEAD is detached and there is no branch to name.
    pub commit: &'static str,

    // Diff pane
    pub select_file: &'static str,
    pub binary_file: &'static str,

    // No repository open
    pub open_title: &'static str,
    pub open_detail: &'static str,
    pub recent: &'static str,

    // Info bar
    pub close: &'static str,
    pub not_installed_title: &'static str,
    pub not_installed_message: &'static str,
    pub not_a_repository_title: &'static str,
    /// `{path}` is replaced with the folder.
    pub not_a_repository_message: &'static str,
    pub failed_title: &'static str,
    pub unreadable_title: &'static str,
    /// `{detail}` is replaced with what could not be read.
    pub unreadable_message: &'static str,
    pub picker_title: &'static str,
}

impl Copy {
    /// Every string, so a test can check none of them is empty. Listing the
    /// fields by destructuring makes a new field a compile error here until it
    /// is added.
    pub fn all(&self) -> Vec<&'static str> {
        let Copy {
            app_name,
            open,
            detached,
            changes,
            no_changes,
            conflicted,
            stage_all,
            unstage_all,
            summary_placeholder,
            description_placeholder,
            commit_to,
            commit,
            select_file,
            binary_file,
            open_title,
            open_detail,
            recent,
            close,
            not_installed_title,
            not_installed_message,
            not_a_repository_title,
            not_a_repository_message,
            failed_title,
            unreadable_title,
            unreadable_message,
            picker_title,
        } = *self;
        vec![
            app_name,
            open,
            detached,
            changes,
            no_changes,
            conflicted,
            stage_all,
            unstage_all,
            summary_placeholder,
            description_placeholder,
            commit_to,
            commit,
            select_file,
            binary_file,
            open_title,
            open_detail,
            recent,
            close,
            not_installed_title,
            not_installed_message,
            not_a_repository_title,
            not_a_repository_message,
            failed_title,
            unreadable_title,
            unreadable_message,
            picker_title,
        ]
    }

    /// The commit button's label.
    pub fn commit_label(&self, branch: Option<&str>) -> String {
        match branch {
            Some(branch) => self.commit_to.replace("{branch}", branch),
            None => self.commit.to_string(),
        }
    }

    /// The info bar's title and message for a problem.
    pub fn problem(&self, problem: &Problem) -> (&'static str, String) {
        match problem {
            Problem::Git(GitError::NotInstalled) => (
                self.not_installed_title,
                self.not_installed_message.to_string(),
            ),
            Problem::Git(GitError::NotARepository(path)) => (
                self.not_a_repository_title,
                self.not_a_repository_message
                    .replace("{path}", &path.to_string_lossy()),
            ),
            // git's own words, as it printed them.
            Problem::Git(GitError::Failed { message, .. }) => (self.failed_title, message.clone()),
            Problem::Git(GitError::UnreadableOutput(detail)) => (
                self.unreadable_title,
                self.unreadable_message.replace("{detail}", detail),
            ),
            Problem::FolderPicker(message) => (self.picker_title, message.clone()),
        }
    }
}

pub const EN_US: Copy = Copy {
    app_name: "Better Git",
    open: "Open…",
    detached: "Detached HEAD",
    changes: "Changes",
    no_changes: "No changes",
    conflicted: "Conflicted",
    stage_all: "Stage all changes",
    unstage_all: "Unstage all changes",
    summary_placeholder: "Summary (required)",
    description_placeholder: "Description",
    commit_to: "Commit to {branch}",
    commit: "Commit",
    select_file: "Select a file to see its changes",
    binary_file: "Binary file",
    open_title: "Open a repository",
    open_detail: "Choose a folder that contains a Git repository.",
    recent: "Recent",
    close: "Close",
    not_installed_title: "Git isn't installed",
    not_installed_message: "Better Git runs the git program, and none was found. Install git, then open the repository again.",
    not_a_repository_title: "Not a repository",
    not_a_repository_message: "{path} is not inside a Git repository.",
    failed_title: "Git couldn't finish",
    unreadable_title: "Unexpected output from git",
    unreadable_message: "Better Git could not read what git printed: {detail}",
    picker_title: "Couldn't show the folder picker",
};

pub const ZH_TW: Copy = Copy {
    app_name: "Better Git",
    open: "開啟…",
    detached: "分離的 HEAD",
    changes: "變更",
    no_changes: "沒有變更",
    conflicted: "衝突",
    stage_all: "暫存全部變更",
    unstage_all: "取消暫存全部變更",
    summary_placeholder: "摘要（必填）",
    description_placeholder: "說明",
    commit_to: "提交到 {branch}",
    commit: "提交",
    select_file: "選擇檔案以檢視變更",
    binary_file: "二進位檔",
    open_title: "開啟儲存庫",
    open_detail: "選擇含有 Git 儲存庫的資料夾。",
    recent: "最近開啟",
    close: "關閉",
    not_installed_title: "沒有安裝 Git",
    not_installed_message: "Better Git 需要 git 程式，但找不到它。請先安裝 git，再重新開啟儲存庫。",
    not_a_repository_title: "不是 Git 儲存庫",
    not_a_repository_message: "「{path}」不在任何 Git 儲存庫裡。",
    failed_title: "Git 無法完成",
    unreadable_title: "看不懂 git 的輸出",
    unreadable_message: "Better Git 讀不懂 git 印出的內容：{detail}",
    picker_title: "無法顯示資料夾選擇視窗",
};

pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}
