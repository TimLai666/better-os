//! The Touchpad page's own words: what spec section 5 adds around today's
//! Better Touchpad copy. The controls' copy is `touchpad-views`' tables,
//! word for word except where the audit found a fault (spec section 5).

use crate::i18n::Locale;

pub(crate) struct PageCopy {
    pub(crate) not_gnome: &'static str,
    pub(crate) this_touchpad: &'static str,
    pub(crate) this_touchpad_none: &'static str,
    pub(crate) gestures_row: &'static str,
    pub(crate) apply_bar_one: &'static str,
    pub(crate) apply_bar_many: &'static str,
    pub(crate) details: &'static str,
    pub(crate) selected_badge: &'static str,
    pub(crate) session_wayland: &'static str,
    pub(crate) session_x11: &'static str,
    pub(crate) session_unknown: &'static str,
    pub(crate) backend_gnome: &'static str,
    pub(crate) backend_memory: &'static str,
    pub(crate) backend_unreachable: &'static str,
    pub(crate) adapter_desktop: &'static str,
    pub(crate) adapter_launcher: &'static str,
    pub(crate) adapter_none: &'static str,
    pub(crate) choose_export: &'static str,
    pub(crate) choose_import: &'static str,
    pub(crate) export_title: &'static str,
    pub(crate) import_title: &'static str,
    pub(crate) exported_to: &'static str,
    pub(crate) portal_missing: &'static str,
    pub(crate) not_a_file: &'static str,
    pub(crate) captured_values: &'static str,
    pub(crate) check_column: &'static str,
    pub(crate) state_column: &'static str,
    pub(crate) detail_column: &'static str,
    pub(crate) safe_mode_card: &'static str,
    pub(crate) safe_mode_description: &'static str,
    pub(crate) home_safe_mode_header: &'static str,
    pub(crate) home_safe_mode_description: &'static str,
    pub(crate) home_turned_off_header: &'static str,
    pub(crate) home_turned_off_description: &'static str,
    pub(crate) view: &'static str,
    pub(crate) choose: &'static str,
}

const EN: PageCopy = PageCopy {
    not_gnome: "These settings currently work only on the GNOME desktop.",
    this_touchpad: "This touchpad",
    this_touchpad_none: "No touchpad",
    gestures_row: "Gestures",
    apply_bar_one: "1 change hasn't been applied.",
    apply_bar_many: "{n} changes haven't been applied.",
    details: "Details",
    selected_badge: "In use",
    session_wayland: "Wayland",
    session_x11: "X11",
    session_unknown: "Unknown",
    backend_gnome: "GNOME settings",
    backend_memory: "Demo: nothing is written",
    backend_unreachable: "GNOME settings aren't answering",
    adapter_desktop: "Connected to the desktop",
    adapter_launcher: "Opens Better Launcher only",
    adapter_none: "Not connected to the desktop",
    choose_export: "Export…",
    choose_import: "Import…",
    export_title: "Export gestures",
    import_title: "Import gestures",
    exported_to: "Saved to {path}.",
    portal_missing: "The file chooser didn't answer, so type the path instead.",
    not_a_file: "That isn't a file on this PC.",
    captured_values: "Captured before the first change",
    check_column: "Check",
    state_column: "State",
    detail_column: "Detail",
    safe_mode_card: "Safe mode",
    safe_mode_description: "Shows the settings without changing anything, and gives GNOME its own gestures back.",
    home_safe_mode_header: "Touchpad safe mode is on",
    home_safe_mode_description: "Better OS won't change touchpad settings or handle gestures.",
    home_turned_off_header: "Gestures turned themselves off",
    home_turned_off_description: "Three gestures in a row didn't work, so they were turned off.",
    view: "View",
    choose: "Choose…",
};

const ZH: PageCopy = PageCopy {
    not_gnome: "這些設定目前只支援 GNOME 桌面。",
    this_touchpad: "這個觸控板",
    this_touchpad_none: "沒有觸控板",
    gestures_row: "手勢",
    apply_bar_one: "有 1 項變更還沒套用。",
    apply_bar_many: "有 {n} 項變更還沒套用。",
    details: "詳細資料",
    selected_badge: "使用中",
    session_wayland: "Wayland",
    session_x11: "X11",
    session_unknown: "不明",
    backend_gnome: "GNOME 設定",
    backend_memory: "示範：不會寫入任何設定",
    backend_unreachable: "GNOME 設定沒有回應",
    adapter_desktop: "已連上桌面",
    adapter_launcher: "只能開啟 Better Launcher",
    adapter_none: "沒有連上桌面",
    choose_export: "匯出…",
    choose_import: "匯入…",
    export_title: "匯出手勢設定",
    import_title: "匯入手勢設定",
    exported_to: "已存到 {path}。",
    portal_missing: "檔案選擇視窗沒有回應，請直接輸入路徑。",
    not_a_file: "這不是這台電腦上的檔案。",
    captured_values: "第一次變更前記錄的值",
    check_column: "檢查項目",
    state_column: "狀態",
    detail_column: "說明",
    safe_mode_card: "安全模式",
    safe_mode_description: "只顯示設定、不做任何變更，並把手勢交還給 GNOME。",
    home_safe_mode_header: "觸控板安全模式已開啟",
    home_safe_mode_description: "Better OS 不會變更觸控板設定，也不會處理手勢。",
    home_turned_off_header: "手勢已自動關閉",
    home_turned_off_description: "連續三次手勢沒有成功，所以先關閉了。",
    view: "查看",
    choose: "請選擇…",
};

pub(crate) fn page_copy(locale: Locale) -> &'static PageCopy {
    match locale.resolved() {
        Locale::ZhTw => &ZH,
        _ => &EN,
    }
}

/// A recognized event's kind, as the gesture test lists it. The
/// recognizer's own key (`threshold-crossed`) is never on screen.
pub(crate) fn event_words(kind: &str, locale: Locale) -> &'static str {
    let zh = locale.resolved() == Locale::ZhTw;
    match (kind, zh) {
        ("begin", true) => "開始",
        ("begin", false) => "Started",
        ("progress", true) => "進行中",
        ("progress", false) => "In progress",
        ("threshold-crossed", true) => "超過觸發門檻",
        ("threshold-crossed", false) => "Past the threshold",
        ("complete", true) => "完成",
        ("complete", false) => "Completed",
        ("cancel", true) => "取消",
        ("cancel", false) => "Cancelled",
        (_, true) => "其他",
        (_, false) => "Other",
    }
}

/// Whether the last gesture test reached the desktop (ticket 88).
pub(crate) fn test_reach(locale: Locale, live: bool, attempted: usize, performed: usize) -> String {
    let zh = locale.resolved() == Locale::ZhTw;
    if !live {
        return if zh {
            "只重播了辨識過程，沒有送到桌面。".to_string()
        } else {
            "Replayed only; nothing was sent to the desktop.".to_string()
        };
    }
    match (attempted, performed) {
        (0, _) => {
            if zh {
                "沒有辨識出要送到桌面的動作。".to_string()
            } else {
                "Nothing was recognized to send to the desktop.".to_string()
            }
        }
        (a, p) if p >= a => {
            if zh {
                "已送到桌面。".to_string()
            } else {
                "It reached the desktop.".to_string()
            }
        }
        (a, p) => {
            if zh {
                format!("有 {} 個動作沒有送到桌面。", a - p)
            } else {
                format!("{} actions didn't reach the desktop.", a - p)
            }
        }
    }
}
