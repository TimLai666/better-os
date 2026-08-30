//! Every word Better Files can show, in both shipped languages.
//!
//! The pattern is Better Manager's and Better Monitor's: one `Copy` struct,
//! one constant per locale, and a compile error the moment a string is added
//! to one language and forgotten in the other. Switching language is a field
//! assignment, so it takes effect on the next frame with no reload.
//!
//! Nothing here formats a number or a size. That is [`crate::format`]'s job,
//! so both locales format identically and a translation cannot change what a
//! byte count means.

use files_core::OpenRefusal;
use files_operations::{Confidence, ConflictKind, JobState, OperationKind, Resolution};
use files_platform::UserDirectory;

use crate::prefs::{ItemScale, LocalePreference, ViewMode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    System,
    EnUs,
    ZhTw,
}

impl Locale {
    /// Resolves `System` against the session's language once, so the rest of
    /// the window never reads the environment.
    pub fn resolved(self) -> Self {
        match self {
            Self::System => {
                let language = std::env::var("LANG")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if language.contains("zh_tw")
                    || language.contains("zh-tw")
                    || language.contains("hant")
                {
                    Self::ZhTw
                } else {
                    Self::EnUs
                }
            }
            locale => locale,
        }
    }

    pub fn from_preference(preference: LocalePreference) -> Self {
        match preference {
            LocalePreference::System => Locale::System,
            LocalePreference::EnUs => Locale::EnUs,
            LocalePreference::ZhTw => Locale::ZhTw,
        }
    }

    pub fn to_preference(self) -> LocalePreference {
        match self {
            Locale::System => LocalePreference::System,
            Locale::EnUs => LocalePreference::EnUs,
            Locale::ZhTw => LocalePreference::ZhTw,
        }
    }
}

pub struct Copy {
    // Shell
    pub brand_name: &'static str,
    pub files: &'static str,
    pub language: &'static str,
    pub english: &'static str,
    pub chinese: &'static str,
    pub light_theme: &'static str,
    pub dark_theme: &'static str,

    // Toolbar
    pub go_back: &'static str,
    pub go_forward: &'static str,
    pub go_to_parent: &'static str,
    pub reload: &'static str,
    pub path_placeholder: &'static str,
    pub path_not_absolute: &'static str,
    pub path_empty: &'static str,
    pub path_not_found: &'static str,
    pub path_not_a_directory: &'static str,
    pub path_unsupported: &'static str,
    pub view_grid: &'static str,
    pub view_list: &'static str,
    pub show_hidden: &'static str,
    pub hide_hidden: &'static str,
    pub sort_by: &'static str,
    pub ascending: &'static str,
    pub descending: &'static str,
    pub folders_first: &'static str,
    pub item_size: &'static str,
    pub size_small: &'static str,
    pub size_medium: &'static str,
    pub size_large: &'static str,

    // Sort keys
    pub sort_name: &'static str,
    pub sort_modified: &'static str,
    pub sort_size: &'static str,
    pub sort_type: &'static str,
    pub sort_extension: &'static str,

    // Tabs
    pub new_tab: &'static str,
    pub close_tab: &'static str,
    pub reopen_closed_tab: &'static str,
    pub last_tab_stays_open: &'static str,
    pub nothing_to_reopen: &'static str,

    // Sidebar
    pub sidebar_places: &'static str,
    pub sidebar_devices: &'static str,
    pub sidebar_applications: &'static str,
    pub sidebar_favorites: &'static str,
    pub unavailable: &'static str,
    pub identity_volatile: &'static str,
    pub device_state_unknown_without_service: &'static str,
    pub no_favorites: &'static str,
    pub no_devices: &'static str,
    pub drop_to_pin: &'static str,
    pub pin_to_sidebar: &'static str,
    pub open: &'static str,
    pub open_in_new_tab: &'static str,
    pub rename_bookmark: &'static str,
    pub remove_from_sidebar: &'static str,
    pub move_up: &'static str,
    pub move_down: &'static str,
    pub already_pinned: &'static str,
    pub bookmark_label_placeholder: &'static str,

    // Built-in locations
    pub place_home: &'static str,
    pub place_desktop: &'static str,
    pub place_documents: &'static str,
    pub place_downloads: &'static str,
    pub place_music: &'static str,
    pub place_pictures: &'static str,
    pub place_videos: &'static str,
    pub place_templates: &'static str,
    pub place_public: &'static str,
    pub place_trash: &'static str,

    // Content
    pub loading: &'static str,
    pub empty_folder: &'static str,
    pub listing_failed: &'static str,
    pub listing_cancelled: &'static str,
    pub not_listable_here: &'static str,
    pub column_name: &'static str,
    pub column_size: &'static str,
    pub column_modified: &'static str,
    pub column_type: &'static str,
    pub column_extension: &'static str,
    pub kind_folder: &'static str,
    pub kind_file: &'static str,
    pub kind_application: &'static str,
    pub kind_special: &'static str,
    pub kind_unknown: &'static str,
    pub selected_count: &'static str,
    pub item_count: &'static str,
    pub hidden_shown: &'static str,
    pub skipped_entries: &'static str,

    // Opening
    pub no_handler_wired: &'static str,
    pub launching_not_wired: &'static str,
    pub refusal_broken_symlink: &'static str,
    pub refusal_symlink_loop: &'static str,
    pub refusal_in_trash: &'static str,
    pub refusal_not_openable: &'static str,

    // Operations
    pub operation_center: &'static str,
    pub no_operations: &'static str,
    pub pause: &'static str,
    pub resume: &'static str,
    pub cancel: &'static str,
    pub retry_failed: &'static str,
    pub throughput: &'static str,
    pub remaining: &'static str,
    pub confidence_high: &'static str,
    pub confidence_medium: &'static str,
    pub confidence_low: &'static str,
    pub confidence_none: &'static str,
    pub failures: &'static str,
    pub completed_this_session: &'static str,
    pub conflict_needs_a_decision: &'static str,
    pub conflict_exists: &'static str,
    pub conflict_no_space: &'static str,
    pub conflict_permission: &'static str,
    pub conflict_case: &'static str,
    pub resolution_skip: &'static str,
    pub resolution_overwrite: &'static str,
    pub resolution_rename: &'static str,
    pub resolution_cancel: &'static str,
    pub apply_to_remaining: &'static str,

    // Job states
    pub state_queued: &'static str,
    pub state_running: &'static str,
    pub state_paused: &'static str,
    pub state_waiting_on_conflict: &'static str,
    pub state_completed: &'static str,
    pub state_failed: &'static str,
    pub state_cancelled: &'static str,
    pub state_rolled_back: &'static str,

    // Job kinds
    pub job_create_file: &'static str,
    pub job_create_folder: &'static str,
    pub job_rename: &'static str,
    pub job_bulk_rename: &'static str,
    pub job_copy: &'static str,
    pub job_move: &'static str,
    pub job_duplicate: &'static str,
    pub job_trash: &'static str,
    pub job_restore: &'static str,
    pub job_permanent_delete: &'static str,
    pub job_checksum: &'static str,

    // Commands
    pub new_folder: &'static str,
    pub new_file: &'static str,
    pub rename: &'static str,
    pub copy_items: &'static str,
    pub cut_items: &'static str,
    pub paste_items: &'static str,
    pub duplicate: &'static str,
    pub move_to_trash: &'static str,
    pub delete_permanently: &'static str,
    pub restore_from_trash: &'static str,
    pub confirm_delete_title: &'static str,
    pub confirm_delete_body: &'static str,
    pub confirm: &'static str,
    pub dismiss: &'static str,
    pub nothing_selected: &'static str,
    pub not_writable_here: &'static str,
    pub new_folder_name: &'static str,
    pub new_file_name: &'static str,
    pub rename_to: &'static str,
    pub name_not_usable: &'static str,
}

pub const EN_US: Copy = Copy {
    brand_name: "Better",
    files: "Files",
    language: "Language",
    english: "English",
    chinese: "繁體中文",
    light_theme: "Light",
    dark_theme: "Dark",

    go_back: "Back",
    go_forward: "Forward",
    go_to_parent: "Up",
    reload: "Reload",
    path_placeholder: "Type a location",
    path_not_absolute: "Enter a full path starting with /",
    path_empty: "Enter a location",
    path_not_found: "That location does not exist",
    path_not_a_directory: "That is a file, not a folder",
    path_unsupported: "This build cannot open that kind of location",
    view_grid: "Grid",
    view_list: "List",
    show_hidden: "Show hidden entries",
    hide_hidden: "Hide hidden entries",
    sort_by: "Sort by",
    ascending: "Ascending",
    descending: "Descending",
    folders_first: "Folders first",
    item_size: "Item size",
    size_small: "Small",
    size_medium: "Medium",
    size_large: "Large",

    sort_name: "Name",
    sort_modified: "Modified",
    sort_size: "Size",
    sort_type: "Type",
    sort_extension: "Extension",

    new_tab: "New tab",
    close_tab: "Close tab",
    reopen_closed_tab: "Reopen closed tab",
    last_tab_stays_open: "The last tab stays open",
    nothing_to_reopen: "No closed tab to reopen",

    sidebar_places: "Places",
    sidebar_devices: "Devices",
    sidebar_applications: "Applications",
    sidebar_favorites: "Favorites",
    unavailable: "Unavailable",
    identity_volatile: "Identity valid for this connection only",
    device_state_unknown_without_service: "Removal state unknown without the storage service",
    no_favorites: "Drag a folder here to pin it",
    no_devices: "No external devices",
    drop_to_pin: "Drop to pin",
    pin_to_sidebar: "Pin to sidebar",
    open: "Open",
    open_in_new_tab: "Open in New Tab",
    rename_bookmark: "Rename Bookmark",
    remove_from_sidebar: "Remove from Sidebar",
    move_up: "Move up",
    move_down: "Move down",
    already_pinned: "Already in Favorites",
    bookmark_label_placeholder: "Bookmark label",

    place_home: "Home",
    place_desktop: "Desktop",
    place_documents: "Documents",
    place_downloads: "Downloads",
    place_music: "Music",
    place_pictures: "Pictures",
    place_videos: "Videos",
    place_templates: "Templates",
    place_public: "Public",
    place_trash: "Trash",

    loading: "Loading",
    empty_folder: "This folder is empty",
    listing_failed: "This folder could not be read",
    listing_cancelled: "Listing stopped",
    not_listable_here: "This build cannot list this location",
    column_name: "Name",
    column_size: "Size",
    column_modified: "Modified",
    column_type: "Type",
    column_extension: "Extension",
    kind_folder: "Folder",
    kind_file: "File",
    kind_application: "Application",
    kind_special: "System object",
    kind_unknown: "Unknown",
    selected_count: "selected",
    item_count: "items",
    hidden_shown: "hidden entries shown",
    skipped_entries: "entries could not be read",

    no_handler_wired: "No application is wired up yet",
    launching_not_wired: "Launching applications arrives with the Applications location",
    refusal_broken_symlink: "This link points at something that is not there",
    refusal_symlink_loop: "This link points at itself",
    refusal_in_trash: "Restore this item before opening it",
    refusal_not_openable: "This is not something that can be opened",

    operation_center: "Operations",
    no_operations: "Nothing is running",
    pause: "Pause",
    resume: "Resume",
    cancel: "Cancel",
    retry_failed: "Retry failed items",
    throughput: "Speed",
    remaining: "Remaining",
    confidence_high: "reliable estimate",
    confidence_medium: "steady estimate",
    confidence_low: "rough estimate",
    confidence_none: "no estimate yet",
    failures: "Failed items",
    completed_this_session: "Finished this session",
    conflict_needs_a_decision: "Waiting for a decision",
    conflict_exists: "Something is already there",
    conflict_no_space: "The destination is full",
    conflict_permission: "Permission denied",
    conflict_case: "A name that differs only in case is already there",
    resolution_skip: "Skip",
    resolution_overwrite: "Replace",
    resolution_rename: "Keep both",
    resolution_cancel: "Stop the job",
    apply_to_remaining: "Apply to the rest",

    state_queued: "Queued",
    state_running: "Running",
    state_paused: "Paused",
    state_waiting_on_conflict: "Waiting",
    state_completed: "Completed",
    state_failed: "Failed",
    state_cancelled: "Cancelled",
    state_rolled_back: "Rolled back",

    job_create_file: "New file",
    job_create_folder: "New folder",
    job_rename: "Rename",
    job_bulk_rename: "Bulk rename",
    job_copy: "Copy",
    job_move: "Move",
    job_duplicate: "Duplicate",
    job_trash: "Move to Trash",
    job_restore: "Restore",
    job_permanent_delete: "Delete permanently",
    job_checksum: "Checksum",

    new_folder: "New Folder",
    new_file: "New File",
    rename: "Rename",
    copy_items: "Copy",
    cut_items: "Cut",
    paste_items: "Paste",
    duplicate: "Duplicate",
    move_to_trash: "Move to Trash",
    delete_permanently: "Delete Permanently",
    restore_from_trash: "Put Back",
    confirm_delete_title: "Delete permanently?",
    confirm_delete_body: "These items are removed immediately and are not recoverable.",
    confirm: "Delete",
    dismiss: "Dismiss",
    nothing_selected: "Select something first",
    not_writable_here: "This location cannot be written to",
    new_folder_name: "Folder name",
    new_file_name: "File name",
    rename_to: "New name",
    name_not_usable: "That name cannot be used",
};

pub const ZH_TW: Copy = Copy {
    brand_name: "Better",
    files: "檔案",
    language: "語言",
    english: "English",
    chinese: "繁體中文",
    light_theme: "淺色",
    dark_theme: "深色",

    go_back: "上一頁",
    go_forward: "下一頁",
    go_to_parent: "上一層",
    reload: "重新載入",
    path_placeholder: "輸入位置",
    path_not_absolute: "請輸入以 / 開頭的完整路徑",
    path_empty: "請輸入位置",
    path_not_found: "找不到這個位置",
    path_not_a_directory: "這是檔案，不是資料夾",
    path_unsupported: "這個版本無法開啟這種位置",
    view_grid: "格狀",
    view_list: "清單",
    show_hidden: "顯示隱藏項目",
    hide_hidden: "隱藏隱藏項目",
    sort_by: "排序依據",
    ascending: "遞增",
    descending: "遞減",
    folders_first: "資料夾優先",
    item_size: "項目大小",
    size_small: "小",
    size_medium: "中",
    size_large: "大",

    sort_name: "名稱",
    sort_modified: "修改時間",
    sort_size: "大小",
    sort_type: "類型",
    sort_extension: "副檔名",

    new_tab: "新分頁",
    close_tab: "關閉分頁",
    reopen_closed_tab: "重新開啟分頁",
    last_tab_stays_open: "最後一個分頁會保留",
    nothing_to_reopen: "沒有可重新開啟的分頁",

    sidebar_places: "位置",
    sidebar_devices: "裝置",
    sidebar_applications: "應用程式",
    sidebar_favorites: "我的最愛",
    unavailable: "無法使用",
    identity_volatile: "此識別僅在這次連接有效",
    device_state_unknown_without_service: "沒有儲存服務時無法判斷可否拔除",
    no_favorites: "把資料夾拖到這裡即可釘選",
    no_devices: "沒有外接裝置",
    drop_to_pin: "放開以釘選",
    pin_to_sidebar: "釘選到側邊欄",
    open: "開啟",
    open_in_new_tab: "在新分頁開啟",
    rename_bookmark: "重新命名書籤",
    remove_from_sidebar: "從側邊欄移除",
    move_up: "上移",
    move_down: "下移",
    already_pinned: "已經在我的最愛",
    bookmark_label_placeholder: "書籤名稱",

    place_home: "家目錄",
    place_desktop: "桌面",
    place_documents: "文件",
    place_downloads: "下載",
    place_music: "音樂",
    place_pictures: "圖片",
    place_videos: "影片",
    place_templates: "範本",
    place_public: "公用",
    place_trash: "垃圾桶",

    loading: "載入中",
    empty_folder: "這個資料夾是空的",
    listing_failed: "無法讀取這個資料夾",
    listing_cancelled: "已停止讀取",
    not_listable_here: "這個版本無法列出這個位置",
    column_name: "名稱",
    column_size: "大小",
    column_modified: "修改時間",
    column_type: "類型",
    column_extension: "副檔名",
    kind_folder: "資料夾",
    kind_file: "檔案",
    kind_application: "應用程式",
    kind_special: "系統物件",
    kind_unknown: "未知",
    selected_count: "個已選取",
    item_count: "個項目",
    hidden_shown: "個隱藏項目已顯示",
    skipped_entries: "個項目無法讀取",

    no_handler_wired: "還沒有接上可以開啟的應用程式",
    launching_not_wired: "啟動應用程式會隨應用程式位置一起提供",
    refusal_broken_symlink: "這個連結指向不存在的目標",
    refusal_symlink_loop: "這個連結指向自己",
    refusal_in_trash: "請先還原再開啟",
    refusal_not_openable: "這個項目無法開啟",

    operation_center: "操作",
    no_operations: "目前沒有進行中的工作",
    pause: "暫停",
    resume: "繼續",
    cancel: "取消",
    retry_failed: "重試失敗項目",
    throughput: "速度",
    remaining: "剩餘",
    confidence_high: "估計可靠",
    confidence_medium: "估計穩定",
    confidence_low: "估計粗略",
    confidence_none: "尚無法估計",
    failures: "失敗項目",
    completed_this_session: "這次工作階段已完成",
    conflict_needs_a_decision: "等待你的決定",
    conflict_exists: "目的地已經有同名項目",
    conflict_no_space: "目的地空間不足",
    conflict_permission: "沒有權限",
    conflict_case: "目的地已有只差大小寫的同名項目",
    resolution_skip: "略過",
    resolution_overwrite: "取代",
    resolution_rename: "兩個都保留",
    resolution_cancel: "停止這項工作",
    apply_to_remaining: "套用到其餘項目",

    state_queued: "排隊中",
    state_running: "進行中",
    state_paused: "已暫停",
    state_waiting_on_conflict: "等待決定",
    state_completed: "已完成",
    state_failed: "失敗",
    state_cancelled: "已取消",
    state_rolled_back: "已復原",

    job_create_file: "新增檔案",
    job_create_folder: "新增資料夾",
    job_rename: "重新命名",
    job_bulk_rename: "批次重新命名",
    job_copy: "複製",
    job_move: "移動",
    job_duplicate: "製作副本",
    job_trash: "移到垃圾桶",
    job_restore: "還原",
    job_permanent_delete: "永久刪除",
    job_checksum: "計算校驗值",

    new_folder: "新增資料夾",
    new_file: "新增檔案",
    rename: "重新命名",
    copy_items: "複製",
    cut_items: "剪下",
    paste_items: "貼上",
    duplicate: "製作副本",
    move_to_trash: "移到垃圾桶",
    delete_permanently: "永久刪除",
    restore_from_trash: "放回原處",
    confirm_delete_title: "要永久刪除嗎？",
    confirm_delete_body: "這些項目會立刻移除，而且無法復原。",
    confirm: "刪除",
    dismiss: "關閉",
    nothing_selected: "請先選取項目",
    not_writable_here: "這個位置無法寫入",
    new_folder_name: "資料夾名稱",
    new_file_name: "檔案名稱",
    rename_to: "新名稱",
    name_not_usable: "無法使用這個名稱",
};

pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}

pub fn view_mode_label(mode: ViewMode, c: &'static Copy) -> &'static str {
    match mode {
        ViewMode::Grid => c.view_grid,
        ViewMode::List => c.view_list,
    }
}

pub fn scale_label(scale: ItemScale, c: &'static Copy) -> &'static str {
    match scale {
        ItemScale::Small => c.size_small,
        ItemScale::Medium => c.size_medium,
        ItemScale::Large => c.size_large,
    }
}

pub fn user_directory_label(directory: UserDirectory, c: &'static Copy) -> &'static str {
    match directory {
        UserDirectory::Home => c.place_home,
        UserDirectory::Desktop => c.place_desktop,
        UserDirectory::Documents => c.place_documents,
        UserDirectory::Downloads => c.place_downloads,
        UserDirectory::Music => c.place_music,
        UserDirectory::Pictures => c.place_pictures,
        UserDirectory::Videos => c.place_videos,
        UserDirectory::Templates => c.place_templates,
        UserDirectory::PublicShare => c.place_public,
    }
}

pub fn job_state_label(state: JobState, c: &'static Copy) -> &'static str {
    match state {
        JobState::Queued => c.state_queued,
        JobState::Running => c.state_running,
        JobState::Paused => c.state_paused,
        JobState::WaitingOnConflict => c.state_waiting_on_conflict,
        JobState::Completed => c.state_completed,
        JobState::Failed => c.state_failed,
        JobState::Cancelled => c.state_cancelled,
        JobState::RolledBack => c.state_rolled_back,
    }
}

pub fn job_kind_label(kind: OperationKind, c: &'static Copy) -> &'static str {
    match kind {
        OperationKind::CreateFile => c.job_create_file,
        OperationKind::CreateFolder => c.job_create_folder,
        OperationKind::Rename => c.job_rename,
        OperationKind::BulkRename => c.job_bulk_rename,
        OperationKind::Copy => c.job_copy,
        OperationKind::Move => c.job_move,
        OperationKind::Duplicate => c.job_duplicate,
        OperationKind::Trash => c.job_trash,
        OperationKind::RestoreFromTrash => c.job_restore,
        OperationKind::PermanentDelete => c.job_permanent_delete,
        OperationKind::Checksum => c.job_checksum,
    }
}

pub fn confidence_label(confidence: Confidence, c: &'static Copy) -> &'static str {
    match confidence {
        Confidence::High => c.confidence_high,
        Confidence::Medium => c.confidence_medium,
        Confidence::Low => c.confidence_low,
        Confidence::None => c.confidence_none,
    }
}

pub fn conflict_label(kind: ConflictKind, c: &'static Copy) -> &'static str {
    match kind {
        ConflictKind::Exists => c.conflict_exists,
        ConflictKind::NoSpace => c.conflict_no_space,
        ConflictKind::Permission => c.conflict_permission,
        ConflictKind::CaseConflict => c.conflict_case,
    }
}

pub fn resolution_label(resolution: Resolution, c: &'static Copy) -> &'static str {
    match resolution {
        Resolution::Skip => c.resolution_skip,
        Resolution::Overwrite => c.resolution_overwrite,
        Resolution::Rename => c.resolution_rename,
        Resolution::Cancel => c.resolution_cancel,
    }
}

pub fn refusal_label(refusal: OpenRefusal, c: &'static Copy) -> &'static str {
    match refusal {
        OpenRefusal::BrokenSymlink => c.refusal_broken_symlink,
        OpenRefusal::SymlinkLoop => c.refusal_symlink_loop,
        OpenRefusal::ItemIsInTrash => c.refusal_in_trash,
        OpenRefusal::NotOpenable => c.refusal_not_openable,
    }
}
