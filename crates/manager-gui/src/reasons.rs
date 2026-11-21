//! What the manager's, the platform's, and the privileged service's keys
//! say, in words (ticket 172).
//!
//! An error from below the window renders as a stable key
//! (`platform.error.daemon_refused:daemon.error.auth_cancelled`), and an error
//! report's details used to show that chain as it was. [`details_in_words`]
//! turns each line of it into sentences in the window's language: a key is
//! its sentence, what follows it fills the sentence's `{detail}` (a sentence
//! with no place for it leaves it out), a key inside a key is said as a
//! second sentence, and text that is no key is the system's own words, kept
//! under a label.

use crate::i18n::Locale;

/// Every key: the key, the English sentence, and the Taiwan Mandarin one.
pub(crate) const REASONS: &[(&str, &str, &str)] = &[
    (
        "manager.error.active_operation",
        "Another change is still running. Wait for it to finish.",
        "還有另一項變更正在進行，請等它完成。",
    ),
    (
        "manager.error.already_disabled",
        "The component is already turned off ({detail}).",
        "這個元件已經關閉了（{detail}）。",
    ),
    (
        "manager.error.already_enabled",
        "The component is already turned on ({detail}).",
        "這個元件已經開啟了（{detail}）。",
    ),
    (
        "manager.error.already_installed",
        "The component is already installed ({detail}).",
        "這個元件已經安裝了（{detail}）。",
    ),
    (
        "manager.error.cannot_remove_self",
        "Better Manager can't remove itself from inside its own window.",
        "Better Manager 無法在自己的視窗裡移除自己。",
    ),
    (
        "manager.error.conflict",
        "The component conflicts with one that is installed ({detail}).",
        "這個元件和已安裝的元件互相衝突（{detail}）。",
    ),
    (
        "manager.error.dependency_unavailable",
        "Something the component needs isn't available ({detail}).",
        "這個元件需要的東西無法取得（{detail}）。",
    ),
    (
        "manager.error.disk_space_requirement_overflow",
        "The space this change needs couldn't be added up.",
        "無法算出這項變更需要的空間。",
    ),
    (
        "manager.error.empty_plan",
        "There is nothing to change.",
        "沒有需要變更的項目。",
    ),
    (
        "manager.error.host_drift",
        "This computer changed since the change was planned ({detail}).",
        "規劃這項變更之後，這台電腦有了改變（{detail}）。",
    ),
    (
        "manager.error.host_unreadable",
        "What this computer is couldn't be read ({detail}).",
        "無法讀取這台電腦的系統資訊（{detail}）。",
    ),
    (
        "manager.error.host_version_unreadable",
        "The installed version couldn't be read ({detail}).",
        "無法讀取已安裝的版本（{detail}）。",
    ),
    (
        "manager.error.incompatible",
        "The component doesn't work on this computer ({detail}).",
        "這個元件無法在這台電腦上運作（{detail}）。",
    ),
    (
        "manager.error.insufficient_disk_space",
        "There isn't enough free space for this change.",
        "可用空間不足以進行這項變更。",
    ),
    (
        "manager.error.invalid_dependency_requirement",
        "The component asks for a version of another component in a way that can't be read ({detail}).",
        "這個元件要求其他元件版本的方式無法判讀（{detail}）。",
    ),
    (
        "manager.error.invalid_manifest",
        "A component's description couldn't be read.",
        "無法讀取元件的說明。",
    ),
    (
        "manager.error.invalid_state",
        "Better Manager's record is not in a state it can use.",
        "Better Manager 的紀錄處於無法使用的狀態。",
    ),
    (
        "manager.error.no_active_operation",
        "No change is running.",
        "沒有正在進行的變更。",
    ),
    (
        "manager.error.no_companions_to_add",
        "There is nothing to add alongside this component ({detail}).",
        "沒有需要和這個元件一起加入的項目（{detail}）。",
    ),
    (
        "manager.error.no_restore_available",
        "There is no earlier version to go back to ({detail}).",
        "沒有可以回復的舊版本（{detail}）。",
    ),
    (
        "manager.error.not_cancelable",
        "This step can't be stopped once it has started.",
        "這個步驟開始之後就無法停止。",
    ),
    (
        "manager.error.not_installed",
        "The component isn't installed ({detail}).",
        "這個元件沒有安裝（{detail}）。",
    ),
    (
        "manager.error.no_update_available",
        "There is no newer version ({detail}).",
        "沒有較新的版本（{detail}）。",
    ),
    (
        "manager.error.required_by",
        "Another component needs this one ({detail}).",
        "有其他元件需要這個元件（{detail}）。",
    ),
    (
        "manager.error.restore_artifact_missing",
        "The earlier version's package is no longer kept ({detail}).",
        "已經沒有保留舊版本的套件（{detail}）。",
    ),
    (
        "manager.error.stale_plan",
        "The plan is out of date. Look at it again and retry.",
        "這份規劃已經過時，請重新查看後再試。",
    ),
    (
        "manager.error.state_not_saved",
        "Better Manager's record couldn't be saved ({detail}).",
        "無法儲存 Better Manager 的紀錄（{detail}）。",
    ),
    (
        "manager.error.unexpected_stage_outcome",
        "A step ended in a way Better Manager didn't expect.",
        "有個步驟以 Better Manager 沒有預期的方式結束。",
    ),
    (
        "manager.error.unknown_component",
        "Better Manager doesn't know this component ({detail}).",
        "Better Manager 不認得這個元件（{detail}）。",
    ),
    (
        "manager.state.invalid_active_plan",
        "The change in progress in Better Manager's record can't be read ({detail}).",
        "無法讀取 Better Manager 紀錄裡進行中的變更（{detail}）。",
    ),
    (
        "manager.state.invalid_activity_sequence",
        "Better Manager's activity record is out of order.",
        "Better Manager 的活動紀錄順序不正確。",
    ),
    (
        "manager.state.invalid_component_id",
        "Better Manager's record names a component that can't be read ({detail}).",
        "Better Manager 的紀錄裡有個無法判讀的元件名稱（{detail}）。",
    ),
    (
        "manager.state.invalid_version",
        "Better Manager's record holds a version that can't be read ({detail}).",
        "Better Manager 的紀錄裡有個無法判讀的版本（{detail}）。",
    ),
    (
        "manager.state.unsupported_schema",
        "Better Manager's record was written by a newer version.",
        "Better Manager 的紀錄是由較新版本寫入的。",
    ),
    (
        "platform.error.capability_unavailable",
        "This computer can't do that ({detail}).",
        "這台電腦無法這麼做（{detail}）。",
    ),
    (
        "platform.error.checksum_mismatch",
        "The downloaded package isn't the one that was published, so it wasn't installed ({detail}).",
        "下載的套件和發布的不同，所以沒有安裝（{detail}）。",
    ),
    (
        "platform.error.daemon_interrupted",
        "The privileged service restarted partway; asking again reaches the new one.",
        "特權服務中途重新啟動了，再試一次會連到新的服務。",
    ),
    (
        "platform.error.daemon_refused",
        "The privileged service refused.",
        "特權服務拒絕了。",
    ),
    (
        "platform.error.daemon_unavailable",
        "The privileged service isn't running or can't be reached.",
        "特權服務沒有在執行或無法連上。",
    ),
    (
        "platform.error.download_failed",
        "The package couldn't be downloaded ({detail}).",
        "無法下載套件（{detail}）。",
    ),
    (
        "platform.error.polkit_denied",
        "The change wasn't allowed, or the password prompt was cancelled.",
        "這項變更沒有被允許，或密碼提示被取消了。",
    ),
    (
        "platform.error.privileged_execution_not_approved",
        "Changes to the system haven't been approved for this window.",
        "這個視窗還沒有被允許變更系統。",
    ),
    (
        "platform.error.unsupported_host",
        "This computer isn't one Better OS supports ({detail}).",
        "Better OS 不支援這台電腦（{detail}）。",
    ),
    (
        "daemon.error.apt_busy",
        "Another program is installing or updating software. Wait for it to finish.",
        "有其他程式正在安裝或更新軟體，請等它完成。",
    ),
    (
        "daemon.error.apt_failed",
        "Installing the package failed ({detail}).",
        "安裝套件失敗了（{detail}）。",
    ),
    (
        "daemon.error.artifact_missing",
        "The package to install wasn't there ({detail}).",
        "要安裝的套件不在（{detail}）。",
    ),
    (
        "daemon.error.auth_cancelled",
        "The password prompt was cancelled, so nothing changed.",
        "密碼提示被取消了，所以沒有任何變更。",
    ),
    (
        "daemon.error.busy",
        "The privileged service is busy with another change.",
        "特權服務正在處理另一項變更。",
    ),
    (
        "daemon.error.cancelled",
        "The change was stopped.",
        "這項變更已停止。",
    ),
    (
        "daemon.error.checksum_mismatch",
        "The package isn't the one that was published, so it wasn't installed ({detail}).",
        "這個套件和發布的不同，所以沒有安裝（{detail}）。",
    ),
    (
        "daemon.error.dpkg_interrupted",
        "An earlier software install was interrupted and has to be repaired first.",
        "先前的軟體安裝被中斷了，必須先修復。",
    ),
    (
        "daemon.error.drive_absent",
        "The drive is no longer connected.",
        "這個磁碟已經中斷連線。",
    ),
    (
        "daemon.error.drive_busy",
        "The drive is in use ({detail}).",
        "這個磁碟正在使用中（{detail}）。",
    ),
    (
        "daemon.error.drive_refused",
        "That drive can't be changed here ({detail}).",
        "這裡無法變更那個磁碟（{detail}）。",
    ),
    (
        "daemon.error.kernel_log_unreadable",
        "This boot's kernel log couldn't be read ({detail}).",
        "讀不到這次開機的核心紀錄（{detail}）。",
    ),
    (
        "daemon.error.usb_controller_not_recognised",
        "The USB controller that stopped isn't one Better OS may restart.",
        "停止運作的 USB 控制器不是 Better OS 可以重新啟動的那種。",
    ),
    (
        "daemon.error.usb_not_affected",
        "This USB-C recovery doesn't apply to this PC.",
        "這個 USB-C 修復不適用於這台電腦。",
    ),
    (
        "daemon.error.usb_module_cannot_load",
        "This kernel can't load the USB-C driver's file, so nothing was unloaded.",
        "這個核心無法載入 USB-C 驅動程式的檔案，所以沒有卸載任何東西。",
    ),
    (
        "daemon.error.usb_nothing_to_reset",
        "Nothing to reset: the USB-C ports are working.",
        "不需要重設：USB-C 連接埠運作正常。",
    ),
    (
        "daemon.error.usb_recovery_failed",
        "The USB-C recovery didn't finish ({detail}).",
        "USB-C 修復沒有完成（{detail}）。",
    ),
    (
        "daemon.error.usb_restart_needed",
        "The USB controller is stuck, and only restarting the PC brings it back.",
        "USB 控制器卡住了，只有重新開機才能恢復。",
    ),
    (
        "daemon.error.hardware_absent",
        "The device isn't there.",
        "這個裝置不存在。",
    ),
    (
        "daemon.error.health_failed",
        "The component was installed but didn't pass its check ({detail}).",
        "元件已經安裝，但沒有通過檢查（{detail}）。",
    ),
    (
        "daemon.error.host_unreadable",
        "What this computer is couldn't be read ({detail}).",
        "無法讀取這台電腦的系統資訊（{detail}）。",
    ),
    (
        "daemon.error.invalid_argument",
        "The privileged service was asked something it can't do ({detail}).",
        "特權服務收到了它無法執行的要求（{detail}）。",
    ),
    (
        "daemon.error.mount_failed",
        "The drive couldn't be mounted ({detail}).",
        "無法掛載這個磁碟（{detail}）。",
    ),
    (
        "daemon.error.mount_table_busy",
        "Another program is changing the list of drives. Try again in a moment.",
        "有其他程式正在變更磁碟清單，請稍後再試。",
    ),
    (
        "daemon.error.mount_table_malformed",
        "The system's list of drives can't be read, so it was left as it is ({detail}).",
        "無法判讀系統的磁碟清單，所以維持原樣（{detail}）。",
    ),
    (
        "daemon.error.nothing_to_repair",
        "There is nothing to repair.",
        "沒有需要修復的項目。",
    ),
    (
        "daemon.error.plan_rejected",
        "The privileged service refused the change ({detail}).",
        "特權服務拒絕了這項變更（{detail}）。",
    ),
    (
        "daemon.error.protocol",
        "Better Manager and the privileged service didn't understand each other ({detail}).",
        "Better Manager 和特權服務無法互相理解（{detail}）。",
    ),
    (
        "daemon.error.repair_failed",
        "The repair didn't work.",
        "修復沒有成功。",
    ),
    (
        "daemon.error.shutting_down",
        "The privileged service is restarting. Try again.",
        "特權服務正在重新啟動，請再試一次。",
    ),
    (
        "daemon.error.state_drift",
        "The installed software isn't what Better Manager's record says ({detail}).",
        "已安裝的軟體和 Better Manager 的紀錄不符（{detail}）。",
    ),
    (
        "daemon.error.storage",
        "The privileged service couldn't keep its record ({detail}).",
        "特權服務無法保存它的紀錄（{detail}）。",
    ),
    (
        "daemon.error.unauthorized",
        "This window isn't allowed to ask for that.",
        "這個視窗沒有權限提出這項要求。",
    ),
    (
        "daemon.error.unknown_transaction",
        "The privileged service doesn't know that change ({detail}).",
        "特權服務不認得那項變更（{detail}）。",
    ),
    (
        "daemon.error.write_failed",
        "The setting couldn't be written ({detail}).",
        "無法寫入這項設定（{detail}）。",
    ),
    (
        "ipc.error.artifact_name_mismatch",
        "The package's file name doesn't match the component.",
        "套件的檔名和元件不符。",
    ),
    (
        "ipc.error.duplicate_component",
        "The change names the same component twice.",
        "這項變更把同一個元件列了兩次。",
    ),
    (
        "ipc.error.empty_plan",
        "There is nothing to change.",
        "沒有需要變更的項目。",
    ),
    (
        "ipc.error.invalid_artifact_size",
        "The package's size can't be right.",
        "套件的大小不正確。",
    ),
    (
        "ipc.error.invalid_checksum",
        "The package's checksum can't be read.",
        "無法判讀套件的檢查碼。",
    ),
    (
        "ipc.error.invalid_component",
        "The change names a component that can't be read.",
        "這項變更裡有個無法判讀的元件名稱。",
    ),
    (
        "ipc.error.invalid_filename",
        "The package's file name can't be used.",
        "套件的檔名無法使用。",
    ),
    (
        "ipc.error.invalid_package_name",
        "The package's name can't be used.",
        "套件的名稱無法使用。",
    ),
    (
        "ipc.error.invalid_target",
        "The change is for a system it can't be read as.",
        "無法判讀這項變更的目標系統。",
    ),
    (
        "ipc.error.invalid_transaction_id",
        "The change's number can't be read.",
        "無法判讀這項變更的編號。",
    ),
    (
        "ipc.error.invalid_version",
        "The change names a version that can't be read.",
        "這項變更裡有個無法判讀的版本。",
    ),
    (
        "ipc.error.malformed",
        "The request couldn't be read.",
        "無法讀取這個要求。",
    ),
    (
        "ipc.error.missing_artifact",
        "The change is missing a package it needs.",
        "這項變更缺少需要的套件。",
    ),
    (
        "ipc.error.missing_target_version",
        "The change doesn't say which version to install.",
        "這項變更沒有指定要安裝的版本。",
    ),
    (
        "ipc.error.payload_too_large",
        "The request was larger than the privileged service accepts.",
        "要求的內容超過特權服務能接受的大小。",
    ),
    (
        "ipc.error.protocol_version",
        "Better Manager and the privileged service are different versions. Update Better Manager.",
        "Better Manager 和特權服務的版本不同，請更新 Better Manager。",
    ),
    (
        "ipc.error.too_many_log_entries",
        "The change's log is longer than the privileged service accepts.",
        "這項變更的紀錄超過特權服務能接受的長度。",
    ),
    (
        "ipc.error.too_many_packages",
        "The change installs more packages than the privileged service accepts.",
        "這項變更安裝的套件超過特權服務能接受的數量。",
    ),
    (
        "ipc.error.too_many_steps",
        "The change has more steps than the privileged service accepts.",
        "這項變更的步驟超過特權服務能接受的數量。",
    ),
    (
        "ipc.error.unexpected_artifact",
        "The change carries a package it doesn't need.",
        "這項變更帶了不需要的套件。",
    ),
    (
        "catalog.fetch.insecure_source",
        "The list of components was not fetched, because its address isn't secure (HTTPS).",
        "沒有取得元件清單，因為它的位址不安全（不是 HTTPS）。",
    ),
    (
        "catalog.fetch.unreachable",
        "The list of components couldn't be fetched ({detail}).",
        "無法取得元件清單（{detail}）。",
    ),
    (
        "catalog.fetch.unreadable",
        "The list of components that was fetched couldn't be read ({detail}).",
        "取得的元件清單無法讀取（{detail}）。",
    ),
    (
        "manager.system.bus_unreachable",
        "The system's message bus can't be reached.",
        "無法連上系統的訊息匯流排。",
    ),
    (
        "manager.system.buses_unreachable",
        "Neither message bus can be reached.",
        "兩個訊息匯流排都無法連上。",
    ),
    (
        "manager.system.no_config_home",
        "Neither XDG_CONFIG_HOME nor HOME is set, so the setting has nowhere to go.",
        "沒有設定 XDG_CONFIG_HOME，也沒有設定 HOME，所以設定無處可存。",
    ),
];

fn zh(locale: Locale) -> bool {
    locale.words() == better_ui::fluent::words::Locale::TraditionalChinese
}

fn row(key: &str, locale: Locale) -> Option<&'static str> {
    REASONS
        .iter()
        .find(|(known, _, _)| *known == key)
        .map(|(_, english, chinese)| if zh(locale) { *chinese } else { *english })
}

/// Whether `text` starts with a key of these families.
fn is_key(word: &str) -> bool {
    word.contains('.')
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
}

/// One key (with what follows it) as sentences; `None` when it is no key.
fn key_in_words(text: &str, locale: Locale) -> Option<String> {
    let (key, rest) = match text.split_once(':') {
        Some((key, rest)) => (key.trim(), rest.trim()),
        None => (text.trim(), ""),
    };
    if !is_key(key) {
        return None;
    }
    let Some(sentence) = row(key, locale) else {
        return Some(if zh(locale) {
            format!("原因不明（{text}）")
        } else {
            format!("Unknown reason ({text})")
        });
    };
    let without_slot = || {
        sentence
            .replace(" ({detail})", "")
            .replace("（{detail}）", "")
            .replace("{detail}", "")
    };
    // A key inside the detail is said as a second sentence.
    if let Some(inner) = key_in_words(rest, locale).filter(|_| !rest.is_empty()) {
        let outer = without_slot();
        return Some(if zh(locale) {
            format!("{outer}{inner}")
        } else {
            format!("{outer} {inner}")
        });
    }
    Some(if rest.is_empty() {
        without_slot()
    } else {
        sentence.replace("{detail}", rest)
    })
}

/// An error report's details in this language: each line of an error chain
/// (`Caused by:` lines included) as sentences, and the system's own text
/// under a label.
pub(crate) fn details_in_words(details: &str, locale: Locale) -> String {
    let (caused_by, system_said) = if zh(locale) {
        ("原因：", "系統回報：")
    } else {
        ("Caused by: ", "The system said: ")
    };
    details
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (prefix, text) = match line.strip_prefix("Caused by: ") {
                Some(text) => (caused_by, text),
                None => ("", line),
            };
            match key_in_words(text, locale) {
                Some(words) => format!("{prefix}{words}"),
                None => format!("{prefix}{system_said}{}", text.trim()),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{REASONS, details_in_words, key_in_words};
    use crate::i18n::Locale;
    use std::collections::BTreeSet;
    use std::path::Path;

    const FAMILIES: &[&str] = &[
        "manager.error.",
        "manager.state.",
        "platform.error.",
        "daemon.error.",
        "ipc.error.",
        "catalog.fetch.",
        "manager.system.",
    ];

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
            if !line.trim_start().starts_with("//") {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    fn keys_in(directory: &Path, into: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                keys_in(&path, into);
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            // The table is the answer, not the question.
            if path.extension().is_none_or(|e| e != "rs")
                || stem == "tests"
                || stem.ends_with("_tests")
                || path.ends_with("manager-gui/src/reasons.rs")
            {
                continue;
            }
            let text = production(&std::fs::read_to_string(&path).unwrap());
            for (at, _) in text.match_indices('"') {
                let rest = &text[at + 1..];
                if !FAMILIES.iter().any(|family| rest.starts_with(family)) {
                    continue;
                }
                let end = rest
                    .find(|c: char| {
                        !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
                    })
                    .unwrap_or(rest.len());
                let key = rest[..end].trim_end_matches('.');
                // A family compared against (`starts_with("daemon.error")`)
                // is not a key; a key names a family and a member.
                if key.matches('.').count() >= 2 {
                    into.insert(key.to_string());
                }
            }
        }
    }

    #[test]
    fn every_key_below_the_window_has_a_sentence_in_both_languages() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut keys = BTreeSet::new();
        for member in [
            "manager-core",
            "manager-platform",
            "manager-ipc",
            "manager-daemon",
            "manager-store",
            "hardware-core",
            "drive-mounts",
            "manager-gui",
        ] {
            keys_in(&crates.join(member).join("src"), &mut keys);
        }
        for required in [
            "platform.error.daemon_refused",
            "daemon.error.auth_cancelled",
            "manager.error.stale_plan",
            "ipc.error.protocol_version",
        ] {
            assert!(keys.contains(required), "the scan missed {required}");
        }
        for key in &keys {
            for locale in [Locale::EnUs, Locale::ZhTw] {
                for text in [key.clone(), format!("{key}:better-files")] {
                    let said = key_in_words(&text, locale).unwrap();
                    assert!(
                        !said.contains("Unknown reason") && !said.contains("原因不明"),
                        "{text} has no sentence"
                    );
                    assert!(!said.contains(key.as_str()), "{text}: {said}");
                    assert!(
                        !said.contains("()") && !said.contains("（）"),
                        "{text}: {said}"
                    );
                }
            }
        }
        for (key, english, chinese) in REASONS {
            assert!(
                keys.contains(*key),
                "{key} has a sentence but nothing emits it"
            );
            assert!(english.ends_with('.') && chinese.ends_with('。'), "{key}");
        }
    }

    #[test]
    fn an_error_chain_is_said_line_by_line_with_the_system_s_words_labelled() {
        let chain = "platform.error.daemon_refused:daemon.error.auth_cancelled\nCaused by: org.freedesktop.DBus.Error.AccessDenied";
        assert_eq!(
            details_in_words(chain, Locale::EnUs),
            "The privileged service refused. The password prompt was cancelled, so nothing changed.\nCaused by: The system said: org.freedesktop.DBus.Error.AccessDenied"
        );
        assert_eq!(
            details_in_words("daemon.error.apt_failed:better-files", Locale::ZhTw),
            "安裝套件失敗了（better-files）。"
        );
        assert_eq!(
            details_in_words("manager.error.not_cancelable:Downloading", Locale::EnUs),
            "This step can't be stopped once it has started."
        );
    }
}
