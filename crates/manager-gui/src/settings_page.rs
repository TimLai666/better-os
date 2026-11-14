//! Settings (spec section 10): one appearance and one language for every
//! page, the launch update check, the update channel, and About. What the
//! page offers and which choice is selected is decided here, without GPUI.
//!
//! `auto_download`, `diagnostic_logs`, and the Preview channel are not
//! offered: nothing reads them (spec section 10). Their fields stay in the
//! stored settings, so a file this window writes still parses in an older
//! manager and an older file still parses here.

use std::path::Path;

use manager_core::StoredTheme;

use crate::i18n::{Locale, shell};
use crate::model::PROJECT_REPOSITORY;
use crate::navigation::Route;
use crate::pages::{PageModule, PageStatus};
use crate::search::SearchEntry;

/// The ids of the page's cards, which the search scrolls to.
pub(crate) mod card {
    pub(crate) const APPEARANCE: &str = "appearance";
    pub(crate) const LANGUAGE: &str = "language";
    pub(crate) const CHECK_UPDATES: &str = "check-updates";
    pub(crate) const UPDATE_CHANNEL: &str = "update-channel";
    pub(crate) const ABOUT: &str = "about";
}

pub(crate) const PAGE: PageModule = PageModule {
    route: Route::Settings,
    name: "settings",
    status: PageStatus::Built,
    title: |s| s.settings,
    search_entries,
    attention: |_| Vec::new(),
    badge: |_| None,
};

fn search_entries(locale: Locale) -> Vec<SearchEntry> {
    let s = shell(locale);
    let entry = |header, description, card| SearchEntry {
        header,
        description,
        page: s.settings,
        group: None,
        route: Route::Settings,
        card,
    };
    vec![
        entry(s.appearance, s.appearance_description, card::APPEARANCE),
        entry(s.language, s.language_description, card::LANGUAGE),
        entry(s.check_updates, "", card::CHECK_UPDATES),
        entry(
            s.update_channel,
            s.update_channel_description,
            card::UPDATE_CHANNEL,
        ),
        entry(s.about, s.privacy, card::ABOUT),
    ]
}

/// The appearance choices, the default first (ADR 0019).
pub(crate) const APPEARANCES: [StoredTheme; 3] = StoredTheme::ALL;

pub(crate) fn appearance_labels(locale: Locale) -> [&'static str; 3] {
    let s = shell(locale);
    APPEARANCES.map(|theme| match theme {
        StoredTheme::System => s.appearance_system,
        StoredTheme::Light => s.appearance_light,
        StoredTheme::Dark => s.appearance_dark,
    })
}

pub(crate) fn appearance_index(theme: StoredTheme) -> usize {
    APPEARANCES
        .iter()
        .position(|candidate| *candidate == theme)
        .unwrap_or(0)
}

/// The language choices, in the spec's order. Each language is named in
/// itself, the same on every page (audit A7).
pub(crate) const LANGUAGES: [Locale; 3] = [Locale::System, Locale::EnUs, Locale::ZhTw];

pub(crate) fn language_labels(locale: Locale) -> [&'static str; 3] {
    let s = shell(locale);
    [s.language_system, s.language_english, s.language_chinese]
}

pub(crate) fn language_index(locale: Locale) -> usize {
    LANGUAGES
        .iter()
        .position(|candidate| *candidate == locale)
        .unwrap_or(0)
}

/// Where the source code is.
pub(crate) const SOURCE_URL: &str = PROJECT_REPOSITORY;

/// Where the package puts the third-party license inventory
/// (`packaging/build-deb.sh`).
pub(crate) const LICENSES_FILE: &str = "/usr/share/doc/better-manager/THIRD-PARTY-LICENSES.md";

/// The same inventory in the repository, for a build that was not installed
/// from the package.
pub(crate) const LICENSES_URL: &str =
    "https://github.com/TimLai666/better-os/blob/main/docs/third-party-licenses.md";

/// What "Third-party licenses" opens: the file the package installed when it
/// is there, the repository's copy otherwise.
pub(crate) fn licenses_target(installed_file: &Path) -> String {
    if installed_file.is_file() {
        format!("file://{}", installed_file.display())
    } else {
        LICENSES_URL.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_is_the_system_light_or_dark_with_the_default_first() {
        assert_eq!(
            appearance_labels(Locale::ZhTw),
            ["跟隨系統", "淺色", "深色"]
        );
        assert_eq!(
            appearance_labels(Locale::EnUs),
            ["Use system setting", "Light", "Dark"]
        );
        assert_eq!(APPEARANCES[0], StoredTheme::default());
        for (index, theme) in APPEARANCES.iter().enumerate() {
            assert_eq!(appearance_index(*theme), index);
        }
    }

    #[test]
    fn language_names_each_language_in_itself() {
        assert_eq!(
            language_labels(Locale::ZhTw),
            ["跟隨系統", "English", "繁體中文"]
        );
        assert_eq!(
            language_labels(Locale::EnUs),
            ["Use system setting", "English", "繁體中文"]
        );
        for (index, locale) in LANGUAGES.iter().enumerate() {
            assert_eq!(language_index(*locale), index);
        }
    }

    /// The four dropped choices are not offered anywhere on the page.
    #[test]
    fn the_page_offers_no_choice_nothing_reads() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let headers: Vec<&str> = search_entries(locale)
                .iter()
                .map(|entry| entry.header)
                .collect();
            assert_eq!(headers.len(), 5);
            for dropped in [
                "自動下載更新",
                "保留失敗操作的診斷紀錄",
                "Download updates automatically",
                "Keep diagnostic logs",
            ] {
                assert!(!headers.contains(&dropped), "{dropped}");
            }
        }
        assert_eq!(
            search_entries(Locale::EnUs)
                .iter()
                .map(|entry| entry.header)
                .collect::<Vec<_>>(),
            [
                "Appearance",
                "Language",
                "Check for updates when opened",
                "Update channel",
                "About"
            ]
        );
        assert_eq!(
            search_entries(Locale::ZhTw)
                .iter()
                .map(|entry| entry.header)
                .collect::<Vec<_>>(),
            ["外觀", "語言", "開啟時檢查更新", "更新頻道", "關於"]
        );
    }

    #[test]
    fn the_licenses_link_opens_the_installed_file_or_the_repository_copy() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("THIRD-PARTY-LICENSES.md");
        assert_eq!(licenses_target(&file), LICENSES_URL);
        std::fs::write(&file, "# licenses").unwrap();
        assert_eq!(licenses_target(&file), format!("file://{}", file.display()));
        assert!(SOURCE_URL.starts_with("https://"));
    }

    /// An older state file that still has the dropped fields loads, and one
    /// written now keeps them for an older manager to read.
    #[test]
    fn the_dropped_fields_stay_in_the_store() {
        let settings = manager_core::ManagerSettings::default();
        let written = serde_json::to_value(&settings).unwrap();
        for field in ["auto_download", "diagnostic_logs", "release_channel"] {
            assert!(written.get(field).is_some(), "{field} is still written");
        }
        let read: manager_core::ManagerSettings = serde_json::from_value(written).unwrap();
        assert_eq!(read, settings);
    }
}
