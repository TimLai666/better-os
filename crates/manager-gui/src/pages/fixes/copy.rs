//! The words of the Fixes page, in both languages (spec section 9).

use crate::i18n::Locale;

pub(crate) struct FixesCopy {
    pub(crate) description: &'static str,
    pub(crate) check_again: &'static str,
    pub(crate) last_checked: &'static str,
    pub(crate) nothing_found: &'static str,
    pub(crate) group_found: &'static str,
    pub(crate) group_applied: &'static str,
    pub(crate) group_all: &'static str,
    pub(crate) fix: &'static str,
    pub(crate) fix_again: &'static str,
    pub(crate) undo: &'static str,
    pub(crate) undo_all: &'static str,
    pub(crate) checking: &'static str,
    pub(crate) fixing: &'static str,
    pub(crate) undoing: &'static str,

    // Entry 1.
    pub(crate) font_header: &'static str,
    pub(crate) font_cause: &'static str,
    pub(crate) font_change: &'static str,
    pub(crate) no_undo_needed: &'static str,

    // Entry 2.
    pub(crate) electron_header: &'static str,
    pub(crate) electron_header_any: &'static str,
    pub(crate) electron_cause: &'static str,
    pub(crate) electron_change: &'static str,
    pub(crate) electron_undo: &'static str,
    pub(crate) electron_restart: &'static str,
    pub(crate) limit_flatpak_only: &'static str,
    pub(crate) limit_base_app: &'static str,
    pub(crate) limit_until_scaled: &'static str,

    // States.
    pub(crate) state_not_present: &'static str,
    pub(crate) state_found: &'static str,
    pub(crate) state_found_many: &'static str,
    pub(crate) state_fixed: &'static str,
    pub(crate) state_still_present: &'static str,
    pub(crate) state_undone: &'static str,
    pub(crate) state_cannot_check: &'static str,
    pub(crate) check_could_not_run: &'static str,
    pub(crate) undone_with_skipped: &'static str,
    pub(crate) undone_with_skipped_one: &'static str,

    // Why a check cannot run.
    pub(crate) reason_program_missing: &'static str,
    pub(crate) reason_font_query: &'static str,
    pub(crate) reason_session_unknown: &'static str,
    pub(crate) reason_scale: &'static str,
    pub(crate) reason_apps: &'static str,
    pub(crate) reason_file: &'static str,
    pub(crate) reason_not_answered: &'static str,

    // Failures.
    pub(crate) fix_failed: &'static str,
    pub(crate) undo_failed: &'static str,
    pub(crate) changed_while_fixing: &'static str,
    pub(crate) nothing_to_change: &'static str,
    pub(crate) record_unreadable: &'static str,
    pub(crate) record_unsaved: &'static str,
    pub(crate) host_unavailable: &'static str,

    // Home.
    pub(crate) home_found_one: &'static str,
    pub(crate) home_found: &'static str,
    pub(crate) view: &'static str,
}

const EN: FixesCopy = FixesCopy {
    description: "Better OS checks this PC for known problems. Every fix can be undone.",
    check_again: "Check again",
    last_checked: "Last checked: {time}",
    nothing_found: "No known problems found.",
    group_found: "Problems found",
    group_applied: "Fixes applied",
    group_all: "All known problems",
    fix: "Fix",
    fix_again: "Fix again",
    undo: "Undo",
    undo_all: "Undo all",
    checking: "Checking…",
    fixing: "Fixing…",
    undoing: "Undoing…",

    font_header: "Text is blank in some apps",
    font_cause: "The font cache is damaged and answers every font query with the same file.",
    font_change: "Rebuilds your own font cache: {command}",
    no_undo_needed: "This fix doesn't need undoing.",

    electron_header: "The pointer is half size over {app}",
    electron_header_any: "The pointer is half size over Electron apps",
    electron_cause: "This app doesn't scale its pointer on Wayland; running it through XWayland fixes it.",
    electron_change: "Writes a Flatpak override for this app only, in {path}: {keys}",
    electron_undo: "Undo removes exactly these settings and leaves anything else in that file alone.",
    electron_restart: "Takes effect the next time {app} starts.",
    limit_flatpak_only: "Verified on Flatpak apps only. Nothing is offered for an app installed another way.",
    limit_base_app: "Only apps built on Flathub's Electron base are recognized.",
    limit_until_scaled: "Undo this once the app ships an Electron that scales its pointer.",

    state_not_present: "This PC doesn't have this problem.",
    state_found: "Found on this PC.",
    state_found_many: "Found in {n} apps on this PC.",
    state_fixed: "Fixed and checked.",
    state_still_present: "Applied, but the problem is still there.",
    state_undone: "Undone.",
    state_cannot_check: "Can't check: {reason}",
    check_could_not_run: "Applied, but it couldn't be checked: {reason}",
    undone_with_skipped: "Undone. {n} settings changed by another program were left as they are.",
    undone_with_skipped_one: "Undone. 1 setting changed by another program was left as it is.",

    reason_program_missing: "{program} isn't installed on this PC.",
    reason_font_query: "fontconfig didn't answer a font query.",
    reason_session_unknown: "It isn't known whether this session is Wayland or X11.",
    reason_scale: "The display scale can't be read.",
    reason_apps: "The installed Flatpak apps can't be read.",
    reason_file: "This app's Flatpak override file can't be read.",
    reason_not_answered: "The check didn't finish.",

    fix_failed: "The fix didn't finish.",
    undo_failed: "The undo didn't finish.",
    changed_while_fixing: "The file changed while it was being fixed. Check again, then try once more.",
    nothing_to_change: "Everything this fix sets is already set.",
    record_unreadable: "The record of applied fixes can't be read, so this page won't fix or undo anything until it can.",
    record_unsaved: "The fix was applied, but its record couldn't be saved, so it may not be possible to undo it later.",
    host_unavailable: "Your home folder isn't known, so nothing can be checked.",

    home_found_one: "1 known problem found",
    home_found: "{n} known problems found",
    view: "View",
};

const ZH: FixesCopy = FixesCopy {
    description: "Better OS 會檢查這台電腦有沒有已知的問題。每一項修正都可以復原。",
    check_again: "重新檢查",
    last_checked: "上次檢查：{time}",
    nothing_found: "沒有找到已知的問題。",
    group_found: "找到的問題",
    group_applied: "已套用的修正",
    group_all: "所有已知問題",
    fix: "修正",
    fix_again: "再修正一次",
    undo: "復原",
    undo_all: "全部復原",
    checking: "正在檢查…",
    fixing: "正在修正…",
    undoing: "正在復原…",

    font_header: "有些應用程式的文字是空白的",
    font_cause: "字型快取損壞了，每次查字型都拿到同一個檔案。",
    font_change: "重建你自己的字型快取：{command}",
    no_undo_needed: "這項修正不需要復原。",

    electron_header: "{app} 的游標只有一半大",
    electron_header_any: "Electron 應用程式的游標只有一半大",
    electron_cause: "這個應用程式在 Wayland 下不會放大游標，改用 XWayland 執行就正常。",
    electron_change: "只為這個應用程式寫入 Flatpak 設定，位置在 {path}：{keys}",
    electron_undo: "復原只會移除這幾項設定，那個檔案裡的其他內容都不會動。",
    electron_restart: "下次開啟 {app} 時生效。",
    limit_flatpak_only: "只在 Flatpak 版本驗證過，用其他方式安裝的應用程式不會提供這項修正。",
    limit_base_app: "只認得用 Flathub Electron 基礎建置的應用程式。",
    limit_until_scaled: "等這個應用程式換成會放大游標的 Electron 版本，就該復原這項修正。",

    state_not_present: "這台電腦沒有這個問題。",
    state_found: "這台電腦有這個問題。",
    state_found_many: "這台電腦有 {n} 個應用程式有這個問題。",
    state_fixed: "已修正，檢查通過。",
    state_still_present: "已套用，但問題還在。",
    state_undone: "已復原。",
    state_cannot_check: "無法檢查：{reason}",
    check_could_not_run: "已套用，但無法檢查：{reason}",
    undone_with_skipped: "已復原。有 {n} 項被其他程式變更的設定維持原樣。",
    undone_with_skipped_one: "已復原。有 1 項被其他程式變更的設定維持原樣。",

    reason_program_missing: "這台電腦沒有安裝 {program}。",
    reason_font_query: "fontconfig 沒有回答字型查詢。",
    reason_session_unknown: "不知道目前是 Wayland 還是 X11 工作階段。",
    reason_scale: "讀不到螢幕的縮放比例。",
    reason_apps: "讀不到已安裝的 Flatpak 應用程式。",
    reason_file: "讀不到這個應用程式的 Flatpak 設定檔。",
    reason_not_answered: "檢查沒有完成。",

    fix_failed: "修正沒有完成。",
    undo_failed: "復原沒有完成。",
    changed_while_fixing: "修正時設定檔被改過了。請重新檢查，再試一次。",
    nothing_to_change: "這項修正要設定的內容都已經設好了。",
    record_unreadable: "讀不到已套用修正的紀錄，在讀到之前這一頁不會修正或復原任何東西。",
    record_unsaved: "修正已套用，但紀錄沒有存下來，之後可能無法復原。",
    host_unavailable: "不知道你的家目錄在哪裡，所以無法檢查。",

    home_found_one: "找到 1 個已知問題",
    home_found: "找到 {n} 個已知問題",
    view: "查看",
};

pub(crate) fn copy(locale: Locale) -> &'static FixesCopy {
    match locale.resolved() {
        Locale::ZhTw => &ZH,
        Locale::EnUs | Locale::System => &EN,
    }
}
