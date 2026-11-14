//! Words for what ticket 103 added: why a dialog cannot go ahead, the
//! identity step, the toolbar's shortcut buttons, worktrees in the lists,
//! and live change detection. They sit beside [`crate::i18n`]'s table rather
//! than in it so the table stays one block per screen; each string is still
//! written in both languages in one call, so neither can be missing.

use crate::dialogs::Invalid;
use crate::i18n::Copy;

impl Copy {
    /// Why a dialog's primary button is off, for under the field.
    pub fn invalid_message(&self, reason: Invalid) -> &'static str {
        match reason {
            Invalid::Empty => self.t("This can't be empty.", "這裡不能空白。"),
            Invalid::BranchExists => self.t(
                "A branch with this name already exists.",
                "已經有同名的分支。",
            ),
            Invalid::BranchReserved => self.t(
                "HEAD is a name Git keeps for itself.",
                "HEAD 是 Git 保留的名稱。",
            ),
            Invalid::TagExists => self.t("A tag with this name already exists.", "已經有同名的標籤。"),
            Invalid::TagTooLong => self.t("The name is too long.", "名稱太長了。"),
            Invalid::NotAbsolute => self.t(
                "Use a full path that starts with /.",
                "請用 / 開頭的完整路徑。",
            ),
            Invalid::NoSelection => self.t("Choose one from the list.", "請從清單中選一個。"),
            Invalid::AlreadyRepository => self.t(
                "A repository with this name is already in that folder. Add it as an existing repository, or choose another name.",
                "這個資料夾裡已經有同名的儲存庫。可以把它加入為現有的儲存庫，或換一個名稱。",
            ),
            Invalid::NotEmpty => self.t(
                "A folder with this name already has files in it. Choose another name or location.",
                "同名的資料夾裡已經有檔案。請換一個名稱或位置。",
            ),
            Invalid::NotAFolder => self.t(
                "A file with this name is already there.",
                "那裡已經有同名的檔案。",
            ),
            Invalid::NotAnEmail => self.t(
                "Enter an email address, such as name@example.com.",
                "請輸入電子郵件地址，例如 name@example.com。",
            ),
        }
    }

    // --- the identity step -------------------------------------------------

    pub fn identity_title(&self) -> &'static str {
        self.t("Who is committing?", "提交者是誰？")
    }

    pub fn identity_body(&self) -> &'static str {
        self.t(
            "Every commit records a name and an email, and Git on this computer does not have both yet. Better Git adds what is missing to the settings every repository here uses, keeps what is already set, and then carries on.",
            "每個提交都會記錄名稱和電子郵件，這台電腦上的 Git 還沒有兩項都設定好。Better Git 會把缺的補進這裡所有儲存庫共用的設定，已經設定的保留不變，然後繼續剛才的動作。",
        )
    }

    pub fn identity_name(&self) -> &'static str {
        self.t("Name", "名稱")
    }

    pub fn identity_email(&self) -> &'static str {
        self.t("Email", "電子郵件")
    }

    pub fn identity_email_hint(&self) -> &'static str {
        self.t(
            "Use the email of your GitHub account, or its private no-reply address, so GitHub links your commits to you.",
            "請用 GitHub 帳號的電子郵件或它的私人 no-reply 地址，GitHub 才會把提交連到你的帳號。",
        )
    }

    pub fn identity_writes(&self) -> &'static str {
        self.t(
            "Git's settings for every repository on this computer:",
            "這台電腦所有儲存庫共用的 Git 設定：",
        )
    }

    /// Beside a setting Git already has, which the step keeps.
    pub fn identity_kept(&self) -> &'static str {
        self.t("already set, kept", "已設定，保留不變")
    }

    /// Beside a setting the step adds.
    pub fn identity_new(&self) -> &'static str {
        self.t("new", "新增")
    }

    pub fn identity_save(&self) -> &'static str {
        self.t("Save and continue", "儲存並繼續")
    }

    // --- the toolbar's shortcut buttons ------------------------------------

    /// "Open in {editor}", naming the editor from Settings.
    pub fn open_in_named_editor(&self) -> &'static str {
        self.t("Open in {editor}", "在 {editor} 開啟")
    }

    pub fn editor_fallback_name(&self) -> &'static str {
        self.t("editor", "編輯器")
    }

    pub fn show_in_better_files(&self) -> &'static str {
        self.t("Show in Better Files", "在 Better Files 顯示")
    }

    pub fn open_in_terminal_short(&self) -> &'static str {
        self.t("Open in Terminal", "在終端機開啟")
    }

    pub fn view_on_github_short(&self) -> &'static str {
        self.t("View on GitHub", "在 GitHub 上檢視")
    }

    // --- amend ---------------------------------------------------------------

    pub fn reading_last_commit(&self) -> &'static str {
        self.t("Reading the last commit…", "正在讀取上一個提交…")
    }

    // --- worktrees in the lists ----------------------------------------------

    pub fn worktree_detached(&self) -> &'static str {
        self.t("Detached", "分離的 HEAD")
    }

    /// A detached worktree's secondary text in the repository list.
    pub fn worktree_detached_at(&self) -> &'static str {
        self.t("Detached at {sha}", "分離於 {sha}")
    }

    pub fn open_in_its_worktree(&self) -> &'static str {
        self.t("Open in its worktree", "在它的工作樹開啟")
    }

    pub fn checked_out_in(&self) -> &'static str {
        self.t("Checked out in {path}", "已在 {path} 簽出")
    }

    // --- live changes ----------------------------------------------------------

    pub fn watch_fallback(&self) -> &'static str {
        self.t(
            "Better Git can't watch this folder for changes, so it checks for them every 3 seconds.",
            "Better Git 無法監看這個資料夾的變更，改成每 3 秒檢查一次。",
        )
    }
}
