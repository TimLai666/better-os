//! Localized wording for the chooser, following the manager GUI's approach:
//! one struct of strings per locale, resolved from the environment unless the
//! embedding surface passes a locale in.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    System,
    EnUs,
    ZhTw,
}

impl Locale {
    pub fn resolved(self) -> Self {
        match self {
            Self::System => {
                let language = std::env::var("LANG")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if language.contains("zh_tw")
                    || language.contains("zh-tw")
                    || language.contains("hant")
                {
                    Self::ZhTw
                } else {
                    Self::EnUs
                }
            }
            locale => locale,
        }
    }

    /// The locale to resolve desktop-entry translations with, so an
    /// application's own localized name is used rather than its default one.
    pub fn entry_locale(self) -> Option<app_catalog_core::Locale> {
        match self.resolved() {
            Self::ZhTw => app_catalog_core::Locale::parse("zh_TW"),
            _ => app_catalog_core::Locale::parse("en_US"),
        }
    }
}

pub struct Copy {
    pub open_with_title: &'static str,
    pub open_with_subtitle: &'static str,
    pub executable_title: &'static str,
    pub executable_subtitle: &'static str,
    pub mode_open_with: &'static str,
    pub mode_executable: &'static str,

    pub section_recommended: &'static str,
    pub section_other: &'static str,
    pub section_all: &'static str,
    pub show_all: &'static str,
    pub hide_all: &'static str,
    pub search_placeholder: &'static str,

    pub open_once: &'static str,
    pub always_use: &'static str,
    pub cancel: &'static str,
    pub undo: &'static str,

    pub loading_title: &'static str,
    pub loading_detail: &'static str,
    pub empty_title: &'static str,
    pub empty_detail: &'static str,
    pub no_matches_title: &'static str,
    pub no_matches_detail: &'static str,
    pub nothing_selected: &'static str,

    pub badge_declares: &'static str,
    pub badge_related: &'static str,
    pub badge_wildcard: &'static str,
    pub badge_previously_used: &'static str,
    pub badge_user_associated: &'static str,
    pub badge_not_declared: &'static str,
    pub badge_default: &'static str,

    pub explain_related: &'static str,
    pub explain_wildcard: &'static str,
    pub explain_previously_used: &'static str,
    pub explain_user_associated: &'static str,
    pub explain_not_declared: &'static str,

    pub source_native: &'static str,
    pub source_flatpak: &'static str,
    pub source_snap: &'static str,
    pub source_appimage: &'static str,
    pub source_wrapper: &'static str,
    pub scope_user: &'static str,
    pub scope_system: &'static str,

    pub launch_failed: &'static str,
    pub launched: &'static str,
    pub association_written: &'static str,
    pub association_failed: &'static str,
    pub association_unchanged: &'static str,
    pub association_rolled_back: &'static str,
    pub warning_does_not_declare: &'static str,
    pub warning_removed_association: &'static str,
    pub warning_duplicate_key: &'static str,

    pub executable_use_path: &'static str,
    pub executable_resolved: &'static str,
    pub executable_no_single: &'static str,
    pub executable_dbus: &'static str,
    pub executable_not_found: &'static str,
    pub executable_no_exec: &'static str,
    pub executable_complex: &'static str,
    pub executable_browse: &'static str,
    pub executable_browse_hint: &'static str,
    pub executable_browse_empty: &'static str,
    pub executable_selected: &'static str,
}

const EN_US: Copy = Copy {
    open_with_title: "Open with",
    open_with_subtitle: "Choose the application to open this file.",
    executable_title: "Choose executable",
    executable_subtitle: "Pick a program file. Not every application has one.",
    mode_open_with: "Open with",
    mode_executable: "Choose executable",

    section_recommended: "Recommended",
    section_other: "Other compatible applications",
    section_all: "All applications",
    show_all: "Show all applications",
    hide_all: "Hide all applications",
    search_placeholder: "Search applications",

    open_once: "Open once",
    always_use: "Always use for this file type",
    cancel: "Cancel",
    undo: "Undo this change",

    loading_title: "Reading installed applications",
    loading_detail: "This runs off the render thread, so the window stays responsive.",
    empty_title: "No applications found",
    empty_detail: "No desktop application is registered on this system.",
    no_matches_title: "No matching applications",
    no_matches_detail: "Try a different search, or show all applications.",
    nothing_selected: "Select an application to continue.",

    badge_declares: "Supports this type",
    badge_related: "Supports a related type",
    badge_wildcard: "Supports this category",
    badge_previously_used: "Used before",
    badge_user_associated: "Your association",
    badge_not_declared: "Does not declare this type",
    badge_default: "Current default",

    explain_related: "This application declares a more general type, not this one exactly.",
    explain_wildcard: "This application declares a whole category rather than this type.",
    explain_previously_used: "You have opened this type with this application before, though it does not declare it.",
    explain_user_associated: "Your own associations list this application for this type; the application does not declare it.",
    explain_not_declared: "This application does not declare support for this file type. It may not open the file correctly.",

    source_native: "System package",
    source_flatpak: "Flatpak",
    source_snap: "Snap",
    source_appimage: "AppImage",
    source_wrapper: "Wrapper script",
    scope_user: "Installed for you",
    scope_system: "Installed for everyone",

    launch_failed: "Could not start the application.",
    launched: "Opened with the selected application.",
    association_written: "Saved as the default for this file type.",
    association_failed: "The default could not be saved. Nothing was changed.",
    association_unchanged: "This was already the default. Nothing was changed.",
    association_rolled_back: "The previous default was restored.",
    warning_does_not_declare: "This application does not declare this file type.",
    warning_removed_association: "Your file also lists this application as removed for this type, which may override the new default. That line was left alone.",
    warning_duplicate_key: "Your file lists this type more than once. Only the first line changed.",

    executable_use_path: "Use this path",
    executable_resolved: "Running this path is equivalent to launching the application.",
    executable_no_single: "This application has no single executable, so no path is offered.",
    executable_dbus: "This application starts over D-Bus, so there is no command to run.",
    executable_not_found: "The program this application names is not installed here.",
    executable_no_exec: "This application declares no command at all.",
    executable_complex: "Running the bare program would drop arguments this application depends on.",
    executable_browse: "Browse for a program",
    executable_browse_hint: "Standard program directories on this system.",
    executable_browse_empty: "No program directories are readable here.",
    executable_selected: "Selected executable",
};

const ZH_TW: Copy = Copy {
    open_with_title: "選擇開啟方式",
    open_with_subtitle: "選擇要用哪個應用程式開啟這個檔案。",
    executable_title: "選擇執行檔",
    executable_subtitle: "挑選一個程式檔案。不是每個應用程式都有。",
    mode_open_with: "開啟方式",
    mode_executable: "選擇執行檔",

    section_recommended: "建議的應用程式",
    section_other: "其他可用的應用程式",
    section_all: "所有應用程式",
    show_all: "顯示所有應用程式",
    hide_all: "收合所有應用程式",
    search_placeholder: "搜尋應用程式",

    open_once: "開啟一次",
    always_use: "一律用這個開啟這種檔案",
    cancel: "取消",
    undo: "復原這項變更",

    loading_title: "正在讀取已安裝的應用程式",
    loading_detail: "這項工作不在畫面執行緒上，所以視窗不會卡住。",
    empty_title: "找不到應用程式",
    empty_detail: "這台電腦沒有註冊任何桌面應用程式。",
    no_matches_title: "沒有符合的應用程式",
    no_matches_detail: "換個關鍵字，或顯示所有應用程式。",
    nothing_selected: "先選一個應用程式才能繼續。",

    badge_declares: "支援這種檔案",
    badge_related: "支援相近的類型",
    badge_wildcard: "支援這個大類",
    badge_previously_used: "用過",
    badge_user_associated: "你設定過的關聯",
    badge_not_declared: "沒有宣告這種檔案",
    badge_default: "目前的預設",

    explain_related: "這個應用程式宣告的是更大範圍的類型，不是這一種。",
    explain_wildcard: "這個應用程式宣告的是整個大類，不是這一種檔案。",
    explain_previously_used: "你以前用它開過這種檔案，但它並沒有宣告支援。",
    explain_user_associated: "你的關聯設定把它列給這種檔案，但應用程式本身沒有宣告支援。",
    explain_not_declared: "這個應用程式沒有宣告支援這種檔案，開啟後可能無法正常顯示。",

    source_native: "系統套件",
    source_flatpak: "Flatpak",
    source_snap: "Snap",
    source_appimage: "AppImage",
    source_wrapper: "包裝腳本",
    scope_user: "只裝給你",
    scope_system: "所有人都能用",

    launch_failed: "無法啟動應用程式。",
    launched: "已用選定的應用程式開啟。",
    association_written: "已設為這種檔案的預設應用程式。",
    association_failed: "無法儲存預設值，沒有任何變更。",
    association_unchanged: "本來就是預設值，沒有任何變更。",
    association_rolled_back: "已還原成先前的預設值。",
    warning_does_not_declare: "這個應用程式沒有宣告這種檔案類型。",
    warning_removed_association: "你的設定檔另外把這個應用程式列在移除清單裡，可能會蓋過新的預設值。那一行沒有被更動。",
    warning_duplicate_key: "你的設定檔重複列了這種檔案類型，只有第一行被更動。",

    executable_use_path: "使用這個路徑",
    executable_resolved: "執行這個路徑等同於啟動該應用程式。",
    executable_no_single: "這個應用程式沒有單一執行檔，所以不提供路徑。",
    executable_dbus: "這個應用程式透過 D-Bus 啟動，沒有可以執行的指令。",
    executable_not_found: "這台電腦上找不到它指定的程式。",
    executable_no_exec: "這個應用程式沒有宣告任何啟動指令。",
    executable_complex: "直接執行程式本身會漏掉它需要的參數。",
    executable_browse: "瀏覽程式檔案",
    executable_browse_hint: "這台電腦上的標準程式目錄。",
    executable_browse_empty: "沒有可以讀取的程式目錄。",
    executable_selected: "已選擇的執行檔",
};

pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}
