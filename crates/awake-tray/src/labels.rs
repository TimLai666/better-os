//! Tray wording in `zh-TW` and `en-US`.
//!
//! Issue #13 fixes the Traditional Chinese tray strings, so they are written
//! here exactly as the issue gives them and every menu is built from this table
//! rather than from literals scattered through the layout code. Panel menus are
//! narrow, so the wording here is the short form; the full window may say more.

/// The locale rule is shared with the service, which words the low-battery
/// notification itself when no tray is running.
pub use awake_ipc::notification::Locale;

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
    /// The lid choice, in the window's words (ticket 109).
    pub keep_awake_lid_closed: &'static str,
    /// The lid choice while the service cannot give it (one from before
    /// protocol version 3 that has not restarted yet).
    pub keep_awake_lid_closed_unavailable: &'static str,
    pub lid_unavailable: &'static str,
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
    /// The battery row on a machine with no battery.
    pub battery_not_applicable: &'static str,
    /// The lid row of "Current settings", in the window's words.
    pub lid_sleep: &'static str,
    pub lid_battery_limit: &'static str,
    /// `{limit}` is the limit, worded by `awake_ipc::notification::limit_words`.
    pub lid_stops_after: &'static str,
    pub lid_never_stops: &'static str,
    /// While the closed-lid clock runs: `{time}` and `{remaining}`.
    pub lid_stops_at: &'static str,
    pub extend_session: &'static str,
    pub change_session: &'static str,
    pub end_session: &'static str,
    pub automatic_rules: &'static str,
    pub pause_automatic_rules: &'static str,
    pub pause_15_minutes: &'static str,
    pub pause_1_hour: &'static str,
    pub pause_until_resumed: &'static str,
    pub resume_automatic_rules: &'static str,
    /// The unarmed override control. Activating it only arms; see
    /// [`crate::menu::OverrideConfirmation`] for why.
    pub override_all_rules: &'static str,
    /// The armed override control. Activating *this* is what switches every
    /// rule off, so it reads as a confirmation rather than as a command.
    pub confirm_override_all_rules: &'static str,
    /// The four states the `Automatic rules` line can report.
    pub rules_on: &'static str,
    pub rules_paused_until: &'static str,
    pub rules_paused_until_resumed: &'static str,
    pub rules_overridden: &'static str,
    /// Rules exist but every one of them is switched off.
    pub rules_off: &'static str,
    /// No rule has been created yet, which is not the same as all of them being
    /// switched off.
    pub no_rules_yet: &'static str,
    /// Rules are running and none of them currently matches.
    pub no_rules_match: &'static str,
    /// Ends the manual session only. Shown when a rule is also holding a
    /// session, so it is clear what survives.
    pub end_manual_session_keep_rules: &'static str,
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
    /// Offered while a manual session runs: the lengths to replace it with.
    pub change_length: &'static str,
    /// The submenu holding what the running session holds back.
    pub current_settings: &'static str,
    /// The title of the notification a refused menu action raises.
    pub action_failed: &'static str,
    /// The bodies of that notification, one per reason, in plain words.
    pub failed_service_unavailable: &'static str,
    pub failed_backend_unavailable: &'static str,
    pub failed_no_manual_session: &'static str,
    /// The session a change or an extension named is no longer running.
    pub failed_session_gone: &'static str,
    pub failed_cannot_extend: &'static str,
    pub failed_end_in_the_past: &'static str,
    pub failed_too_many_sessions: &'static str,
    pub failed_other: &'static str,
}

pub const ZH_TW: Labels = Labels {
    application_name: awake_ipc::notification::ZH_TW.application_name,
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
    keep_awake_lid_closed: "闔上螢幕時也保持清醒",
    keep_awake_lid_closed_unavailable: "闔上螢幕時也保持清醒：目前無法使用",
    lid_unavailable: "目前無法使用",
    reason: "原因",
    remaining: "剩餘",
    started: "開始於",
    until_ended: "直到手動結束",
    system_sleep: "系統睡眠",
    display_sleep: "螢幕休眠",
    automatic_lock: "自動鎖定",
    battery_protection: "電量停止門檻",
    prevented: "已擋下",
    allowed: "維持原本行為",
    not_supported: "有要求，但後端無法提供",
    battery_stops_at: "低於 {percent}% 時停止",
    battery_off: "不因電量停止",
    battery_not_applicable: "這台機器不適用",
    lid_sleep: "闔上螢幕時睡眠",
    lid_battery_limit: "闔上螢幕並使用電池時",
    lid_stops_after: "{limit}後停止",
    lid_never_stops: "不會因此停止",
    lid_stops_at: "{time} 停止（還有 {remaining}）",
    extend_session: "延長工作階段",
    change_session: "工作階段設定…",
    end_session: "結束工作階段",
    automatic_rules: "自動規則",
    pause_automatic_rules: "暫停自動規則",
    pause_15_minutes: "暫停 15 分鐘",
    pause_1_hour: "暫停 1 小時",
    pause_until_resumed: "暫停到手動恢復",
    resume_automatic_rules: "恢復自動規則",
    override_all_rules: "覆寫所有自動規則",
    confirm_override_all_rules: "再點一次確認覆寫",
    rules_on: "已啟用",
    rules_paused_until: "暫停到 {time}",
    rules_paused_until_resumed: "已暫停",
    rules_overridden: "已覆寫",
    rules_off: "全部關閉",
    no_rules_yet: "尚未建立規則",
    no_rules_match: "目前沒有符合的規則",
    end_manual_session_keep_rules: "結束工作階段，保留規則",
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
    change_length: "改成其他長度",
    current_settings: "目前的設定",
    action_failed: "保持清醒沒有完成這個動作",
    failed_service_unavailable: "保持清醒服務沒有回應。請稍後再試一次。",
    failed_backend_unavailable: "這台電腦目前無法保持清醒，所以沒有開始。",
    failed_no_manual_session: "目前沒有手動開始的工作階段可以結束。",
    failed_session_gone: "這段工作階段已經結束了，請重新開始一段。",
    failed_cannot_extend: "這段工作階段沒有結束時間，不需要延長。",
    failed_end_in_the_past: "選的時間已經過了，請改選其他長度。",
    failed_too_many_sessions: "同時進行的工作階段太多了，請先結束一些。",
    failed_other: "服務沒有接受這個要求，工作階段維持原狀。",
};

pub const EN_US: Labels = Labels {
    application_name: awake_ipc::notification::EN_US.application_name,
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
    keep_awake_lid_closed: "Also when the lid is closed",
    keep_awake_lid_closed_unavailable: "Also when the lid is closed: not available right now",
    lid_unavailable: "Not available right now",
    reason: "Reason",
    remaining: "Remaining",
    started: "Started",
    until_ended: "Until ended",
    system_sleep: "System sleep",
    display_sleep: "Display sleep",
    automatic_lock: "Automatic lock",
    battery_protection: "Battery stop threshold",
    prevented: "Held off",
    allowed: "Allowed",
    not_supported: "Asked for, not delivered by the backend",
    battery_stops_at: "Stops below {percent}%",
    battery_off: "Never stops for battery level",
    battery_not_applicable: "Not applicable on this machine",
    lid_sleep: "Sleep when the lid is closed",
    lid_battery_limit: "Lid closed on battery",
    lid_stops_after: "Stops after {limit}",
    lid_never_stops: "Never stops for this",
    lid_stops_at: "Stops at {time} ({remaining} left)",
    extend_session: "Extend session",
    change_session: "Session settings…",
    end_session: "End session",
    automatic_rules: "Automatic rules",
    pause_automatic_rules: "Pause automatic rules",
    pause_15_minutes: "Pause for 15 minutes",
    pause_1_hour: "Pause for 1 hour",
    pause_until_resumed: "Pause until resumed",
    resume_automatic_rules: "Resume automatic rules",
    override_all_rules: "Override all rules",
    confirm_override_all_rules: "Click again to override all",
    rules_on: "On",
    rules_paused_until: "Paused until {time}",
    rules_paused_until_resumed: "Paused",
    rules_overridden: "Overridden",
    rules_off: "Off",
    no_rules_yet: "No rules yet",
    no_rules_match: "No rules match right now",
    end_manual_session_keep_rules: "End session, keep rules",
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
    change_length: "Change length",
    current_settings: "Current settings",
    action_failed: "Better Awake could not do that",
    failed_service_unavailable: "The Better Awake service is not answering. Try again in a moment.",
    failed_backend_unavailable: "This computer cannot be kept awake right now, so nothing was started.",
    failed_no_manual_session: "There is no session you started to end.",
    failed_session_gone: "That session has already ended. Start a new one.",
    failed_cannot_extend: "This session has no end time, so there is nothing to extend.",
    failed_end_in_the_past: "That time has already passed. Choose another length.",
    failed_too_many_sessions: "Too many sessions are running at once. End some first.",
    failed_other: "The service did not accept that, so the session is unchanged.",
};

/// The tray's wording for a locale. A trait because [`Locale`] is defined where
/// the service can share it.
pub trait LocaleLabels {
    fn labels(self) -> &'static Labels;
}

impl LocaleLabels for Locale {
    fn labels(self) -> &'static Labels {
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
                labels.pause_15_minutes,
                labels.pause_1_hour,
                labels.pause_until_resumed,
                labels.resume_automatic_rules,
                labels.override_all_rules,
                labels.confirm_override_all_rules,
                labels.rules_on,
                labels.rules_paused_until,
                labels.rules_paused_until_resumed,
                labels.rules_overridden,
                labels.rules_off,
                labels.no_rules_yet,
                labels.no_rules_match,
                labels.end_manual_session_keep_rules,
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

    #[test]
    fn every_automatic_rule_label_is_translated_in_both_locales() {
        // The struct makes a missing field a compile error; this catches the
        // other half, a field filled in with an empty string or left as the
        // English wording in the zh-TW table.
        for locale in [Locale::ZhTw, Locale::EnUs] {
            let labels = locale.labels();
            for (name, label) in [
                ("pause_15_minutes", labels.pause_15_minutes),
                ("pause_1_hour", labels.pause_1_hour),
                ("pause_until_resumed", labels.pause_until_resumed),
                ("resume_automatic_rules", labels.resume_automatic_rules),
                ("override_all_rules", labels.override_all_rules),
                (
                    "confirm_override_all_rules",
                    labels.confirm_override_all_rules,
                ),
                ("rules_on", labels.rules_on),
                ("rules_paused_until", labels.rules_paused_until),
                (
                    "rules_paused_until_resumed",
                    labels.rules_paused_until_resumed,
                ),
                ("rules_overridden", labels.rules_overridden),
                ("rules_off", labels.rules_off),
                ("no_rules_yet", labels.no_rules_yet),
                ("no_rules_match", labels.no_rules_match),
                (
                    "end_manual_session_keep_rules",
                    labels.end_manual_session_keep_rules,
                ),
            ] {
                assert!(
                    !label.trim().is_empty(),
                    "{} has no wording for {name}",
                    locale.tag()
                );
                assert!(
                    label.chars().count() <= 32,
                    "{} {name} is too long for a panel menu: {label}",
                    locale.tag()
                );
            }
        }

        assert_ne!(
            ZH_TW.override_all_rules, EN_US.override_all_rules,
            "the zh-TW table must be translated, not copied"
        );
        assert!(
            ZH_TW.rules_paused_until.contains("{time}")
                && EN_US.rules_paused_until.contains("{time}"),
            "both locales must keep the placeholder the menu substitutes"
        );
    }

    #[test]
    fn the_wording_ticket_102_adds_is_short_and_translated() {
        for locale in [Locale::ZhTw, Locale::EnUs] {
            let labels = locale.labels();
            for label in [labels.change_length, labels.current_settings] {
                assert!(!label.trim().is_empty());
                assert!(label.chars().count() <= 32);
            }
            for body in [
                labels.action_failed,
                labels.failed_service_unavailable,
                labels.failed_backend_unavailable,
                labels.failed_no_manual_session,
                labels.failed_session_gone,
                labels.failed_cannot_extend,
                labels.failed_end_in_the_past,
                labels.failed_too_many_sessions,
                labels.failed_other,
            ] {
                assert!(!body.trim().is_empty());
                // Plain words, never the error key the service answered with.
                assert!(!body.contains("awake."), "{body}");
            }
        }
        assert_ne!(ZH_TW.change_length, EN_US.change_length);
        assert_ne!(ZH_TW.current_settings, EN_US.current_settings);
        assert_ne!(ZH_TW.failed_other, EN_US.failed_other);
    }

    #[test]
    fn the_lid_rows_use_the_windows_words() {
        // Copied from awake-gui's i18n, like the rows below.
        assert_eq!(
            [
                ZH_TW.keep_awake_lid_closed,
                ZH_TW.lid_sleep,
                ZH_TW.lid_battery_limit,
                ZH_TW.lid_stops_after,
                ZH_TW.lid_never_stops,
            ],
            [
                "闔上螢幕時也保持清醒",
                "闔上螢幕時睡眠",
                "闔上螢幕並使用電池時",
                "{limit}後停止",
                "不會因此停止",
            ]
        );
        assert_eq!(
            [
                EN_US.keep_awake_lid_closed,
                EN_US.lid_sleep,
                EN_US.lid_battery_limit,
                EN_US.lid_stops_after,
                EN_US.lid_never_stops,
            ],
            [
                "Also when the lid is closed",
                "Sleep when the lid is closed",
                "Lid closed on battery",
                "Stops after {limit}",
                "Never stops for this",
            ]
        );
    }

    #[test]
    fn the_current_settings_rows_use_the_windows_words() {
        // Copied from awake-gui's i18n (effective policy and battery rows), so
        // the tray and the window say the same thing about the same session.
        for (labels, words) in [
            (
                &ZH_TW,
                [
                    "系統睡眠",
                    "螢幕休眠",
                    "自動鎖定",
                    "已擋下",
                    "維持原本行為",
                    "有要求，但後端無法提供",
                    "電量停止門檻",
                    "低於 {percent}% 時停止",
                    "不因電量停止",
                    "這台機器不適用",
                ],
            ),
            (
                &EN_US,
                [
                    "System sleep",
                    "Display sleep",
                    "Automatic lock",
                    "Held off",
                    "Allowed",
                    "Asked for, not delivered by the backend",
                    "Battery stop threshold",
                    "Stops below {percent}%",
                    "Never stops for battery level",
                    "Not applicable on this machine",
                ],
            ),
        ] {
            assert_eq!(
                [
                    labels.system_sleep,
                    labels.display_sleep,
                    labels.automatic_lock,
                    labels.prevented,
                    labels.allowed,
                    labels.not_supported,
                    labels.battery_protection,
                    labels.battery_stops_at,
                    labels.battery_off,
                    labels.battery_not_applicable,
                ],
                words
            );
        }
    }
}
