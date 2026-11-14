//! What the storage service and the job engine's error keys mean, in words
//! a person can act on, in the window's language.
//!
//! The layers below report stable keys (`storage.erase.busy`,
//! `files.operation.error.no_space:/media/stick/a.iso`), not sentences, so a
//! translation never depends on them. Everything the window shows a key in
//! (a notice, a failed job's line) passes it through [`explain`] first; text
//! that is not a key is shown as it is.

use crate::i18n::{Copy, ZH_TW};

/// The key's sentence, with what follows its `:` (a path, a device, a
/// detail) where the sentence has a place for it. Text that is no key this
/// knows comes back unchanged.
pub fn explain(text: &str, c: &'static Copy) -> String {
    let zh = c.place_home == ZH_TW.place_home;
    // A refusal relayed by the storage client wraps the service's own key.
    let text = text
        .strip_prefix("storage.client.error.rejected:")
        .unwrap_or(text);
    let (key, detail) = match text.split_once(':') {
        Some((key, detail)) if key.contains('.') && !key.contains(' ') => (key, detail),
        _ => (text, ""),
    };
    let detail = detail.trim();
    let say = |en: &str, zh_text: &str| -> String {
        let sentence = if zh { zh_text } else { en };
        sentence.replace("{detail}", detail)
    };
    match key {
        // --- The storage service -------------------------------------
        "storage.erase.busy" => say(
            "Something is still writing to this drive. Wait until it finishes, then try again.",
            "還有東西正在寫入這個磁碟。請等它完成後再試一次。",
        ),
        "storage.erase.not_confirmed" => say(
            "Nothing was erased: the erase was not confirmed.",
            "沒有清除任何東西：清除動作沒有被確認。",
        ),
        "storage.image.unmount_failed" => {
            let device = detail.split(':').next().unwrap_or(detail);
            let sentence = if zh {
                "磁碟上的一個磁區（{device}）正在使用中，無法卸載，所以沒有寫入。請關閉使用它的程式後再試一次。"
            } else {
                "A volume on the drive ({device}) is in use and could not be unmounted, so nothing was written. Close what is using it and try again."
            };
            sentence.replace("{device}", device)
        }
        "storage.image.not_a_drive" => say(
            "An image can only be written to a whole drive, not to one volume on it.",
            "映像檔只能寫入整顆磁碟，不能只寫入其中一個磁區。",
        ),
        "storage.image.not_a_file" => say(
            "That image is not a file that can be read. Check that it is still there.",
            "這個映像檔不是可以讀取的檔案。請確認它還在原處。",
        ),
        "storage.format.label" => say(
            "That name does not fit this filesystem: it is too long, or has a character the filesystem cannot store. Choose a shorter or plainer name.",
            "這個名稱不適合這種檔案系統：太長，或含有它無法儲存的字元。請換一個較短或較簡單的名稱。",
        ),
        "storage.client.error.service_unavailable"
        | "storage.client.error.transport"
        | "storage.client.error.protocol"
        | "storage.client.error.protocol_version" => say(
            "The storage service did not answer. Try again in a moment.",
            "儲存服務沒有回應。請稍後再試一次。",
        ),
        // --- The job engine ------------------------------------------
        "files.operation.error.no_space" => say(
            "There is not enough space at {detail}. Free some space, then retry.",
            "{detail} 的空間不足。請騰出空間後再重試。",
        ),
        "files.operation.error.quota_exceeded" => say(
            "Your quota at {detail} is used up. Free some space, then retry.",
            "{detail} 的配額已用完。請騰出空間後再重試。",
        ),
        "files.operation.error.permission_denied" => say(
            "You are not allowed to change {detail}.",
            "你沒有權限變更 {detail}。",
        ),
        "files.operation.error.read_only" => say("{detail} is read-only.", "{detail} 是唯讀的。"),
        "files.operation.error.not_found" => {
            say("{detail} is no longer there.", "{detail} 已經不在了。")
        }
        "files.operation.error.already_exists" => say(
            "Something named like that is already at {detail}.",
            "{detail} 已經有同名的項目。",
        ),
        "files.operation.error.not_a_directory" => {
            say("{detail} is not a folder.", "{detail} 不是資料夾。")
        }
        "files.operation.error.is_a_directory" => {
            say("{detail} is a folder.", "{detail} 是資料夾。")
        }
        "files.operation.error.destination_inside_source" => say(
            "A folder can't be copied or moved into itself ({detail}).",
            "資料夾不能複製或移動到它自己裡面（{detail}）。",
        ),
        "files.operation.error.replaces_source" => say(
            "An item can't replace itself or the folder it is in ({detail}). Nothing was moved to the trash.",
            "項目不能取代它自己或它所在的資料夾（{detail}）。沒有任何東西被移到垃圾桶。",
        ),
        "files.operation.error.symlink_loop" => say(
            "{detail} links back into itself and was skipped.",
            "{detail} 的連結繞回自己，已略過。",
        ),
        "files.operation.error.name_too_long" => say(
            "The name is too long for {detail}. Choose a shorter one.",
            "名稱對 {detail} 來說太長了。請換一個較短的名稱。",
        ),
        "files.operation.error.invalid_name" => say(
            "That name can't be used ({detail}).",
            "這個名稱不能使用（{detail}）。",
        ),
        "files.operation.error.device_lost" => say(
            "The drive holding {detail} went away. Reconnect it, then retry.",
            "存放 {detail} 的磁碟不見了。請重新接上後再重試。",
        ),
        "files.operation.error.cross_device" => say(
            "{detail} can't be moved across drives that way.",
            "{detail} 無法以這種方式跨磁碟移動。",
        ),
        "files.operation.error.externally_modified" => say(
            "{detail} changed while it was being copied. Retry to copy it as it is now.",
            "{detail} 在複製途中被變更了。請重試以複製它目前的樣子。",
        ),
        "files.operation.error.verification_failed" => say(
            "The copy of {detail} does not match the original. Retry it.",
            "{detail} 的複本和原檔不一致。請重試。",
        ),
        "files.operation.error.confirmation_required" => {
            say("This needs to be confirmed first.", "這需要先確認。")
        }
        "files.operation.error.cancelled" => say("Stopped.", "已停止。"),
        "files.operation.error.interrupted" => say(
            "Better Files closed before this finished. It can be resumed from the file operations panel.",
            "Better Files 在這項作業完成前關閉了。可以從檔案作業面板繼續。",
        ),
        "files.operation.error.conflict_unresolved" => say(
            "{detail} was left as it was: the clash was not answered.",
            "{detail} 維持原樣：衝突沒有得到回答。",
        ),
        "files.operation.error.trash_unavailable" => say(
            "The trash can't be used here, so nothing was replaced ({detail}).",
            "這裡無法使用垃圾桶，所以沒有取代任何東西（{detail}）。",
        ),
        "files.operation.error.archive_entry_refused" => say(
            "An entry in the archive was refused because it would land outside its folder ({detail}).",
            "壓縮檔裡有一個項目會落在資料夾之外，已拒絕（{detail}）。",
        ),
        "files.operation.error.archive_limit_exceeded" => say(
            "The archive is larger than Better Files unpacks ({detail}).",
            "這個壓縮檔超過 Better Files 可以解開的大小（{detail}）。",
        ),
        "files.operation.error.archive_unreadable" => say(
            "The archive can't be read ({detail}).",
            "無法讀取這個壓縮檔（{detail}）。",
        ),
        "files.operation.error.io" => say(
            "Something went wrong reading or writing ({detail}). Retry, or check the drive.",
            "讀取或寫入時出了問題（{detail}）。請重試，或檢查磁碟。",
        ),
        // --- The window ----------------------------------------------
        "files.prefs.error.unwritable" => say(
            "Your settings could not be saved ({detail}).",
            "無法儲存你的設定（{detail}）。",
        ),
        "files.plugin.no_view" => say(
            "No plugin has a page named “{detail}”.",
            "沒有外掛提供名為「{detail}」的頁面。",
        ),
        _ => text.to_string(),
    }
}
