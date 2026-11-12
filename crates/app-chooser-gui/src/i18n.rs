//! Localized wording for the chooser: one struct of strings per locale,
//! resolved from the environment unless the embedding surface passes a
//! locale in, and the sentences that carry the file's name.
//!
//! Every word here is one a person reads. No MIME type, no desktop file
//! name, no packaging format: the reason an app is offered is a plain
//! sentence, and only when someone asks for it.

use crate::{FileKind, Reason};

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

    /// The locale to resolve desktop-entry translations with, so an
    /// application's own localized name is used rather than its default one.
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
    pub open_with_title: &'static str,
    pub executable_title: &'static str,
    pub executable_subtitle: &'static str,

    pub search_placeholder: &'static str,
    pub other_apps: &'static str,
    pub current_default: &'static str,
    pub why: &'static str,

    pub open: &'static str,
    pub cancel: &'static str,
    pub undo: &'static str,

    pub loading_title: &'static str,
    pub empty_title: &'static str,
    pub empty_detail: &'static str,
    pub no_matches_title: &'static str,
    pub no_matches_detail: &'static str,

    pub reason_default: &'static str,
    pub reason_declares: &'static str,
    pub reason_related: &'static str,
    pub reason_wildcard: &'static str,
    pub reason_user_associated: &'static str,
    pub reason_previously_used: &'static str,
    pub reason_not_declared: &'static str,

    pub launch_failed: &'static str,
    pub launch_failed_advice: &'static str,
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
    pub executable_browse_empty: &'static str,
    pub executable_selected: &'static str,
}

const EN_US: Copy = Copy {
    open_with_title: "Open with",
    executable_title: "Choose a program",
    executable_subtitle: "Pick the program file to run. Not every app has one.",

    search_placeholder: "Search apps",
    other_apps: "Other apps",
    current_default: "Opens this kind of file now",
    why: "Why is this app here?",

    open: "Open",
    cancel: "Cancel",
    undo: "Undo",

    loading_title: "Getting your apps",
    empty_title: "No apps found",
    empty_detail: "No app is installed for this desktop.",
    no_matches_title: "No matching apps",
    no_matches_detail: "Try a different word.",

    reason_default: "This is the app that opens this kind of file now.",
    reason_declares: "This app says it can open this kind of file.",
    reason_related: "This app opens a broader kind of file that includes this one.",
    reason_wildcard: "This app opens files of this general kind, such as any text or any image.",
    reason_user_associated: "You chose this app for this kind of file before.",
    reason_previously_used: "You opened this kind of file with this app before.",
    reason_not_declared: "This app doesn't say it can open this kind of file, so it might not open it correctly.",

    launch_failed: "That app didn't open.",
    launch_failed_advice: "Try another app, or try again.",
    association_written: "From now on, this kind of file opens with this app.",
    association_failed: "Your choice couldn't be saved. Nothing was changed.",
    association_unchanged: "This app already opens this kind of file.",
    association_rolled_back: "The app that opened this kind of file before is back.",
    warning_does_not_declare: "This app doesn't say it can open this kind of file.",
    warning_removed_association: "An older setting of yours hides this app for this kind of file and may win over your new choice.",
    warning_duplicate_key: "Your settings list this kind of file twice; only the first was changed.",

    executable_use_path: "Use this program",
    executable_resolved: "Running this file starts the app.",
    executable_no_single: "This app has no single program file to pick.",
    executable_dbus: "This app is started by the desktop, so there is no program file to run.",
    executable_not_found: "The program this app names isn't installed here.",
    executable_no_exec: "This app names no program to run.",
    executable_complex: "Running the program on its own would leave out settings this app needs.",
    executable_browse: "Browse for a program",
    executable_browse_empty: "No program folder can be read here.",
    executable_selected: "Chosen program",
};

const ZH_TW: Copy = Copy {
    open_with_title: "開啟方式",
    executable_title: "選擇程式",
    executable_subtitle: "選一個要執行的程式檔。不是每個應用程式都有。",

    search_placeholder: "搜尋應用程式",
    other_apps: "其他應用程式",
    current_default: "目前用它開啟這種檔案",
    why: "為什麼會列出這個應用程式？",

    open: "開啟",
    cancel: "取消",
    undo: "復原",

    loading_title: "正在讀取應用程式",
    empty_title: "找不到應用程式",
    empty_detail: "這個桌面沒有安裝任何應用程式。",
    no_matches_title: "沒有符合的應用程式",
    no_matches_detail: "換個關鍵字試試看。",

    reason_default: "目前就是用這個應用程式開啟這種檔案。",
    reason_declares: "這個應用程式表示自己能開啟這種檔案。",
    reason_related: "這個應用程式能開啟範圍更大的一類檔案，這種檔案也包含在內。",
    reason_wildcard: "這個應用程式能開啟這一大類檔案，例如所有文字檔或所有圖片。",
    reason_user_associated: "你以前曾指定用這個應用程式開啟這種檔案。",
    reason_previously_used: "你以前用這個應用程式開過這種檔案。",
    reason_not_declared: "這個應用程式沒有表示能開啟這種檔案，開起來可能不正常。",

    launch_failed: "應用程式沒有開起來。",
    launch_failed_advice: "換一個應用程式，或再試一次。",
    association_written: "以後這種檔案都會用這個應用程式開啟。",
    association_failed: "無法儲存你的選擇，沒有任何變更。",
    association_unchanged: "本來就是用這個應用程式開啟這種檔案。",
    association_rolled_back: "已改回原本開啟這種檔案的應用程式。",
    warning_does_not_declare: "這個應用程式沒有表示能開啟這種檔案。",
    warning_removed_association: "你有一項舊設定把這個應用程式從這種檔案隱藏起來，可能會蓋過新的選擇。",
    warning_duplicate_key: "你的設定重複列了這種檔案，只改了第一筆。",

    executable_use_path: "使用這個程式",
    executable_resolved: "執行這個檔案就會開啟這個應用程式。",
    executable_no_single: "這個應用程式沒有單一的程式檔可選。",
    executable_dbus: "這個應用程式由桌面啟動，沒有可以直接執行的程式檔。",
    executable_not_found: "這台電腦沒有安裝它指定的程式。",
    executable_no_exec: "這個應用程式沒有指定要執行的程式。",
    executable_complex: "單獨執行這個程式會漏掉應用程式需要的設定。",
    executable_browse: "瀏覽程式",
    executable_browse_empty: "這裡沒有可以讀取的程式資料夾。",
    executable_selected: "選擇的程式",
};

pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}

/// The plain sentence for why an app is in the list.
pub fn reason_text(locale: Locale, reason: Reason) -> &'static str {
    let c = copy(locale);
    match reason {
        Reason::CurrentDefault => c.reason_default,
        Reason::Declares => c.reason_declares,
        Reason::Related => c.reason_related,
        Reason::Wildcard => c.reason_wildcard,
        Reason::UserAssociated => c.reason_user_associated,
        Reason::PreviouslyUsed => c.reason_previously_used,
        Reason::NotDeclared => c.reason_not_declared,
    }
}

/// The dialog's title, naming the file.
pub fn select_title(locale: Locale, file: &str) -> String {
    match locale.resolved() {
        Locale::ZhTw => format!("選擇要用來開啟「{file}」的應用程式"),
        _ => format!("Select an app to open \u{201c}{file}\u{201d}"),
    }
}

/// The check box under the list. It names the kind of file the default is
/// written for, which is the type, not merely the last extension: an
/// `a.tar.gz` is a compressed tar archive, not a `.gz` file.
pub fn always_use(locale: Locale, kind: &FileKind) -> String {
    match (locale.resolved(), kind) {
        (Locale::ZhTw, FileKind::Described(text)) => format!("一律使用這個應用程式開啟「{text}」"),
        (Locale::ZhTw, FileKind::Extension(extension)) => {
            format!("一律使用這個應用程式開啟 {extension} 檔案")
        }
        (Locale::ZhTw, FileKind::Unknown) => "一律使用這個應用程式開啟這種檔案".to_string(),
        (_, FileKind::Described(text)) => format!("Always use this app for {text}"),
        (_, FileKind::Extension(extension)) => {
            format!("Always use this app to open {extension} files")
        }
        (_, FileKind::Unknown) => "Always use this app for this kind of file".to_string(),
    }
}

/// The shared-mime-info language this locale reads descriptions in: the
/// untranslated (English) one, or zh_TW.
pub fn mime_language(locale: Locale) -> Option<&'static str> {
    match locale.resolved() {
        Locale::ZhTw => Some("zh_TW"),
        _ => None,
    }
}

/// The "Other apps" row, with how many it holds.
pub fn other_apps(locale: Locale, count: usize) -> String {
    format!("{} ({count})", copy(locale).other_apps)
}
