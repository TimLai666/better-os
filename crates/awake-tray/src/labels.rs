//! Tray wording in `zh-TW` and `en-US`.
//!
//! Issue #13 fixes the Traditional Chinese tray strings, so they are written
//! here exactly as the issue gives them and every menu is built from this table
//! rather than from literals scattered through the layout code. Panel menus are
//! narrow, so the wording here is the short form; the full window may say more.

/// The two locales Phase 1 ships.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    ZhTw,
    EnUs,
}

impl Locale {
    /// Reads the user's locale the way every other POSIX program does.
    /// Anything that is not Traditional Chinese falls back to `en-US`, which is
    /// a fallback, not a guess about the user.
    pub fn from_environment() -> Self {
        let value = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|name| std::env::var(name).ok())
            .unwrap_or_default();
        Self::from_tag(&value)
    }

    pub fn from_tag(tag: &str) -> Self {
        let tag = tag.replace('-', "_").to_ascii_lowercase();
        if tag.starts_with("zh_tw") || tag.starts_with("zh_hant") || tag.starts_with("zh_hk") {
            Locale::ZhTw
        } else {
            Locale::EnUs
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Locale::ZhTw => "zh-TW",
            Locale::EnUs => "en-US",
        }
    }
}

/// Every string the tray can show. A struct rather than a lookup by key, so a
/// missing translation is a compile error instead of a menu entry that silently
/// falls back to English.
#[derive(Clone, Copy, Debug)]
pub struct Labels {
    pub application_name: &'static str,
    pub inactive_summary: &'static str,
    pub active_summary: &'static str,
    pub start_a_session: &'static str,
    pub indefinitely: &'static str,
    pub minutes_15: &'static str,
    pub minutes_30: &'static str,
    pub hour_1: &'static str,
    pub hours_2: &'static str,
    pub until_a_time: &'static str,
    pub quick_options: &'static str,
    pub allow_display_off: &'static str,
    pub stop_below_battery: &'static str,
    pub reason: &'static str,
    pub remaining: &'static str,
    pub started: &'static str,
    pub until_ended: &'static str,
    pub system_sleep: &'static str,
    pub display_sleep: &'static str,
    pub automatic_lock: &'static str,
    pub battery_protection: &'static str,
    pub prevented: &'static str,
    pub allowed: &'static str,
    pub not_supported: &'static str,
    pub battery_stops_at: &'static str,
    pub battery_off: &'static str,
    pub extend_session: &'static str,
    pub change_session: &'static str,
    pub end_session: &'static str,
    pub automatic_rules: &'static str,
    pub pause_automatic_rules: &'static str,
    pub not_available_yet: &'static str,
    pub open_application: &'static str,
    pub quit: &'static str,
    pub attention: &'static str,
    pub backend_unavailable: &'static str,
    pub interrupted_previous_session: &'static str,
    /// Shown when a session asks for something the backend cannot hold.
    pub not_in_force: &'static str,
    pub hour_unit: &'static str,
    pub minute_unit: &'static str,
    pub second_unit: &'static str,
    pub active_reasons: &'static str,
    /// The reason a one-click tray session records. A preset carries no typed
    /// explanation, so it says where it came from rather than inventing one.
    pub tray_session_reason: &'static str,
    pub security_confirmation_needed: &'static str,
}

pub const ZH_TW: Labels = Labels {
    application_name: "保持清醒",
    inactive_summary: "目前未保持清醒",
    active_summary: "正在保持這台電腦清醒",
    start_a_session: "開始一段工作階段",
    indefinitely: "持續保持清醒",
    minutes_15: "15 分鐘",
    minutes_30: "30 分鐘",
    hour_1: "1 小時",
    hours_2: "2 小時",
    until_a_time: "直到指定時間",
    quick_options: "快速選項",
    allow_display_off: "允許螢幕關閉",
    stop_below_battery: "低於 20% 電量時停止",
    reason: "原因",
    remaining: "剩餘",
    started: "開始於",
    until_ended: "直到手動結束",
    system_sleep: "系統睡眠",
    display_sleep: "螢幕睡眠",
    automatic_lock: "自動鎖定",
    battery_protection: "電量保護",
    prevented: "已阻止",
    allowed: "允許",
    not_supported: "此環境不支援",
    battery_stops_at: "低於 {percent}% 時停止",
    battery_off: "未啟用",
    extend_session: "延長工作階段",
    change_session: "變更工作階段…",
    end_session: "結束工作階段",
    automatic_rules: "自動規則",
    pause_automatic_rules: "暫停自動規則",
    not_available_yet: "尚未提供",
    open_application: "開啟 Better Awake…",
    quit: "結束 Better Awake",
    attention: "需要處理",
    backend_unavailable: "此環境無法保持清醒",
    interrupted_previous_session: "上次的工作階段未正常結束",
    not_in_force: "未生效",
    hour_unit: "小時",
    minute_unit: "分鐘",
    second_unit: "秒",
    active_reasons: "{count} 個進行中的原因",
    tray_session_reason: "從系統匣開始的工作階段",
    security_confirmation_needed: "需要在主視窗確認",
};

pub const EN_US: Labels = Labels {
    application_name: "Better Awake",
    inactive_summary: "Not keeping this computer awake",
    active_summary: "Keeping this computer awake",
    start_a_session: "Start a session",
    indefinitely: "Indefinitely",
    minutes_15: "15 minutes",
    minutes_30: "30 minutes",
    hour_1: "1 hour",
    hours_2: "2 hours",
    until_a_time: "Until…",
    quick_options: "Quick options",
    allow_display_off: "Allow display to turn off",
    stop_below_battery: "Stop below 20% battery",
    reason: "Reason",
    remaining: "Remaining",
    started: "Started",
    until_ended: "Until ended",
    system_sleep: "System sleep",
    display_sleep: "Display sleep",
    automatic_lock: "Automatic lock",
    battery_protection: "Battery protection",
    prevented: "Prevented",
    allowed: "Allowed",
    not_supported: "Not supported here",
    battery_stops_at: "Stops at {percent}%",
    battery_off: "Off",
    extend_session: "Extend session",
    change_session: "Change session…",
    end_session: "End session",
    automatic_rules: "Automatic rules",
    pause_automatic_rules: "Pause automatic rules",
    not_available_yet: "Not available yet",
    open_application: "Open Better Awake…",
    quit: "Quit Better Awake",
    attention: "Needs attention",
    backend_unavailable: "No keep-awake support here",
    interrupted_previous_session: "Previous session ended abruptly",
    not_in_force: "Not in force",
    hour_unit: "hr",
    minute_unit: "min",
    second_unit: "sec",
    active_reasons: "{count} active reasons",
    tray_session_reason: "Started from the tray",
    security_confirmation_needed: "Confirm in the main window",
};

impl Locale {
    pub fn labels(self) -> &'static Labels {
        match self {
            Locale::ZhTw => &ZH_TW,
            Locale::EnUs => &EN_US,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traditional_chinese_locale_is_recognized_in_every_shape_it_arrives_in() {
        for tag in [
            "zh_TW.UTF-8",
            "zh-TW",
            "zh_Hant",
            "zh_HK.UTF-8",
            "ZH_tw.utf8",
        ] {
            assert_eq!(Locale::from_tag(tag), Locale::ZhTw, "{tag}");
        }
    }

    #[test]
    fn anything_else_falls_back_to_english() {
        for tag in ["en_US.UTF-8", "zh_CN.UTF-8", "", "C", "de_DE"] {
            assert_eq!(Locale::from_tag(tag), Locale::EnUs, "{tag}");
        }
    }

    #[test]
    fn the_wording_issue_13_fixes_is_the_wording_that_ships() {
        let labels = Locale::ZhTw.labels();
        assert_eq!(labels.application_name, "保持清醒");
        assert_eq!(labels.inactive_summary, "目前未保持清醒");
        assert_eq!(labels.start_a_session, "開始一段工作階段");
        assert_eq!(labels.indefinitely, "持續保持清醒");
        assert_eq!(labels.until_a_time, "直到指定時間");
        assert_eq!(labels.allow_display_off, "允許螢幕關閉");
        assert_eq!(labels.stop_below_battery, "低於 20% 電量時停止");
        assert_eq!(labels.extend_session, "延長工作階段");
        assert_eq!(labels.end_session, "結束工作階段");
        assert_eq!(labels.automatic_rules, "自動規則");
        assert_eq!(labels.pause_automatic_rules, "暫停自動規則");
        assert_eq!(labels.open_application, "開啟 Better Awake…");
    }

    #[test]
    fn no_tray_label_is_long_enough_to_be_clipped_by_a_panel_menu() {
        // A panel menu gets narrow; anything past this is a paragraph, not a
        // menu entry. Counted in characters, because the zh-TW wording is not
        // ASCII.
        for locale in [Locale::ZhTw, Locale::EnUs] {
            let labels = locale.labels();
            for label in [
                labels.application_name,
                labels.inactive_summary,
                labels.active_summary,
                labels.start_a_session,
                labels.indefinitely,
                labels.until_a_time,
                labels.quick_options,
                labels.allow_display_off,
                labels.stop_below_battery,
                labels.extend_session,
                labels.change_session,
                labels.end_session,
                labels.automatic_rules,
                labels.pause_automatic_rules,
                labels.open_application,
                labels.quit,
                labels.backend_unavailable,
                labels.interrupted_previous_session,
            ] {
                assert!(
                    label.chars().count() <= 32,
                    "{} label is too long for a panel menu: {label}",
                    locale.tag()
                );
            }
        }
    }
}
