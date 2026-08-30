//! Localized wording for the overlay, following the manager and chooser
//! approach: one struct of strings per locale, resolved from the environment
//! unless a caller passes a locale in.

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

    /// The locale desktop-entry translations are resolved with, so an
    /// application's own localized name is what the user searches and sees.
    pub fn entry_locale(self) -> Option<app_catalog_core::Locale> {
        match self.resolved() {
            Self::ZhTw => app_catalog_core::Locale::parse("zh_TW"),
            _ => app_catalog_core::Locale::parse("en_US"),
        }
    }
}

pub struct Copy {
    pub search_placeholder: &'static str,

    pub loading_title: &'static str,
    pub loading_detail: &'static str,
    pub refreshing: &'static str,

    pub empty_library_title: &'static str,
    pub empty_library_detail: &'static str,
    pub no_matches_title: &'static str,
    pub no_matches_detail: &'static str,

    pub launch_failed: &'static str,

    pub hint_navigate: &'static str,
    pub hint_launch: &'static str,
    pub hint_close: &'static str,

    pub library_count: &'static str,
    pub result_count: &'static str,
}

const EN_US: Copy = Copy {
    search_placeholder: "Search applications",

    loading_title: "Reading installed applications",
    loading_detail: "This runs off the render thread, so the overlay stays responsive.",
    refreshing: "Applications changed. Updating the list.",

    empty_library_title: "No applications found",
    empty_library_detail: "No desktop application is registered on this system.",
    no_matches_title: "No matching applications",
    no_matches_detail: "Clear the search row to return to the application library.",

    launch_failed: "Could not start the application.",

    hint_navigate: "Arrow keys to move",
    hint_launch: "Enter to open",
    hint_close: "Esc to close",

    library_count: "applications",
    result_count: "results",
};

const ZH_TW: Copy = Copy {
    search_placeholder: "搜尋應用程式",

    loading_title: "正在讀取已安裝的應用程式",
    loading_detail: "這項工作不在畫面執行緒上，所以畫面不會卡住。",
    refreshing: "應用程式有變動，正在更新清單。",

    empty_library_title: "找不到應用程式",
    empty_library_detail: "這台電腦沒有註冊任何桌面應用程式。",
    no_matches_title: "沒有符合的應用程式",
    no_matches_detail: "清空搜尋列就會回到應用程式清單。",

    launch_failed: "無法啟動應用程式。",

    hint_navigate: "方向鍵移動",
    hint_launch: "Enter 開啟",
    hint_close: "Esc 關閉",

    library_count: "個應用程式",
    result_count: "個結果",
};

pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}
