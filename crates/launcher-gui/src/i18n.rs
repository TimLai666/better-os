//! Localized wording for the launcher: one struct of strings per locale,
//! resolved from the environment unless a caller passes a locale in, and the
//! few phrases that carry a number or a name.

use crate::model::{Command, GroupKey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    System,
    EnUs,
    ZhTw,
}

impl Locale {
    pub fn resolved(self) -> Self {
        match self {
            Self::System => match better_ui::fluent::words::Locale::from_environment() {
                better_ui::fluent::words::Locale::TraditionalChinese => Self::ZhTw,
                better_ui::fluent::words::Locale::English => Self::EnUs,
            },
            locale => locale,
        }
    }

    /// The locale desktop-entry translations are resolved with, so an
    /// application's own localized name is what the user searches and sees.
    pub fn entry_locale(self) -> Option<app_catalog_core::Locale> {
        match self.resolved() {
            Self::ZhTw => app_catalog_core::Locale::parse("zh_TW"),
            _ => app_catalog_core::Locale::parse("en_US"),
        }
    }

    /// The shared words' locale.
    pub fn words(self) -> better_ui::fluent::words::Locale {
        match self.resolved() {
            Self::ZhTw => better_ui::fluent::words::Locale::TraditionalChinese,
            _ => better_ui::fluent::words::Locale::English,
        }
    }
}

pub struct Copy {
    pub search_placeholder: &'static str,

    pub pinned: &'static str,
    pub all_apps: &'static str,
    pub back: &'static str,
    pub recommended: &'static str,
    pub pinned_empty: &'static str,
    pub recommended_empty: &'static str,

    pub best_match: &'static str,
    pub apps: &'static str,
    pub files_and_folders: &'static str,
    pub kind_app: &'static str,
    pub kind_terminal_app: &'static str,
    pub kind_folder: &'static str,
    pub kind_file: &'static str,
    pub kind_calculation: &'static str,

    pub loading_title: &'static str,
    pub refreshing: &'static str,
    pub empty_library_title: &'static str,
    pub empty_library_detail: &'static str,
    pub no_matches_detail: &'static str,

    pub launch_failed: &'static str,
    pub launch_failed_advice: &'static str,
    pub not_saved: &'static str,

    pub open: &'static str,
    pub run_in_terminal: &'static str,
    pub pin_to_start: &'static str,
    pub unpin_from_start: &'static str,
    pub move_to_front: &'static str,
    pub remove_from_list: &'static str,
    pub open_file_location: &'static str,
    pub uninstall: &'static str,
    pub copy: &'static str,
    pub open_containing_folder: &'static str,

    pub letters_other: &'static str,
    pub letters_hint: &'static str,

    pub appearance: &'static str,
    pub appearance_dark: &'static str,
    pub appearance_light: &'static str,
    pub appearance_system: &'static str,
}

const EN_US: Copy = Copy {
    search_placeholder: "Search for apps, files, and sums",

    pinned: "Pinned",
    all_apps: "All apps",
    back: "Back",
    recommended: "Recommended",
    pinned_empty: "Nothing is pinned. Right-click an app in All apps and choose Pin to Start.",
    recommended_empty: "The apps you open will show up here.",

    best_match: "Best match",
    apps: "Apps",
    files_and_folders: "Files and folders",
    kind_app: "App",
    kind_terminal_app: "App that runs in a terminal",
    kind_folder: "Folder",
    kind_file: "File",
    kind_calculation: "Calculator",

    loading_title: "Getting your apps",
    refreshing: "Apps changed. Updating the list.",
    empty_library_title: "No apps found",
    empty_library_detail: "No app is installed for this desktop.",
    no_matches_detail: "Check the spelling, or try a shorter word.",

    launch_failed: "That app didn't open.",
    launch_failed_advice: "It may have been removed. Try again, or open it another way.",
    not_saved: "Your Start layout couldn't be saved, so this change lasts until Better Launcher closes.",

    open: "Open",
    run_in_terminal: "Run in terminal",
    pin_to_start: "Pin to Start",
    unpin_from_start: "Unpin from Start",
    move_to_front: "Move to front",
    remove_from_list: "Remove from list",
    open_file_location: "Open file location",
    uninstall: "Uninstall",
    copy: "Copy",
    open_containing_folder: "Open folder location",

    letters_other: "Other",
    letters_hint: "Choose a letter to jump to it.",

    appearance: "Appearance",
    appearance_dark: "Dark",
    appearance_light: "Light",
    appearance_system: "Use system setting",
};

const ZH_TW: Copy = Copy {
    search_placeholder: "搜尋應用程式、檔案或算式",

    pinned: "已釘選",
    all_apps: "所有應用程式",
    back: "返回",
    recommended: "建議",
    pinned_empty: "還沒有釘選任何應用程式。在「所有應用程式」裡按右鍵，選「釘選到開始」。",
    recommended_empty: "你開過的應用程式會出現在這裡。",

    best_match: "最佳比對",
    apps: "應用程式",
    files_and_folders: "檔案和資料夾",
    kind_app: "應用程式",
    kind_terminal_app: "在終端機裡執行的應用程式",
    kind_folder: "資料夾",
    kind_file: "檔案",
    kind_calculation: "計算機",

    loading_title: "正在讀取應用程式",
    refreshing: "應用程式有變動，正在更新清單。",
    empty_library_title: "找不到應用程式",
    empty_library_detail: "這個桌面沒有安裝任何應用程式。",
    no_matches_detail: "檢查一下拼字，或換個短一點的關鍵字。",

    launch_failed: "應用程式沒有開起來。",
    launch_failed_advice: "它可能已經被移除。再試一次，或用其他方式開啟。",
    not_saved: "無法儲存開始畫面的排列，這次的變更只會保留到關閉 Better Launcher 為止。",

    open: "開啟",
    run_in_terminal: "在終端機執行",
    pin_to_start: "釘選到開始",
    unpin_from_start: "從開始取消釘選",
    move_to_front: "移到最前面",
    remove_from_list: "從清單移除",
    open_file_location: "開啟檔案位置",
    uninstall: "解除安裝",
    copy: "複製",
    open_containing_folder: "開啟所在資料夾",

    letters_other: "其他",
    letters_hint: "選一個字母，直接跳到那一段。",

    appearance: "外觀",
    appearance_dark: "深色",
    appearance_light: "淺色",
    appearance_system: "跟隨系統",
};

pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}

/// What a command is called, in a menu and beside a search result.
pub fn command_label(locale: Locale, command: &Command) -> &'static str {
    let c = copy(locale);
    match command {
        Command::Open | Command::OpenPath => c.open,
        Command::RunInTerminal => c.run_in_terminal,
        Command::PinToStart => c.pin_to_start,
        Command::UnpinFromStart => c.unpin_from_start,
        Command::MoveToFront => c.move_to_front,
        Command::RemoveFromList => c.remove_from_list,
        Command::OpenFileLocation => c.open_file_location,
        Command::Uninstall(_) => c.uninstall,
        Command::CopyResult => c.copy,
        Command::OpenContainingFolder => c.open_containing_folder,
    }
}

/// The line under "Uninstall": where the removal happens.
pub fn uninstall_detail(locale: Locale, component: &str) -> String {
    match locale.resolved() {
        Locale::ZhTw => format!("會開啟 Better Manager 來移除 {component}"),
        _ => format!("Opens Better Manager to remove {component}"),
    }
}

/// The empty-result title, naming what was typed.
pub fn no_matches(locale: Locale, query: &str) -> String {
    let query = query.trim();
    match locale.resolved() {
        Locale::ZhTw => format!("找不到「{query}」的結果"),
        _ => format!("No results for \u{201c}{query}\u{201d}"),
    }
}

/// The notice after a calculation was copied.
pub fn copied(locale: Locale, value: &str) -> String {
    match locale.resolved() {
        Locale::ZhTw => format!("已複製 {value}。"),
        _ => format!("Copied {value}."),
    }
}

/// A letter header's label: `#`, the letter, or "Other".
pub fn group_label(locale: Locale, key: GroupKey) -> String {
    match key {
        GroupKey::Symbol => "#".to_string(),
        GroupKey::Letter(letter) => letter.to_string(),
        GroupKey::Other => copy(locale).letters_other.to_string(),
    }
}

/// When an app was last opened, the way Windows 11's Recommended says it.
pub fn when(locale: Locale, now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then);
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let zh = locale.resolved() == Locale::ZhTw;
    match (days, hours, minutes) {
        (0, 0, 0) => if zh { "剛剛" } else { "Just now" }.to_string(),
        (0, 0, 1) if !zh => "1 minute ago".to_string(),
        (0, 0, m) if zh => format!("{m} 分鐘前"),
        (0, 0, m) => format!("{m} minutes ago"),
        (0, 1, _) if !zh => "1 hour ago".to_string(),
        (0, h, _) if zh => format!("{h} 小時前"),
        (0, h, _) => format!("{h} hours ago"),
        (1, _, _) => if zh { "昨天" } else { "Yesterday" }.to_string(),
        (d, _, _) if zh => format!("{d} 天前"),
        (d, _, _) => format!("{d} days ago"),
    }
}
