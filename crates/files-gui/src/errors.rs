//! What the storage service's, the disk service's and the job engine's
//! errors mean, in words a person can act on, in the window's language.
//!
//! The layers below report stable keys (`storage.erase.busy`,
//! `files.operation.error.no_space:/media/stick/a.iso`), or, for the disk
//! service, an English sentence of a fixed shape
//! (`unmount was refused for /org/freedesktop/UDisks2/block_devices/sdc1:
//! target is busy`), not translated text. Everything the window shows one in
//! (a notice, a failed job's line) passes it through [`explain`] first; the
//! sentences themselves are [`crate::i18n::ErrorCopy`], one table per
//! language. Text that is neither is shown as it is.

use crate::i18n::{Copy, ErrorCopy, ErrorLanguage};

/// The keys of the layers below that have no field of their own in
/// [`ErrorCopy`]: the key, the English sentence, and the Taiwan Mandarin one
/// (ticket 172). `{detail}` is what follows the key; a row named
/// `key:part` is used for a detail that starts with `part`, with `{path}`
/// and `{maximum}` filled in from the rest.
pub const SENTENCES: &[(&str, &str, &str)] = &[
    (
        "files.listing.error.not_listable",
        "This kind of place can't be shown as a list of files.",
        "這種位置無法以檔案清單顯示。",
    ),
    (
        "files.listing.error.permission_denied",
        "You don't have permission to open this folder ({detail}).",
        "你沒有權限開啟這個資料夾（{detail}）。",
    ),
    (
        "files.listing.error.not_found",
        "This folder no longer exists ({detail}).",
        "這個資料夾已經不存在（{detail}）。",
    ),
    (
        "files.listing.error.not_a_directory",
        "This is a file, not a folder ({detail}).",
        "這是檔案，不是資料夾（{detail}）。",
    ),
    (
        "files.listing.error.device_lost",
        "The drive this folder was on is no longer connected ({detail}).",
        "這個資料夾所在的磁碟已經中斷連線（{detail}）。",
    ),
    (
        "files.listing.error.symlink_loop",
        "A link here points back to itself, so it leads nowhere ({detail}).",
        "這裡有個連結指回自己，所以哪裡都到不了（{detail}）。",
    ),
    (
        "files.listing.error.name_too_long",
        "A name here is longer than the drive allows ({detail}).",
        "這裡有個名稱超過磁碟允許的長度（{detail}）。",
    ),
    (
        "files.listing.error.io",
        "This folder couldn't be read ({detail}). Retry, or check the drive.",
        "無法讀取這個資料夾（{detail}）。請重試，或檢查磁碟。",
    ),
    (
        "files.location.error.relative_path",
        "That isn't a full path. Start it with / ({detail}).",
        "這不是完整路徑，請以 / 開頭（{detail}）。",
    ),
    (
        "files.location.error.invalid_name",
        "That name can't be used ({detail}).",
        "這個名稱無法使用（{detail}）。",
    ),
    (
        "files.location.error.missing_authority",
        "The address is missing the server's name ({detail}).",
        "這個位址缺少伺服器名稱（{detail}）。",
    ),
    (
        "files.location.error.backend_unavailable",
        "This kind of place can't be opened on this computer ({detail}).",
        "這台電腦無法開啟這種位置（{detail}）。",
    ),
    (
        "files.navigation.error.no_such_tab",
        "That tab is already closed.",
        "這個分頁已經關閉了。",
    ),
    (
        "files.navigation.error.last_tab",
        "The last tab stays open.",
        "最後一個分頁會保持開啟。",
    ),
    (
        "files.navigation.error.nothing_to_restore",
        "There is no closed tab to reopen.",
        "沒有可以重新開啟的已關閉分頁。",
    ),
    (
        "files.platform.error.watch_failed",
        "Better Files can't watch this folder for changes, so it won't update by itself ({detail}).",
        "Better Files 無法監看這個資料夾的變動，所以它不會自動更新（{detail}）。",
    ),
    (
        "files.platform.error.read_failed",
        "This couldn't be read ({detail}).",
        "無法讀取（{detail}）。",
    ),
    (
        "files.platform.error.not_a_directory",
        "This is a file, not a folder ({detail}).",
        "這是檔案，不是資料夾（{detail}）。",
    ),
    (
        "files.trash.error.cross_device",
        "The item is on another drive than this trash ({detail}).",
        "這個項目和垃圾桶在不同的磁碟上（{detail}）。",
    ),
    (
        "files.trash.error.not_found",
        "The item is no longer there ({detail}).",
        "這個項目已經不在了（{detail}）。",
    ),
    (
        "files.trash.error.no_record",
        "The trash has no record of where this item came from ({detail}).",
        "垃圾桶沒有這個項目原本位置的紀錄（{detail}）。",
    ),
    (
        "files.trash.error.destination_occupied",
        "Something else is now where this item came from ({detail}).",
        "這個項目原本的位置現在有別的東西（{detail}）。",
    ),
    (
        "files.trash.error.original_parent_missing",
        "The folder this item came from no longer exists ({detail}).",
        "這個項目原本所在的資料夾已經不存在（{detail}）。",
    ),
    (
        "files.trash.error.unusable",
        "The trash folder can't be used safely: it is a link, not a folder, or belongs to someone else ({detail}).",
        "垃圾桶資料夾無法安全使用：它是連結、不是資料夾，或屬於其他人（{detail}）。",
    ),
    (
        "files.trash.error.io",
        "The trash couldn't be read or written ({detail}).",
        "無法讀取或寫入垃圾桶（{detail}）。",
    ),
    (
        "files.trash.error.no_home",
        "There is no home folder to keep the trash in.",
        "找不到存放垃圾桶的個人資料夾。",
    ),
    (
        "files.trash.error.no_free_name",
        "The trash already holds too many items with this name ({detail}). Empty the trash and try again.",
        "垃圾桶裡同名的項目太多了（{detail}）。請清空垃圾桶後再試。",
    ),
    (
        "files.trash.error.not_copyable",
        "This is a special file (a pipe, a socket, or a device) that the trash can't keep ({detail}).",
        "這是垃圾桶無法保存的特殊檔案（管道、通訊端或裝置）（{detail}）。",
    ),
    (
        "files.trash.error.no_uid",
        "Your user number couldn't be found, so the drive's own trash can't be used.",
        "找不到你的使用者編號，所以無法使用磁碟本身的垃圾桶。",
    ),
    (
        "files.trash.error.filesystem_not_answering",
        "A network or remote drive did not answer in time, so its trash isn't listed.",
        "有個網路或遠端磁碟沒有及時回應，所以沒有列出它的垃圾桶。",
    ),
    (
        "files.trash.shared.usable",
        "The drive's shared trash folder is used.",
        "使用磁碟的共用垃圾桶資料夾。",
    ),
    (
        "files.trash.shared.missing",
        "The drive has no shared trash folder, so a trash folder of your own is used.",
        "磁碟沒有共用垃圾桶資料夾，所以使用你自己的垃圾桶資料夾。",
    ),
    (
        "files.trash.shared.symlink",
        "The drive's shared trash folder is a link, so it isn't used; a trash folder of your own is used instead.",
        "磁碟的共用垃圾桶資料夾是連結，所以不使用它，改用你自己的垃圾桶資料夾。",
    ),
    (
        "files.trash.shared.not_a_directory",
        "The drive's shared trash is not a folder, so a trash folder of your own is used instead.",
        "磁碟的共用垃圾桶不是資料夾，所以改用你自己的垃圾桶資料夾。",
    ),
    (
        "files.trash.shared.not_sticky",
        "The drive's shared trash folder would let other people remove your items, so a trash folder of your own is used instead.",
        "磁碟的共用垃圾桶資料夾會讓其他人刪除你的項目，所以改用你自己的垃圾桶資料夾。",
    ),
    (
        "files.trash.shared.unreadable",
        "The drive's shared trash folder couldn't be checked, so a trash folder of your own is used instead.",
        "無法檢查磁碟的共用垃圾桶資料夾，所以改用你自己的垃圾桶資料夾。",
    ),
    (
        "files.trash.shared.user_directory_unusable",
        "Your folder inside the drive's shared trash can't be used, so a trash folder of your own is used instead.",
        "磁碟共用垃圾桶裡你的資料夾無法使用，所以改用你自己的垃圾桶資料夾。",
    ),
    (
        "files.archive.entry.absolute_path",
        "The entry names a full path, which would put it outside the folder.",
        "這個項目指定了完整路徑，會被放到資料夾外面。",
    ),
    (
        "files.archive.entry.parent_traversal",
        "The entry's name climbs out of the folder with “..”.",
        "這個項目的名稱用「..」跳出了資料夾。",
    ),
    (
        "files.archive.entry.unusable_name",
        "The entry's name can't be used on this drive.",
        "這個項目的名稱無法在這個磁碟上使用。",
    ),
    (
        "files.archive.entry.name_not_utf8",
        "The entry's name isn't text this archive format can store.",
        "這個項目的名稱不是這種壓縮格式能儲存的文字。",
    ),
    (
        "files.archive.entry.through_link",
        "The entry would be written through a link to somewhere else.",
        "這個項目會經由連結寫到別的地方。",
    ),
    (
        "files.archive.entry.replaces_existing",
        "The entry would replace something already in the folder.",
        "這個項目會取代資料夾裡已經有的東西。",
    ),
    (
        "files.archive.entry.link_outside",
        "The entry is a link that points outside the folder.",
        "這個項目是指向資料夾外面的連結。",
    ),
    (
        "files.archive.entry.hard_link_outside",
        "The entry is a hard link to a file outside the folder.",
        "這個項目是指向資料夾外面檔案的硬連結。",
    ),
    (
        "files.archive.entry.hard_link_target_missing",
        "The entry is a hard link to a file the archive doesn't contain.",
        "這個項目是指向壓縮檔裡沒有的檔案的硬連結。",
    ),
    (
        "files.archive.entry.special_entry_skipped",
        "A device, pipe, or socket in the archive was left out.",
        "壓縮檔裡的裝置、管道或通訊端沒有被解開。",
    ),
    (
        "files.archive.entry.special_file_skipped",
        "A device, pipe, or socket was left out of the archive.",
        "裝置、管道或通訊端沒有被放進壓縮檔。",
    ),
    (
        "files.archive.error.unknown_format",
        "Better Files doesn't recognize this kind of archive.",
        "Better Files 認不出這種壓縮檔。",
    ),
    (
        "files.archive.error.compressor_stopped",
        "Compressing stopped partway.",
        "壓縮到一半停止了。",
    ),
    (
        "files.archive.error.output_failed",
        "The archive couldn't be written.",
        "無法寫入壓縮檔。",
    ),
    (
        "files.archive.error.zip_end_record_missing",
        "The zip file is missing its table of contents, so it is damaged or incomplete.",
        "這個 zip 檔缺少目錄，所以已經損壞或不完整。",
    ),
    (
        "files.archive.error.zip64_end_record_unusable",
        "The zip file's table of contents can't be read, so it is damaged.",
        "無法讀取這個 zip 檔的目錄，所以它已經損壞。",
    ),
    (
        "files.archive.stopped",
        "The job was stopped.",
        "工作已經停止。",
    ),
    (
        "files.operation.error.archive_limit_exceeded:unpacked_bytes",
        "The archive unpacks to more than {maximum} bytes, which is more than Better Files unpacks ({path}).",
        "這個壓縮檔解開後超過 {maximum} 位元組，超過 Better Files 可以解開的大小（{path}）。",
    ),
    (
        "files.operation.error.archive_limit_exceeded:entries",
        "The archive holds more than {maximum} items, which is more than Better Files unpacks ({path}).",
        "這個壓縮檔裡超過 {maximum} 個項目，超過 Better Files 可以解開的數量（{path}）。",
    ),
    (
        "files.operation.error.archive_limit_exceeded:central_directory_bytes",
        "The zip file says its table of contents is larger than {maximum} bytes, so it isn't opened ({path}).",
        "這個 zip 檔宣稱它的目錄大於 {maximum} 位元組，所以不開啟它（{path}）。",
    ),
    (
        "files.job.store.error.io",
        "A saved job couldn't be read or written ({detail}).",
        "無法讀取或寫入已儲存的工作（{detail}）。",
    ),
    (
        "files.job.store.error.unreadable",
        "A saved job is damaged and couldn't be read ({detail}).",
        "有個已儲存的工作已損壞，無法讀取（{detail}）。",
    ),
    (
        "files.job.store.error.unsupported_schema",
        "A saved job was written by a newer Better Files ({detail}).",
        "有個已儲存的工作是由較新版的 Better Files 寫入的（{detail}）。",
    ),
    (
        "files.operation.hard_link.first_copy_changed",
        "A file linked to another one changed during the copy, so it was copied on its own.",
        "有個和其他檔案相連的檔案在複製時變動了，所以單獨複製它。",
    ),
    (
        "files.operation.hard_link.first_copy_gone",
        "The first copy of a linked file is gone, so this one was copied on its own.",
        "相連檔案的第一份複本不見了，所以單獨複製這一份。",
    ),
    (
        "files.preview.degraded.no_provider",
        "There is no preview for this kind of file.",
        "這種檔案沒有預覽。",
    ),
    (
        "files.preview.degraded.too_large",
        "This file is too large to preview.",
        "這個檔案太大，無法預覽。",
    ),
    (
        "files.preview.degraded.binary",
        "This file isn't text, so it can't be previewed as text.",
        "這個檔案不是文字，無法以文字預覽。",
    ),
    (
        "files.preview.degraded.decode_failed",
        "This file couldn't be decoded for a preview.",
        "無法解碼這個檔案來預覽。",
    ),
    (
        "files.preview.degraded.parser_faulted",
        "The preview stopped on a fault in the file.",
        "預覽因為檔案裡的錯誤而停止。",
    ),
    (
        "files.preview.degraded.unreadable",
        "This file couldn't be read for a preview.",
        "無法讀取這個檔案來預覽。",
    ),
    (
        "files.preview.error.cancelled",
        "The preview was stopped.",
        "預覽已經停止。",
    ),
    (
        "files.preview.error.not_handled",
        "There is no preview for this kind of file.",
        "這種檔案沒有預覽。",
    ),
    (
        "files.preview.error.degraded",
        "A full preview isn't possible.",
        "無法完整預覽。",
    ),
    (
        "storage.client.error.rejected",
        "The storage service refused.",
        "儲存服務拒絕了。",
    ),
    (
        "storage.unknown.ambiguous_identity",
        "Two drives look the same, so which one this is can't be told.",
        "有兩個磁碟看起來一樣，所以分不出這是哪一個。",
    ),
    (
        "storage.unknown.evidence_stale",
        "What is known about this drive is out of date.",
        "關於這個磁碟的資訊已經過時。",
    ),
    (
        "storage.unknown.filesystem_error",
        "The drive reported a filesystem error.",
        "磁碟回報了檔案系統錯誤。",
    ),
    (
        "storage.unknown.flush_failed",
        "Writing the last data to the drive failed.",
        "把最後的資料寫入磁碟時失敗了。",
    ),
    (
        "storage.unknown.not_yet_observed",
        "The drive hasn't been checked yet.",
        "還沒有檢查過這個磁碟。",
    ),
    (
        "storage.unknown.service_restarted",
        "The storage service restarted, so it doesn't know yet whether writes are pending.",
        "儲存服務重新啟動了，所以還不知道是否有尚未寫入的資料。",
    ),
    (
        "storage.unknown.signal_permission_denied",
        "Better OS isn't allowed to read whether this drive has pending writes.",
        "Better OS 沒有權限讀取這個磁碟是否還有待寫入的資料。",
    ),
    (
        "storage.unknown.signal_unavailable",
        "This drive doesn't say whether it has pending writes.",
        "這個磁碟沒有回報是否還有待寫入的資料。",
    ),
    (
        "storage.unknown.signal_unsupported",
        "This kind of drive can't report whether it has pending writes.",
        "這種磁碟無法回報是否還有待寫入的資料。",
    ),
    (
        "storage.writing.flush_in_progress",
        "Data is being written to the drive now.",
        "正在把資料寫入磁碟。",
    ),
    (
        "storage.writing.pending_writeback",
        "Some data hasn't been written to the drive yet.",
        "還有資料沒有寫入磁碟。",
    ),
    (
        "storage.writing.tracked_operation",
        "An application is still copying to this drive.",
        "還有應用程式正在複製到這個磁碟。",
    ),
    (
        "storage.performance.data_loss_on_direct_removal",
        "Unplugging without ejecting can lose what was being written.",
        "沒有退出就拔除，可能會遺失正在寫入的資料。",
    ),
    (
        "storage.performance.eject_required",
        "Eject the drive before unplugging it.",
        "拔除磁碟前請先退出。",
    ),
    (
        "storage.performance.throughput_tradeoff",
        "Writing is faster, and unplugging without ejecting is riskier.",
        "寫入會比較快，但沒有退出就拔除的風險比較高。",
    ),
    (
        "files.apps.warning.dropped_action_exec",
        "One of the application's actions has a command that can't be run, so it was left out ({detail}).",
        "這個應用程式有個動作的指令無法執行，所以沒有列出（{detail}）。",
    ),
    (
        "files.apps.warning.dropped_icon_path",
        "The application's icon file couldn't be used ({detail}).",
        "無法使用這個應用程式的圖示檔（{detail}）。",
    ),
    (
        "files.apps.warning.dropped_mime_type",
        "The application names a file type that isn't valid, so it was left out ({detail}).",
        "這個應用程式指定了無效的檔案類型，所以沒有列出（{detail}）。",
    ),
];

fn table_sentence(key: &str, e: &ErrorCopy) -> Option<&'static str> {
    SENTENCES
        .iter()
        .find(|(known, _, _)| *known == key)
        .map(|(_, english, chinese)| match e.language {
            ErrorLanguage::English => *english,
            ErrorLanguage::TraditionalChinese => *chinese,
        })
}

/// Where a key of this window's families starts inside `text`: at its start,
/// or right after a `:`.
fn nested_key_start(text: &str) -> Option<usize> {
    let starts_key = |rest: &str| {
        (rest.starts_with("files.") || rest.starts_with("storage."))
            && rest
                .split(':')
                .next()
                .is_some_and(|key| key.contains('.') && !key.contains(' '))
    };
    if starts_key(text) {
        return Some(0);
    }
    text.match_indices(':')
        .map(|(at, _)| at + 1)
        .find(|&at| starts_key(text[at..].trim_start()))
        .map(|at| at + (text[at..].len() - text[at..].trim_start().len()))
}

/// `sentence` with `{detail}` filled in. A detail that carries another
/// layer's key (`files.operation.error.trash_unavailable:files.trash.error.
/// no_home`) is not shown as a key: the part before it fills `{detail}`, or
/// the slot goes when there is none, and the key's own sentence follows.
fn with_detail(sentence: &str, detail: &str, c: &'static Copy) -> String {
    let without_slot = || {
        sentence
            .replace(" ({detail})", "")
            .replace("（{detail}）", "")
            .replace("{detail}", "")
    };
    if detail.trim().is_empty() {
        return without_slot();
    }
    let Some(at) = nested_key_start(detail) else {
        return sentence.replace("{detail}", detail);
    };
    let before = detail[..at].trim_end_matches(':').trim();
    let inner = explain(&detail[at..], c);
    let outer = if before.is_empty() {
        without_slot()
    } else {
        sentence.replace("{detail}", before)
    };
    if outer.ends_with('。') || outer.ends_with('：') {
        format!("{outer}{inner}")
    } else {
        format!("{outer} {inner}")
    }
}

/// The sentence for `text`, with what follows its key (a path, a device, a
/// detail) where the sentence has a place for it. Text that is no key or
/// disk service sentence this knows comes back unchanged.
pub fn explain(text: &str, c: &'static Copy) -> String {
    let e = &c.errors;
    // A refusal relayed by the storage client wraps the service's own key
    // or the disk service's sentence.
    let text = text
        .strip_prefix("storage.client.error.rejected:")
        .unwrap_or(text);
    // A key's detail may quote a disk service sentence; only text that is
    // not a key is one.
    if !text.starts_with("storage.")
        && !text.starts_with("files.")
        && let Some(sentence) = platform(text, e)
    {
        return sentence;
    }
    let (key, detail) = match text.split_once(':') {
        Some((key, detail)) if key.contains('.') && !key.contains(' ') => (key, detail),
        _ => (text, ""),
    };
    let detail = detail.trim();
    let sentence = match key {
        // --- The storage service -------------------------------------
        "storage.system.running_system" => e.system_running,
        "storage.system.folder" => e.system_folder,
        "storage.system.swap" => e.system_swap,
        "storage.system.table" => e.system_table,
        "storage.system.unknown" => e.system_unknown,
        "storage.erase.busy" => e.erase_busy,
        "storage.erase.device_changed" => e.erase_device_changed,
        "storage.erase.ambiguous" => e.erase_ambiguous,
        "storage.erase.not_confirmed" => e.erase_not_confirmed,
        "storage.image.unmount_failed" => {
            let device = detail.split(':').next().unwrap_or(detail).trim();
            if device.is_empty() {
                return e
                    .image_unmount_failed
                    .replace(" ({device})", "")
                    .replace("（{device}）", "");
            }
            return e.image_unmount_failed.replace("{device}", device);
        }
        "storage.image.not_a_drive" => e.image_not_a_drive,
        "storage.image.not_a_file" => e.image_not_a_file,
        "storage.format.label" => e.format_label,
        "storage.client.error.service_unavailable"
        | "storage.client.error.transport"
        | "storage.client.error.protocol"
        | "storage.client.error.protocol_version" => e.service_unavailable,
        // --- The job engine ------------------------------------------
        "files.operation.error.no_space" => e.no_space,
        "files.operation.error.quota_exceeded" => e.quota_exceeded,
        "files.operation.error.permission_denied" => e.permission_denied,
        "files.operation.error.read_only" => e.read_only,
        "files.operation.error.not_found" => e.not_found,
        "files.operation.error.already_exists" => e.already_exists,
        "files.operation.error.not_a_directory" => e.not_a_directory,
        "files.operation.error.is_a_directory" => e.is_a_directory,
        "files.operation.error.destination_inside_source" => e.destination_inside_source,
        "files.operation.error.replaces_source" => e.replaces_source,
        "files.operation.error.symlink_loop" => e.symlink_loop,
        "files.operation.error.name_too_long" => e.name_too_long,
        "files.operation.error.invalid_name" => e.invalid_name,
        "files.operation.error.device_lost" => e.device_lost,
        "files.operation.error.cross_device" => e.cross_device,
        "files.operation.error.externally_modified" => e.externally_modified,
        "files.operation.error.verification_failed" => e.verification_failed,
        "files.operation.error.confirmation_required" => e.confirmation_required,
        "files.operation.error.cancelled" => e.cancelled,
        "files.operation.error.interrupted" => e.interrupted,
        "files.operation.error.conflict_unresolved" => e.conflict_unresolved,
        "files.operation.error.trash_unavailable" => e.trash_unavailable,
        "files.operation.error.archive_entry_refused" => e.archive_entry_refused,
        "files.operation.error.archive_unreadable" => e.archive_unreadable,
        "files.operation.error.io" => e.io,
        // --- The window ----------------------------------------------
        "files.prefs.error.unwritable" => e.prefs_unwritable,
        "files.prefs.error.unreadable" => e.prefs_unreadable,
        "files.bookmarks.error.unwritable" => e.bookmarks_unwritable,
        "files.plugin.no_view" => e.plugin_no_view,
        "files.plugin.error.not_trusted" => e.plugin_not_trusted,
        "files.plugin.error.no_program" => e.plugin_no_program,
        "files.plugin.error.not_found" => e.plugin_not_found,
        "files.plugin.error.failed" if detail.is_empty() => e.plugin_failed_silently,
        "files.plugin.error.failed" => e.plugin_failed,
        "files.plugin.error.timed_out" => e.plugin_timed_out,
        "files.plugin.error.too_large" => e.plugin_too_large,
        // How large an archive may be: which limit it was says the sentence.
        "files.operation.error.archive_limit_exceeded" => {
            let mut parts = detail.rsplitn(3, ':');
            let (maximum, limit, path) = (parts.next(), parts.next(), parts.next());
            if let (Some(maximum), Some(limit), Some(path)) = (maximum, limit, path)
                && let Some(sentence) = table_sentence(&format!("{key}:{limit}"), e)
            {
                return sentence
                    .replace("{maximum}", maximum)
                    .replace("{path}", path);
            }
            e.archive_limit_exceeded
        }
        _ if table_sentence(key, e).is_some() => {
            return with_detail(table_sentence(key, e).unwrap_or_default(), detail, c);
        }
        // A key of this window's own families that has no sentence says so,
        // naming the key, rather than standing on screen alone (ticket 172).
        _ if detail_free_key(key) => return e.unknown_reason.replace("{key}", text),
        _ => return text.to_string(),
    };
    with_detail(sentence, detail, c)
}

/// Whether `key` is shaped like one of the keys this window is handed
/// (`files.…`, `storage.…`): lower-case dotted words, no spaces.
fn detail_free_key(key: &str) -> bool {
    (key.starts_with("files.") || key.starts_with("storage."))
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
}

/// The disk service's errors (`storage_platform::PlatformError`), which
/// reach the window as their English sentences.
fn platform(text: &str, e: &ErrorCopy) -> Option<String> {
    if text.starts_with("the storage service could not reach ") {
        return Some(e.platform_unreachable.to_string());
    }
    if let Some(device) = text.strip_prefix("no device is known at ") {
        return Some(
            e.platform_unknown_device
                .replace("{device}", &device_name(device)),
        );
    }
    let (operation, rest) = text
        .split_once(" was refused for ")
        .or_else(|| text.split_once(" is not supported for "))?;
    let (device, detail) = rest.split_once(": ").unwrap_or((rest, ""));
    let operation = match operation {
        "mount" => e.op_mount,
        "unmount" => e.op_unmount,
        "format" => e.op_format,
        "write image" => e.op_write_image,
        "filesystem access" => e.op_filesystem_access,
        _ => e.op_other,
    };
    Some(
        e.platform_refused
            .replace("{operation}", operation)
            .replace("{device}", &device_name(device))
            .replace("{detail}", &reason(detail, e)),
    )
}

/// Why the disk service refused, in the window's language where the reason
/// is one it gives in its own words or one UDisks names; anything else as
/// UDisks said it.
fn reason(detail: &str, e: &ErrorCopy) -> String {
    let lower = detail.to_ascii_lowercase();
    let known = match detail {
        "it belongs to the running system" => Some(e.why_system),
        "it is a partition, not a whole drive" => Some(e.why_partition),
        "the image is larger than the drive" => Some(e.why_image_too_large),
        _ if lower.contains("notauthorized") || lower.contains("not authorized") => {
            Some(e.why_not_authorized)
        }
        _ if lower.contains("error.cancelled") || lower.contains("dismissed") => {
            Some(e.why_cancelled)
        }
        _ if lower.contains("devicebusy") || lower.contains("target is busy") => Some(e.why_busy),
        _ => None,
    };
    known.map_or_else(|| detail.to_string(), str::to_string)
}

/// `/dev/sdc1` for UDisks' `/org/freedesktop/UDisks2/block_devices/sdc1`
/// (whose `_2d` is an escaped `-`); anything else as it is.
fn device_name(device: &str) -> String {
    let Some(name) = device.strip_prefix("/org/freedesktop/UDisks2/block_devices/") else {
        return device.to_string();
    };
    let mut out = String::from("/dev/");
    let mut rest = name;
    while let Some(at) = rest.find('_') {
        out.push_str(&rest[..at]);
        let escaped = rest.get(at + 1..at + 3);
        match escaped.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
            Some(byte) if byte.is_ascii() => {
                out.push(byte as char);
                rest = &rest[at + 3..];
            }
            _ => {
                out.push('_');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}
