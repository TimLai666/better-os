//! The login screen watch on Home and Health (ADR 0033, ticket 140), with no
//! GPUI dependency: the words, which record Home tells of, and what Health's
//! card lists. The records come from `manager_core::login_watch`; the window
//! only draws what this says.

use manager_core::login_watch::{
    Event, LastBootOutcome, Records, Report, RestartOutcome, Setting, Severity as WatchSeverity,
};

use crate::attention::{AttentionAction, AttentionItem, Severity, Snapshot, row};
use crate::i18n::Locale;
use crate::model::Page;
use crate::navigation::Route;
use crate::pages::keep_awake::localtime::{UtcOffset, calendar_time};

/// The id of Health's card, for a search result and a restored view.
pub(crate) const CARD: &str = "login-watch";

/// Where the window reads the records. A test or a screenshot points it at a
/// fixture directory through `BETTER_OS_TEST_BOOT_WATCH_DIR`; reading is all
/// the window ever does there.
pub(crate) fn records() -> Records {
    match std::env::var_os("BETTER_OS_TEST_BOOT_WATCH_DIR") {
        Some(dir) => Records::at(dir),
        None => Records::system(),
    }
}

/// What the window holds about the watch.
#[derive(Debug, Default)]
pub(crate) struct LoginWatchState {
    /// `None` until the records are first read.
    pub(crate) report: Option<Report>,
    /// The switch was flipped and the privileged service has not answered.
    pub(crate) writing: bool,
    /// The last write's failure, in the person's words.
    pub(crate) error: Option<&'static str>,
    pub(crate) expanded: bool,
}

impl LoginWatchState {
    pub(crate) fn read(&mut self) {
        self.report = Some(Report::read(&records()));
    }

    /// The event Home tells of, for the window's snapshot.
    pub(crate) fn recent(&self, now: u64) -> Option<Event> {
        self.report.as_ref()?.recent(now).cloned()
    }
}

/// The words, in both languages.
pub(crate) struct Copy {
    pub(crate) title: &'static str,
    pub(crate) about: &'static str,
    pub(crate) late_header: &'static str,
    pub(crate) recovered: &'static str,
    pub(crate) still_missing: &'static str,
    pub(crate) restart_failed: &'static str,
    pub(crate) pending: &'static str,
    pub(crate) person_appeared: &'static str,
    pub(crate) appeared_before_restart: &'static str,
    pub(crate) view: &'static str,
    pub(crate) nothing_recorded: &'static str,
    pub(crate) last_boot: &'static str,
    pub(crate) appeared_after: &'static str,
    pub(crate) person_present: &'static str,
    pub(crate) restarted: &'static str,
    pub(crate) already_handled: &'static str,
    pub(crate) turned_off: &'static str,
    pub(crate) setting_unreadable: &'static str,
    pub(crate) other_display_manager: &'static str,
    pub(crate) no_display_manager: &'static str,
    pub(crate) gdm_not_started: &'static str,
    pub(crate) could_not_watch: &'static str,
    pub(crate) not_checked_yet: &'static str,
    pub(crate) evidence: &'static str,
    pub(crate) yes: &'static str,
    pub(crate) no: &'static str,
    pub(crate) unknown: &'static str,
    pub(crate) none: &'static str,
    pub(crate) list_separator: &'static str,
    pub(crate) records_at: &'static str,
    pub(crate) open_folder: &'static str,
    pub(crate) write_cancelled: &'static str,
    pub(crate) write_refused: &'static str,
    pub(crate) write_failed: &'static str,
    pub(crate) switch_unknown: &'static str,
}

pub(crate) fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        Locale::EnUs | Locale::System => &EN_US,
    }
}

const ZH_TW: Copy = Copy {
    title: "登入畫面守護",
    about: "每次開機後確認登入畫面有出現。30 秒後還沒出現，就先記錄原因，再重新啟動它一次。",
    late_header: "{date} 開機時登入畫面沒有出現",
    recovered: "Better OS 已重新啟動它，{n} 秒後登入畫面出現了。",
    still_missing: "Better OS 重新啟動了它，但登入畫面還是沒有出現。",
    restart_failed: "Better OS 無法重新啟動它：{details}",
    pending: "Better OS 要求重新啟動它，但沒有記錄到結果。",
    person_appeared: "重新啟動前已經有人登入，所以沒有重新啟動。",
    appeared_before_restart: "它在重新啟動前出現了，所以沒有重新啟動。",
    view: "查看",
    nothing_recorded: "沒有登入畫面沒出現的紀錄。",
    last_boot: "這次開機",
    appeared_after: "GDM 啟動 {n} 秒後登入畫面就出現了。",
    person_present: "已經有人登入，所以沒有檢查。",
    restarted: "登入畫面沒有出現，見下方紀錄。",
    already_handled: "這次開機已經處理過一次，不會再重新啟動。",
    turned_off: "守護已關閉，沒有檢查。",
    setting_unreadable: "讀不到這項設定，所以沒有檢查。",
    other_display_manager: "這台電腦的登入管理員是 {unit}，不是 GDM，所以不需要守護。",
    no_display_manager: "這台電腦沒有安裝登入管理員，所以不需要守護。",
    gdm_not_started: "GDM 沒有在時間內啟動（{state}），交給 systemd 處理。",
    could_not_watch: "無法檢查：{details}",
    not_checked_yet: "還沒有檢查過任何一次開機。重新開機後就會開始。",
    evidence: "seat0 可顯示圖形：{graphical} · 顯示裝置：{devices} · 失敗的服務：{failed} 個",
    yes: "是",
    no: "否",
    unknown: "不明",
    none: "沒有",
    list_separator: "、",
    records_at: "完整紀錄（含系統日誌節錄）存放在 {dir}",
    open_folder: "開啟資料夾",
    write_cancelled: "沒有完成驗證，設定沒有變更。",
    write_refused: "沒有權限變更這項設定。",
    write_failed: "Better OS 的系統服務沒有完成這項變更。",
    switch_unknown: "讀不到這項設定，守護目前不會動作。切換開關可以重新寫入。",
};

const EN_US: Copy = Copy {
    title: "Login screen watch",
    about: "After each startup, checks that the login screen appears. If it hasn't after 30 seconds, Better OS records why and restarts it once.",
    late_header: "The login screen didn't appear at startup on {date}",
    recovered: "Better OS restarted it, and it appeared {n} s later.",
    still_missing: "Better OS restarted it, but it still didn't appear.",
    restart_failed: "Better OS couldn't restart it: {details}",
    pending: "Better OS asked to restart it, and no result was recorded.",
    person_appeared: "Someone logged in before the restart, so it wasn't restarted.",
    appeared_before_restart: "It appeared just before the restart, so it wasn't restarted.",
    view: "View",
    nothing_recorded: "No startup without a login screen is recorded.",
    last_boot: "This startup",
    appeared_after: "The login screen appeared {n} s after GDM started.",
    person_present: "Someone was already logged in, so nothing was checked.",
    restarted: "The login screen didn't appear. See the record below.",
    already_handled: "Already handled once this startup; it's never restarted twice.",
    turned_off: "The watch is off, so nothing was checked.",
    setting_unreadable: "The setting couldn't be read, so nothing was checked.",
    other_display_manager: "This PC logs in through {unit}, not GDM, so there's nothing to watch.",
    no_display_manager: "No login manager is installed, so there's nothing to watch.",
    gdm_not_started: "GDM didn't start in time ({state}); that's left to systemd.",
    could_not_watch: "Couldn't check: {details}",
    not_checked_yet: "No startup has been checked yet. The watch starts with the next one.",
    evidence: "seat0 ready for graphics: {graphical} · Display devices: {devices} · Failed services: {failed}",
    yes: "yes",
    no: "no",
    unknown: "unknown",
    none: "none",
    list_separator: ", ",
    records_at: "The full records, with journal excerpts, are in {dir}",
    open_folder: "Open folder",
    write_cancelled: "Authentication wasn't completed, so nothing changed.",
    write_refused: "You're not allowed to change this setting.",
    write_failed: "The Better OS system service didn't finish the change.",
    switch_unknown: "The setting couldn't be read, so the watch won't act. Flip the switch to write it again.",
};

/// "The login screen didn't appear at startup on {date}".
pub(crate) fn header(locale: Locale, event: &Event, offset: UtcOffset) -> String {
    copy(locale).late_header.replace(
        "{date}",
        &calendar_time(event.detected_at_unix_seconds, offset),
    )
}

/// The newest record in one line, for the card's description: what
/// happened, then what Better OS did.
pub(crate) fn summary(locale: Locale, event: &Event, offset: UtcOffset) -> String {
    let separator = match locale.resolved() {
        Locale::ZhTw => "。",
        Locale::EnUs | Locale::System => ". ",
    };
    format!(
        "{}{separator}{}",
        header(locale, event, offset),
        outcome(locale, &event.restart)
    )
}

/// What Better OS did about it.
pub(crate) fn outcome(locale: Locale, restart: &RestartOutcome) -> String {
    let c = copy(locale);
    match restart {
        RestartOutcome::Recovered { after_seconds } => {
            c.recovered.replace("{n}", &after_seconds.to_string())
        }
        RestartOutcome::StillMissing { .. } => c.still_missing.to_string(),
        RestartOutcome::RestartFailed { details } => {
            c.restart_failed.replace("{details}", details.trim())
        }
        RestartOutcome::Pending => c.pending.to_string(),
        RestartOutcome::PersonAppeared => c.person_appeared.to_string(),
        RestartOutcome::AppearedBeforeRestart => c.appeared_before_restart.to_string(),
    }
}

/// One line of what was recorded before the restart.
pub(crate) fn evidence_line(locale: Locale, event: &Event) -> String {
    let c = copy(locale);
    let evidence = &event.evidence;
    let graphical = match evidence.seat0_can_graphical {
        Some(true) => c.yes,
        Some(false) => c.no,
        None => c.unknown,
    };
    let devices = if evidence.drm_devices.is_empty() {
        c.none.to_string()
    } else {
        evidence.drm_devices.join(c.list_separator)
    };
    let failed = evidence
        .failed_units
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    c.evidence
        .replace("{graphical}", graphical)
        .replace("{devices}", &devices)
        .replace("{failed}", &failed.to_string())
}

/// What the latest startup's watch found, or that none was checked yet.
pub(crate) fn last_boot_line(locale: Locale, report: &Report) -> String {
    let c = copy(locale);
    let Some(last) = &report.last_boot else {
        return c.not_checked_yet.to_string();
    };
    match &last.outcome {
        LastBootOutcome::LoginScreenAppeared { after_seconds } => {
            c.appeared_after.replace("{n}", &after_seconds.to_string())
        }
        LastBootOutcome::PersonPresent => c.person_present.to_string(),
        LastBootOutcome::Restarted => c.restarted.to_string(),
        LastBootOutcome::AlreadyHandled => c.already_handled.to_string(),
        LastBootOutcome::TurnedOff => c.turned_off.to_string(),
        LastBootOutcome::SettingUnreadable => c.setting_unreadable.to_string(),
        LastBootOutcome::NoGdm {
            display_manager: Some(unit),
        } => c.other_display_manager.replace("{unit}", unit),
        LastBootOutcome::NoGdm {
            display_manager: None,
        } => c.no_display_manager.to_string(),
        LastBootOutcome::GdmNotStarted { state } => c.gdm_not_started.replace("{state}", state),
        LastBootOutcome::CouldNotWatch { details } => {
            c.could_not_watch.replace("{details}", details)
        }
    }
}

/// The switch's position. An unreadable setting is drawn off, because the
/// watch does not act on it, with [`Copy::switch_unknown`] beside it.
pub(crate) fn switch_on(setting: Setting) -> bool {
    setting.watches()
}

/// The words for a failed write, from the privileged service's key.
pub(crate) fn write_error(locale: Locale, error: &manager_platform::PlatformError) -> &'static str {
    let c = copy(locale);
    let text = error.to_string();
    if text.contains("auth_cancelled") {
        c.write_cancelled
    } else if matches!(error, manager_platform::PlatformError::PolkitDenied) {
        c.write_refused
    } else {
        c.write_failed
    }
}

/// Home's card for a late login screen in the last week (spec section 3,
/// row 2).
pub(crate) fn attention(snapshot: &Snapshot) -> Vec<AttentionItem> {
    let Some(event) = &snapshot.login_screen_late else {
        return Vec::new();
    };
    let offset = UtcOffset::for_system(event.detected_at_unix_seconds);
    vec![AttentionItem {
        source: Route::Apps,
        severity: match Report::severity(event) {
            WatchSeverity::Warning => Severity::Warning,
            WatchSeverity::Error => Severity::Error,
        },
        row: row::APPS_LOGIN_SCREEN,
        header: header(snapshot.locale, event, offset),
        description: outcome(snapshot.locale, &event.restart),
        action_label: copy(snapshot.locale).view,
        action: AttentionAction::Open(Page::Health),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_core::login_watch::Evidence;

    /// 2026-11-16 20:43 in Taipei.
    const STUCK: u64 = 1_794_832_980;
    const TAIPEI: UtcOffset = UtcOffset(8 * 3_600);

    fn event(restart: RestartOutcome) -> Event {
        Event {
            version: 1,
            boot_id: "0123456789abcdef0123456789abcdef".to_string(),
            detected_at_unix_seconds: STUCK,
            display_manager: "gdm.service".to_string(),
            waited_seconds: 30,
            evidence: Evidence {
                seat0_can_graphical: Some(false),
                drm_devices: vec!["card1".to_string(), "renderD128".to_string()],
                failed_units: "a.service loaded failed failed\n\n".to_string(),
                ..Evidence::default()
            },
            restart,
        }
    }

    #[test]
    fn the_header_says_when_in_local_time_in_both_languages() {
        let late = event(RestartOutcome::Recovered { after_seconds: 5 });
        assert_eq!(
            header(Locale::ZhTw, &late, TAIPEI),
            "2026-11-16 20:43 開機時登入畫面沒有出現"
        );
        assert_eq!(
            header(Locale::EnUs, &late, TAIPEI),
            "The login screen didn't appear at startup on 2026-11-16 20:43"
        );
        assert_eq!(
            outcome(Locale::EnUs, &late.restart),
            "Better OS restarted it, and it appeared 5 s later."
        );
        assert_eq!(
            outcome(Locale::ZhTw, &late.restart),
            "Better OS 已重新啟動它，5 秒後登入畫面出現了。"
        );
        assert_eq!(
            summary(Locale::ZhTw, &late, TAIPEI),
            "2026-11-16 20:43 開機時登入畫面沒有出現。Better OS 已重新啟動它，5 秒後登入畫面出現了。"
        );
        assert_eq!(
            summary(Locale::EnUs, &late, TAIPEI),
            "The login screen didn't appear at startup on 2026-11-16 20:43. Better OS restarted it, and it appeared 5 s later."
        );
    }

    #[test]
    fn every_outcome_has_words_in_both_languages() {
        for restart in [
            RestartOutcome::Pending,
            RestartOutcome::Recovered { after_seconds: 1 },
            RestartOutcome::StillMissing { waited_seconds: 30 },
            RestartOutcome::RestartFailed {
                details: "Job canceled.".to_string(),
            },
            RestartOutcome::PersonAppeared,
            RestartOutcome::AppearedBeforeRestart,
        ] {
            for locale in [Locale::ZhTw, Locale::EnUs] {
                let words = outcome(locale, &restart);
                assert!(!words.is_empty() && !words.contains('{'), "{words}");
            }
        }
    }

    #[test]
    fn the_evidence_line_counts_failed_services_and_lists_devices() {
        let late = event(RestartOutcome::Pending);
        assert_eq!(
            evidence_line(Locale::EnUs, &late),
            "seat0 ready for graphics: no · Display devices: card1, renderD128 · Failed services: 1"
        );
        let mut bare = late.clone();
        bare.evidence = Evidence::default();
        assert_eq!(
            evidence_line(Locale::ZhTw, &late),
            "seat0 可顯示圖形：否 · 顯示裝置：card1、renderD128 · 失敗的服務：1 個"
        );
        assert_eq!(
            evidence_line(Locale::ZhTw, &bare),
            "seat0 可顯示圖形：不明 · 顯示裝置：沒有 · 失敗的服務：0 個"
        );
    }

    #[test]
    fn home_shows_a_recent_event_as_a_warning_or_an_error_opening_health() {
        let mut snapshot = Snapshot::empty(Locale::EnUs);
        assert!(attention(&snapshot).is_empty());
        snapshot.login_screen_late = Some(event(RestartOutcome::Recovered { after_seconds: 5 }));
        let items = attention(&snapshot);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].severity, Severity::Warning);
        assert_eq!(items[0].action, AttentionAction::Open(Page::Health));
        assert!(
            items[0]
                .header
                .starts_with("The login screen didn't appear at startup on ")
        );
        snapshot.login_screen_late =
            Some(event(RestartOutcome::StillMissing { waited_seconds: 30 }));
        assert_eq!(attention(&snapshot)[0].severity, Severity::Error);
    }

    #[test]
    fn the_last_boot_line_says_what_the_watch_did_and_never_a_guess() {
        let report = |outcome| Report {
            setting: Setting::OnByDefault,
            last_boot: Some(manager_core::login_watch::LastBoot {
                version: 1,
                boot_id: "f".repeat(32),
                checked_at_unix_seconds: 0,
                outcome,
            }),
            events: Vec::new(),
        };
        assert_eq!(
            last_boot_line(
                Locale::ZhTw,
                &report(LastBootOutcome::LoginScreenAppeared { after_seconds: 4 })
            ),
            "GDM 啟動 4 秒後登入畫面就出現了。"
        );
        assert_eq!(
            last_boot_line(
                Locale::EnUs,
                &report(LastBootOutcome::NoGdm {
                    display_manager: Some("lightdm.service".to_string())
                })
            ),
            "This PC logs in through lightdm.service, not GDM, so there's nothing to watch."
        );
        let never = Report {
            setting: Setting::OnByDefault,
            last_boot: None,
            events: Vec::new(),
        };
        assert_eq!(
            last_boot_line(Locale::EnUs, &never),
            "No startup has been checked yet. The watch starts with the next one."
        );
        assert!(!switch_on(Setting::Unreadable));
        assert!(switch_on(Setting::OnByDefault));
    }

    #[test]
    fn a_dismissed_prompt_reads_differently_from_a_refusal() {
        use manager_platform::PlatformError;
        assert_eq!(
            write_error(
                Locale::EnUs,
                &PlatformError::DaemonRefused("daemon.error.auth_cancelled".to_string())
            ),
            EN_US.write_cancelled
        );
        assert_eq!(
            write_error(Locale::EnUs, &PlatformError::PolkitDenied),
            EN_US.write_refused
        );
        assert_eq!(
            write_error(
                Locale::ZhTw,
                &PlatformError::DaemonUnavailable("x".to_string())
            ),
            ZH_TW.write_failed
        );
    }
}
