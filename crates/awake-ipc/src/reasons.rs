//! What a Better Awake message key says, in words a person can read.
//!
//! The service reports why something is unavailable or refused as a stable
//! key (`awake.provider.no_battery`, `awake.backend.denied`,
//! `awake.error.too_many_sessions`), never a sentence, so the wire stays
//! language-free. Every window that shows one — the Better Awake window and
//! Better Manager's Keep awake page — turns it into a sentence here, from one
//! table. A key may carry a detail after a colon
//! (`awake.provider.path_unwatchable:Permission denied`); the detail follows the
//! sentence in parentheses. A detail that is itself a key
//! (`awake.error.end_condition:awake.error.end_time_in_the_past`) is said as
//! a second sentence, and a detail a row names on its own
//! (`awake.ipc.error.invalid_quick_sessions:no_lengths`) is that row. A key the
//! table does not know is shown as "Unknown reason" with the key in
//! parentheses, never as the key alone.

/// The languages the windows ship.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    English,
    TraditionalChinese,
}

/// Every provider reason: the key, the English sentence, and the Taiwan
/// Mandarin sentence.
pub const PROVIDER_REASONS: &[(&str, &str, &str)] = &[
    (
        "awake.provider.not_sampled",
        "This reading has not been taken yet.",
        "還沒有讀取過這項資料。",
    ),
    (
        "awake.provider.awaiting_second_sample",
        "This needs two readings a few seconds apart, and only one has been taken so far.",
        "這項資料需要相隔幾秒讀取兩次，目前只讀到一次。",
    ),
    (
        "awake.provider.counters_did_not_advance",
        "The system's counters did not change between two readings, so no rate can be worked out.",
        "系統計數器在兩次讀取之間沒有變化，所以算不出速率。",
    ),
    (
        "awake.provider.malformed_stat",
        "The system's processor statistics could not be understood.",
        "看不懂系統的處理器統計資料。",
    ),
    (
        "awake.provider.interface_missing",
        "The system file this reading comes from does not exist on this computer.",
        "這台電腦上沒有提供這項資料的系統檔案。",
    ),
    (
        "awake.provider.permission_denied",
        "Better Awake is not allowed to read the system file this reading comes from.",
        "Better Awake 沒有權限讀取提供這項資料的系統檔案。",
    ),
    (
        "awake.provider.read_failed",
        "The system file this reading comes from could not be read.",
        "無法讀取提供這項資料的系統檔案。",
    ),
    (
        "awake.provider.no_battery",
        "This computer has no battery.",
        "這台電腦沒有電池。",
    ),
    (
        "awake.provider.no_mains_supply",
        "This computer reports no power adapter, so whether it is plugged in cannot be told.",
        "這台電腦沒有回報電源變壓器，所以無法判斷是否接上電源。",
    ),
    (
        "awake.provider.no_power_supply",
        "This computer reports no battery and no power adapter.",
        "這台電腦沒有回報電池，也沒有回報電源變壓器。",
    ),
    (
        "awake.provider.wrong_provider",
        "Better Awake asked the wrong source for this reading. This is a fault in Better Awake.",
        "Better Awake 向錯誤的來源要求這項資料，這是 Better Awake 本身的問題。",
    ),
    (
        "awake.provider.no_drm_connectors",
        "The graphics driver lists no display connectors, so an external display cannot be detected.",
        "顯示卡驅動程式沒有列出任何顯示連接埠，所以偵測不到外接螢幕。",
    ),
    (
        "awake.provider.no_alsa_playback_device",
        "No sound playback device was found. Better Awake reads sound through ALSA, so Bluetooth and network speakers are not seen.",
        "找不到播放聲音的裝置。Better Awake 是透過 ALSA 讀取聲音狀態，看不到藍牙和網路喇叭。",
    ),
    (
        "awake.provider.fullscreen_needs_compositor_adapter",
        "Only the desktop itself knows whether a window is full screen, and the part that would ask it is not built yet.",
        "只有桌面本身知道視窗是否為全螢幕，而向桌面詢問的元件還沒有做好。",
    ),
    (
        "awake.provider.clock_out_of_range",
        "The system clock reads a time outside what Better Awake can handle.",
        "系統時鐘的時間超出 Better Awake 能處理的範圍。",
    ),
    (
        "awake.provider.no_time_zone",
        "This computer has no time zone set, so the local time a schedule needs is not known.",
        "這台電腦沒有設定時區，所以不知道排程需要的當地時間。",
    ),
    (
        "awake.provider.path_unwatchable",
        "This file or folder cannot be watched for changes.",
        "無法監看這個檔案或資料夾的變動。",
    ),
    (
        "awake.provider.watch_unavailable",
        "Watching files for changes is not available on this computer.",
        "這台電腦無法監看檔案變動。",
    ),
];

/// Every reason the keep-awake backend can give: the key, the English
/// sentence, and the Taiwan Mandarin sentence (ticket 172).
pub const BACKEND_REASONS: &[(&str, &str, &str)] = &[
    (
        "awake.backend.not_probed",
        "The system service that keeps the computer awake has not been checked yet.",
        "還沒有檢查過讓電腦保持清醒的系統服務。",
    ),
    (
        "awake.backend.unavailable",
        "The system service that keeps the computer awake could not be reached.",
        "無法連上讓電腦保持清醒的系統服務。",
    ),
    (
        "awake.backend.denied",
        "The system refused to keep the computer awake.",
        "系統拒絕讓電腦保持清醒。",
    ),
    (
        "awake.backend.protocol",
        "The system service that keeps the computer awake gave an answer Better Awake could not use.",
        "讓電腦保持清醒的系統服務給了 Better Awake 無法使用的回應。",
    ),
    (
        "awake.backend.lease_lost",
        "The system stopped keeping the computer awake, so Better Awake is asking it again.",
        "系統不再讓電腦保持清醒，Better Awake 正在重新要求。",
    ),
];

/// Every other key the service can send: a refused request, a rule it holds
/// back, a file it had to recover, and how a conflict was settled.
pub const SERVICE_REASONS: &[(&str, &str, &str)] = &[
    // Sessions.
    (
        "awake.error.manual_session_already_active",
        "A session you started is already running. Change or end that one instead.",
        "你開始的工作階段已經在執行，請改為調整或結束那一個。",
    ),
    (
        "awake.error.unknown_session",
        "That session has already ended.",
        "這個工作階段已經結束了。",
    ),
    (
        "awake.error.end_condition",
        "The end you chose for the session cannot be used.",
        "你為工作階段選的結束方式無法使用。",
    ),
    (
        "awake.error.too_many_sessions",
        "Too many sessions are running at once. End one before starting another.",
        "同時執行的工作階段太多了，請先結束一個再開始新的。",
    ),
    (
        "awake.error.security_confirmation_required",
        "Stopping the automatic screen lock needs you to accept the security warning first.",
        "要阻止自動鎖定螢幕，必須先接受安全警告。",
    ),
    (
        "awake.error.backend_unavailable",
        "The computer cannot be kept awake right now, because the system service that does it is not available.",
        "目前無法讓電腦保持清醒，因為負責這件事的系統服務無法使用。",
    ),
    (
        "awake.error.no_manual_session",
        "There is no session you started to change.",
        "沒有你開始的工作階段可以調整。",
    ),
    (
        "awake.error.reason_empty",
        "The name cannot be empty.",
        "名稱不能空白。",
    ),
    (
        "awake.error.reason_too_long",
        "The name is too long.",
        "名稱太長了。",
    ),
    (
        "awake.error.reason_control_character",
        "The name contains a character that cannot be used.",
        "名稱含有不能使用的字元。",
    ),
    (
        "awake.error.duration_zero",
        "The length must be longer than zero.",
        "長度必須大於零。",
    ),
    (
        "awake.error.duration_too_long",
        "The length is longer than Better Awake allows.",
        "長度超過 Better Awake 允許的上限。",
    ),
    (
        "awake.error.end_time_in_the_past",
        "The end time has already passed.",
        "結束時間已經過了。",
    ),
    (
        "awake.error.cannot_extend_indefinite",
        "A session that runs until you end it cannot be extended.",
        "持續到手動結束的工作階段無法延長。",
    ),
    (
        "awake.error.lid_limit_out_of_range",
        "That time limit for a closed lid is outside what Better Awake allows.",
        "這個闔上螢幕的時間限制超出 Better Awake 允許的範圍。",
    ),
    // Rules.
    (
        "awake.rule.error.unknown_rule",
        "That rule no longer exists.",
        "這條規則已經不存在了。",
    ),
    (
        "awake.rule.error.too_many_rules",
        "There are too many rules. Delete one before adding another.",
        "規則太多了，請先刪除一條再新增。",
    ),
    (
        "awake.rule.error.empty_rule",
        "A rule needs at least one condition.",
        "規則至少需要一個條件。",
    ),
    (
        "awake.rule.error.empty_group",
        "A condition group needs at least one condition.",
        "條件群組至少需要一個條件。",
    ),
    (
        "awake.rule.error.too_many_groups",
        "This rule has too many condition groups.",
        "這條規則的條件群組太多了。",
    ),
    (
        "awake.rule.error.too_many_conditions",
        "This rule has too many conditions.",
        "這條規則的條件太多了。",
    ),
    (
        "awake.rule.error.empty_matcher",
        "The program to look for cannot be empty.",
        "要尋找的程式不能空白。",
    ),
    (
        "awake.rule.error.matcher_too_long",
        "The program to look for is too long.",
        "要尋找的程式名稱太長了。",
    ),
    (
        "awake.rule.error.invalid_matcher",
        "The program to look for contains a character that cannot be used.",
        "要尋找的程式含有不能使用的字元。",
    ),
    (
        "awake.rule.error.invalid_interface",
        "That is not a network interface name.",
        "這不是網路介面的名稱。",
    ),
    (
        "awake.rule.error.invalid_watched_path",
        "That file or folder path cannot be watched.",
        "無法監看這個檔案或資料夾路徑。",
    ),
    (
        "awake.rule.error.invalid_watch_window",
        "The time to watch for changes is outside what Better Awake allows.",
        "監看變動的時間超出 Better Awake 允許的範圍。",
    ),
    (
        "awake.rule.error.invalid_battery_range",
        "The battery range must go from a lower percentage to a higher one.",
        "電量範圍必須從較低的百分比到較高的百分比。",
    ),
    (
        "awake.rule.error.invalid_battery_threshold",
        "That battery percentage cannot be used.",
        "這個電量百分比無法使用。",
    ),
    (
        "awake.rule.error.invalid_cpu_threshold",
        "That processor usage percentage cannot be used.",
        "這個處理器使用率百分比無法使用。",
    ),
    (
        "awake.rule.error.empty_schedule",
        "The schedule has no days or times.",
        "排程沒有任何日期或時間。",
    ),
    (
        "awake.rule.error.invalid_schedule",
        "That schedule cannot be used.",
        "這個排程無法使用。",
    ),
    (
        "awake.rule.error.invalid_name",
        "The rule's name cannot be used.",
        "規則名稱無法使用。",
    ),
    (
        "awake.rule.error.invalid_position",
        "The rule cannot be moved there.",
        "規則無法移到那個位置。",
    ),
    (
        "awake.rule.error.override_confirmation_required",
        "Pausing every rule at once needs your confirmation first.",
        "一次暫停所有規則，必須先經過你確認。",
    ),
    (
        "awake.rule.error.invalid_pause_duration",
        "That pause length cannot be used.",
        "這個暫停長度無法使用。",
    ),
    // A rule that matches and is held back.
    (
        "awake.battery.below_stop_threshold",
        "The battery is below this rule's stop level, so the rule is not keeping the computer awake.",
        "電量低於這條規則的停止門檻，所以這條規則沒有讓電腦保持清醒。",
    ),
    (
        "awake.lid.closed_on_battery_limit",
        "The lid has been closed on battery for longer than the time limit, so the rule is not keeping the computer awake.",
        "闔上螢幕並使用電池的時間已超過限制，所以這條規則沒有讓電腦保持清醒。",
    ),
    // How a conflict between rules was settled.
    (
        "awake.conflict.strongest_protection_wins",
        "The strongest protection wins.",
        "保護力最強的一方勝出。",
    ),
    (
        "awake.conflict.earliest_battery_stop_wins",
        "The earliest battery stop wins.",
        "最早停止的電量門檻勝出。",
    ),
    // The request itself.
    (
        "awake.ipc.error.payload_too_large",
        "The request was larger than the service accepts.",
        "要求的內容超過服務能接受的大小。",
    ),
    (
        "awake.ipc.error.malformed",
        "The service could not read the request.",
        "服務看不懂這個要求。",
    ),
    (
        "awake.ipc.error.protocol_version",
        "This window and the keep-awake service are different versions. Update Better Awake.",
        "這個視窗和保持清醒服務的版本不同，請更新 Better Awake。",
    ),
    (
        "awake.ipc.error.invalid_reason",
        "The name cannot be used.",
        "名稱無法使用。",
    ),
    (
        "awake.ipc.error.invalid_session_id",
        "That session does not exist.",
        "這個工作階段不存在。",
    ),
    (
        "awake.ipc.error.invalid_battery_threshold",
        "That battery percentage cannot be used.",
        "這個電量百分比無法使用。",
    ),
    (
        "awake.ipc.error.invalid_duration",
        "That length cannot be used.",
        "這個長度無法使用。",
    ),
    (
        "awake.ipc.error.end_condition",
        "The end you chose for the session cannot be used.",
        "你為工作階段選的結束方式無法使用。",
    ),
    (
        "awake.ipc.error.invalid_rule_id",
        "That rule does not exist.",
        "這條規則不存在。",
    ),
    (
        "awake.ipc.error.invalid_rule",
        "The rule cannot be saved.",
        "無法儲存這條規則。",
    ),
    (
        "awake.ipc.error.invalid_rule_position",
        "The rule cannot be moved there.",
        "規則無法移到那個位置。",
    ),
    (
        "awake.ipc.error.invalid_pause_duration",
        "That pause length cannot be used.",
        "這個暫停長度無法使用。",
    ),
    (
        "awake.ipc.error.invalid_history_limit",
        "That many history entries cannot be shown at once.",
        "無法一次顯示這麼多筆紀錄。",
    ),
    (
        "awake.ipc.error.invalid_lid_limit",
        "That time limit for a closed lid cannot be used.",
        "這個闔上螢幕的時間限制無法使用。",
    ),
    (
        "awake.ipc.error.unexpected_reply",
        "The keep-awake service answered with something other than what was asked for.",
        "保持清醒服務回傳的內容不是所要求的。",
    ),
    (
        "awake.ipc.error.needs_newer_service",
        "The keep-awake service is too old to do this. Update Better Awake.",
        "保持清醒服務的版本太舊，無法這麼做，請更新 Better Awake。",
    ),
    (
        "awake.ipc.error.invalid_quick_sessions",
        "The quick session lengths cannot be saved.",
        "無法儲存快速工作階段的長度。",
    ),
    (
        "awake.ipc.error.invalid_quick_sessions:no_lengths",
        "Keep at least one length.",
        "至少要保留一個長度。",
    ),
    (
        "awake.ipc.error.invalid_quick_sessions:too_many_lengths",
        "There are too many lengths.",
        "長度太多了。",
    ),
    (
        "awake.ipc.error.invalid_quick_sessions:duplicate_length",
        "The same length is listed twice.",
        "同一個長度出現了兩次。",
    ),
    (
        "awake.ipc.error.invalid_quick_sessions:length_out_of_range",
        "A length is outside what Better Awake allows.",
        "有個長度超出 Better Awake 允許的範圍。",
    ),
    (
        "awake.ipc.error.invalid_quick_sessions:default_out_of_range",
        "The default length is not one of the lengths offered.",
        "預設長度不在提供的長度之中。",
    ),
    (
        "awake.ipc.error.invalid_quick_sessions:battery_out_of_range",
        "The battery stop must be between 1 and 99 percent.",
        "電量停止門檻必須介於 1% 到 99% 之間。",
    ),
    // Files the service keeps.
    (
        "awake.quick_sessions.import_newer_schema",
        "The quick sessions to bring over were saved by a newer Better Awake, so they were not read.",
        "要帶過來的快速工作階段是由較新版的 Better Awake 儲存的，所以沒有讀取。",
    ),
    (
        "awake.quick_sessions.import_unreadable",
        "The earlier quick session settings could not be read, so they were not brought over.",
        "無法讀取先前的快速工作階段設定，所以沒有帶過來。",
    ),
    (
        "awake.quick_sessions.newer_schema",
        "The quick sessions were saved by a newer Better Awake, so the defaults are used.",
        "快速工作階段是由較新版的 Better Awake 儲存的，所以改用預設值。",
    ),
    (
        "awake.quick_sessions.not_writable",
        "The quick sessions cannot be saved, because their file cannot be written.",
        "無法儲存快速工作階段，因為無法寫入它們的檔案。",
    ),
    (
        "awake.quick_sessions.recovered_corrupt",
        "The quick sessions file was damaged, so the defaults are used.",
        "快速工作階段的檔案已損壞，所以改用預設值。",
    ),
    (
        "awake.quick_sessions.unreadable",
        "The quick sessions file could not be read, so the defaults are used.",
        "無法讀取快速工作階段的檔案，所以改用預設值。",
    ),
    (
        "awake.quick_sessions.write_failed",
        "The quick sessions could not be saved.",
        "無法儲存快速工作階段。",
    ),
    (
        "awake.settings.newer_schema",
        "The settings were saved by a newer Better Awake, so the defaults are used.",
        "設定是由較新版的 Better Awake 儲存的，所以改用預設值。",
    ),
    (
        "awake.settings.not_writable",
        "The settings cannot be saved, because their file cannot be written.",
        "無法儲存設定，因為無法寫入設定檔。",
    ),
    (
        "awake.settings.recovered_corrupt",
        "The settings file was damaged, so the defaults are used.",
        "設定檔已損壞，所以改用預設值。",
    ),
    (
        "awake.settings.unreadable",
        "The settings file could not be read, so the defaults are used.",
        "無法讀取設定檔，所以改用預設值。",
    ),
    (
        "awake.settings.write_failed",
        "The settings could not be saved.",
        "無法儲存設定。",
    ),
    (
        "awake.store.recovered_corrupt_state",
        "The record of running sessions was damaged and has been started over.",
        "執行中工作階段的紀錄已損壞，已重新開始記錄。",
    ),
    (
        "awake.store.recovered_corrupt_rules",
        "The rules file was damaged and has been started over.",
        "規則檔已損壞，已重新開始。",
    ),
    (
        "awake.store.recovered_corrupt_history",
        "The history file was damaged and has been started over.",
        "紀錄檔已損壞，已重新開始記錄。",
    ),
    (
        "awake.store.error.io",
        "A Better Awake file could not be read or written.",
        "無法讀取或寫入 Better Awake 的檔案。",
    ),
    (
        "awake.store.error.unsupported_schema",
        "A Better Awake file was written by a newer version.",
        "Better Awake 的檔案是由較新的版本寫入的。",
    ),
    (
        "awake.store.error.serialize",
        "Better Awake could not write its data.",
        "Better Awake 無法寫入它的資料。",
    ),
];

/// Every row of the three tables.
fn rows() -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    PROVIDER_REASONS
        .iter()
        .chain(BACKEND_REASONS)
        .chain(SERVICE_REASONS)
}

fn sentence(key: &str, language: Language) -> Option<&'static str> {
    rows()
        .find(|(known, _, _)| *known == key)
        .map(|(_, english, chinese)| match language {
            Language::English => *english,
            Language::TraditionalChinese => *chinese,
        })
}

/// The sentence for any Better Awake message key, in `language`.
pub fn reason(key: &str, language: Language) -> String {
    let key = key.trim();
    let (base, detail) = match key.split_once(':') {
        Some((base, detail)) => (base, Some(detail.trim()).filter(|d| !d.is_empty())),
        None => (key, None),
    };
    let Some(mut said) = sentence(base, language) else {
        return match language {
            Language::English => format!("Unknown reason ({key})"),
            Language::TraditionalChinese => format!("原因不明（{key}）"),
        };
    };
    let mut detail = detail;
    // A row named for the detail's own first part says it better than the
    // detail in parentheses (`invalid_quick_sessions:no_lengths`).
    if let Some(rest) = detail {
        let (head, tail) = match rest.split_once(':') {
            Some((head, tail)) => (head, Some(tail.trim()).filter(|t| !t.is_empty())),
            None => (rest, None),
        };
        if let Some(specific) = sentence(&format!("{base}:{head}"), language) {
            said = specific;
            detail = tail;
        }
    }
    match (detail, language) {
        (None, _) => said.to_string(),
        (Some(inner), Language::English) if inner.starts_with("awake.") => {
            format!("{said} {}", reason(inner, language))
        }
        (Some(inner), Language::TraditionalChinese) if inner.starts_with("awake.") => {
            format!("{said}{}", reason(inner, language))
        }
        (Some(detail), Language::English) => format!("{said} ({detail})"),
        (Some(detail), Language::TraditionalChinese) => format!("{said}（{detail}）"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IpcError, QuickSessionsError};
    use awake_core::{EndConditionError, ReasonError, RuleError};
    use std::collections::BTreeSet;
    use std::path::Path;

    /// The lines of a source file before its `#[cfg(test)] mod …`: what the
    /// crate can actually send, not what its tests make up.
    fn production(text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = String::new();
        for (index, line) in lines.iter().enumerate() {
            if line.trim() == "#[cfg(test)]"
                && lines
                    .get(index + 1)
                    .is_some_and(|next| next.trim_start().starts_with("mod "))
            {
                break;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// Every `awake.*` key written in a crate's sources, but a window's or the
    /// tray's own (`awake.gui.*`, `awake.tray.*`), which the windows say in
    /// their own words.
    fn keys_in(directory: &Path, into: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                keys_in(&path, into);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("");
            if stem == "tests" || stem.ends_with("_tests") {
                continue;
            }
            let text = production(&std::fs::read_to_string(&path).unwrap());
            for (at, _) in text.match_indices("\"awake.") {
                let rest = &text[at + 1..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_lowercase() || c == '_' || c == '.'))
                    .unwrap_or(rest.len());
                let key = rest[..end].trim_end_matches('.');
                // A prefix the code compares against (`starts_with("awake.")`)
                // is not a key; a key names a family and a member.
                if key.matches('.').count() < 2
                    || key.starts_with("awake.gui.")
                    || key.starts_with("awake.tray.")
                {
                    continue;
                }
                into.insert(key.to_string());
            }
        }
    }

    /// Every key the service and the crates it is built from can send.
    fn emitted() -> BTreeSet<String> {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut keys = BTreeSet::new();
        for member in [
            "awake-platform",
            "awake-core",
            "awake-service",
            "awake-store",
            "awake-ipc",
        ] {
            keys_in(&crates.join(member).join("src"), &mut keys);
        }
        keys
    }

    /// Whether text shows a key (`awake.error.x`), as opposed to a sentence
    /// that ends on the word ("keeps the computer awake.").
    fn shows_key(text: &str) -> bool {
        text.match_indices("awake.")
            .any(|(at, _)| text[at + 6..].starts_with(|c: char| c.is_ascii_lowercase()))
    }

    fn assert_plain(key: &str, english: &str, chinese: &str) {
        assert!(
            english.ends_with('.') && !shows_key(english),
            "{key}: {english}"
        );
        assert!(
            chinese.ends_with('。') && !shows_key(chinese),
            "{key}: {chinese}"
        );
        // Taiwan usage, not the mainland's.
        for word in ["屏幕", "设置", "文件夹", "程序", "信息", "默认", "用户"] {
            assert!(!chinese.contains(word), "{key}: {chinese} uses {word}");
        }
    }

    #[test]
    fn every_key_the_service_can_emit_has_a_sentence_in_both_languages() {
        let keys = emitted();
        for required in [
            "awake.provider.no_time_zone",
            "awake.provider.fullscreen_needs_compositor_adapter",
            "awake.provider.no_alsa_playback_device",
            "awake.provider.path_unwatchable",
            "awake.backend.not_probed",
            "awake.backend.unavailable",
            "awake.backend.denied",
            "awake.backend.protocol",
            "awake.backend.lease_lost",
            "awake.error.too_many_sessions",
            "awake.rule.error.empty_group",
            "awake.ipc.error.malformed",
            "awake.settings.write_failed",
            "awake.store.recovered_corrupt_rules",
        ] {
            assert!(keys.contains(required), "the scan missed {required}");
        }
        for key in &keys {
            let row = rows().find(|(known, _, _)| known == key);
            let (_, english, chinese) = row.unwrap_or_else(|| panic!("no sentence for {key}"));
            assert_plain(key, english, chinese);
            assert!(!shows_key(&reason(key, Language::English)), "{key}");
            assert!(
                !shows_key(&reason(key, Language::TraditionalChinese)),
                "{key}"
            );
        }
        for (key, english, chinese) in rows() {
            assert_plain(key, english, chinese);
            // A row for a detail (`invalid_quick_sessions:no_lengths`) is
            // checked against the error type below.
            if !key.contains(':') {
                assert!(
                    keys.contains(*key),
                    "{key} has a sentence but nothing emits it"
                );
            }
        }
    }

    #[test]
    fn every_refused_quick_session_offer_is_said_in_its_own_words() {
        for error in [
            QuickSessionsError::NoLengths,
            QuickSessionsError::TooManyLengths { count: 9 },
            QuickSessionsError::DuplicateLength,
            QuickSessionsError::LengthOutOfRange { minutes: 0 },
            QuickSessionsError::DefaultOutOfRange { index: 8 },
            QuickSessionsError::BatteryOutOfRange { percent: 100 },
        ] {
            let key = IpcError::InvalidQuickSessions(error).to_string();
            for language in [Language::English, Language::TraditionalChinese] {
                let said = reason(&key, language);
                let detail = error.to_string();
                let head = detail.split(':').next().unwrap();
                assert!(!said.contains(head), "{key} shows {head}: {said}");
                assert!(!shows_key(&said), "{key}: {said}");
            }
        }
        assert_eq!(
            reason(
                &IpcError::InvalidQuickSessions(QuickSessionsError::NoLengths).to_string(),
                Language::English
            ),
            "Keep at least one length."
        );
        assert_eq!(
            reason(
                &IpcError::InvalidQuickSessions(QuickSessionsError::TooManyLengths { count: 9 })
                    .to_string(),
                Language::TraditionalChinese
            ),
            "長度太多了。（9）"
        );
    }

    #[test]
    fn a_key_inside_a_key_is_said_as_a_second_sentence() {
        let key = IpcError::InvalidRule(RuleError::EmptyGroup).to_string();
        assert_eq!(
            reason(&key, Language::English),
            "The rule cannot be saved. A condition group needs at least one condition."
        );
        assert_eq!(
            reason(&key, Language::TraditionalChinese),
            "無法儲存這條規則。條件群組至少需要一個條件。"
        );
        let key = IpcError::InvalidReason(ReasonError::Empty).to_string();
        assert_eq!(
            reason(&key, Language::English),
            "The name cannot be used. The name cannot be empty."
        );
        let key = format!(
            "awake.error.end_condition:{}",
            EndConditionError::EndTimeInThePast
        );
        assert_eq!(
            reason(&key, Language::English),
            "The end you chose for the session cannot be used. The end time has already passed."
        );
    }

    #[test]
    fn a_backend_reason_carries_the_systems_own_words_after_it() {
        assert_eq!(
            reason("awake.backend.unavailable:no logind", Language::English),
            "The system service that keeps the computer awake could not be reached. (no logind)"
        );
        assert_eq!(
            reason("awake.backend.denied:no", Language::TraditionalChinese),
            "系統拒絕讓電腦保持清醒。（no）"
        );
        assert_eq!(
            reason("awake.backend.not_probed", Language::English),
            "The system service that keeps the computer awake has not been checked yet."
        );
    }

    #[test]
    fn a_detail_follows_the_sentence_in_parentheses() {
        assert_eq!(
            reason(
                "awake.provider.interface_missing:/sys/class/drm",
                Language::English
            ),
            "The system file this reading comes from does not exist on this computer. (/sys/class/drm)"
        );
        assert_eq!(
            reason(
                "awake.provider.interface_missing:/sys/class/drm",
                Language::TraditionalChinese
            ),
            "這台電腦上沒有提供這項資料的系統檔案。（/sys/class/drm）"
        );
        assert_eq!(
            reason("awake.provider.no_battery", Language::English),
            "This computer has no battery."
        );
    }

    #[test]
    fn an_unknown_key_says_so_and_names_the_key_never_shows_it_alone() {
        assert_eq!(
            reason("awake.provider.something_new", Language::English),
            "Unknown reason (awake.provider.something_new)"
        );
        assert_eq!(
            reason(
                "awake.backend.something_new:detail",
                Language::TraditionalChinese
            ),
            "原因不明（awake.backend.something_new:detail）"
        );
    }
}
