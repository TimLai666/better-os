//! Every keyboard shortcut, as the `?` sheet lists it.
//!
//! folio's `HelpOverlay.qml` lists its keys and leaves a few out (`Alt+D`,
//! `Alt+Up`, `Shift+F10`, `F1`); this sheet lists every binding
//! [`crate::keys::command_for`] has, and a test asks that function about each
//! line so the sheet cannot drift from what the keys really do.

use crate::i18n::Locale;

/// One line of the sheet: the keys, and what they do in each language.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shortcut {
    pub keys: &'static str,
    pub english: &'static str,
    pub chinese: &'static str,
}

impl Shortcut {
    pub fn describe(&self, locale: Locale) -> &'static str {
        match locale.resolved() {
            Locale::ZhTw => self.chinese,
            _ => self.english,
        }
    }
}

const fn line(keys: &'static str, english: &'static str, chinese: &'static str) -> Shortcut {
    Shortcut {
        keys,
        english,
        chinese,
    }
}

/// The sheet, in reading order.
pub const SHORTCUTS: &[Shortcut] = &[
    line("← ↑ → ↓", "Move the selection", "移動選取"),
    line("Shift+↑ / Shift+↓", "Select a range", "選取範圍"),
    line(
        "Ctrl+click / Alt+click",
        "Add or remove one item",
        "加入或移除一個項目",
    ),
    line("Shift+click", "Select a range", "選取範圍"),
    line("Ctrl+A", "Select everything", "全選"),
    line("Home / End", "First or last item", "第一個或最後一個項目"),
    line(
        "Page Up / Page Down",
        "A screen up or down",
        "上一頁或下一頁",
    ),
    line("Enter", "Open", "開啟"),
    line(
        "Backspace / Alt+↑",
        "Go to the parent folder",
        "到上一層資料夾",
    ),
    line("Alt+← / Alt+→", "Back / forward", "上一頁／下一頁"),
    line("Alt+Home", "Go home", "回家目錄"),
    line("Ctrl+L / Alt+D", "Type a location", "輸入位置"),
    line(
        "/ or Ctrl+F",
        "Search this folder and below",
        "搜尋這個資料夾及子資料夾",
    ),
    line(
        "Ctrl+Shift+F",
        "Filter this folder by name",
        "依名稱篩選這個資料夾",
    ),
    line(
        "Ctrl+1 / Ctrl+2 / Ctrl+3",
        "Grid, list or columns",
        "格狀、清單或欄位",
    ),
    line(
        "Ctrl++ / Ctrl+− / Ctrl+0",
        "Larger, smaller or normal items",
        "放大、縮小或一般大小",
    ),
    line("Ctrl+H", "Show or hide hidden files", "顯示或隱藏隱藏檔"),
    line(
        "Space",
        "Preview in its own window (← → next, Space closes)",
        "在獨立視窗預覽（← → 切換，空白鍵關閉）",
    ),
    line("Ctrl+I", "Details of an application", "應用程式詳細資料"),
    line("Ctrl+P", "Pin or unpin the folder", "釘選或取消釘選資料夾"),
    line(
        "Ctrl+C / Ctrl+X / Ctrl+V",
        "Copy, cut, paste",
        "複製、剪下、貼上",
    ),
    line("Ctrl+Shift+C", "Copy the path as text", "複製路徑文字"),
    line("F2", "Rename", "重新命名"),
    line("Ctrl+D / Ctrl+Shift+D", "Duplicate", "建立複本"),
    line("Ctrl+Shift+N", "New folder", "新增資料夾"),
    line(
        "Ctrl+Shift+P / Ctrl+Shift+E",
        "Compress / extract",
        "壓縮／解壓縮",
    ),
    line(
        "Ctrl+Shift+O",
        "Open with another application",
        "用其他應用程式開啟",
    ),
    line("Delete", "Move to the trash", "移到垃圾桶"),
    line(
        "Shift+Delete",
        "Delete permanently (asks first)",
        "永久刪除（會先詢問）",
    ),
    line("Ctrl+Z", "Undo", "復原"),
    line("Ctrl+Shift+Z / Ctrl+Y", "Redo", "重做"),
    line("Ctrl+O", "File operations", "檔案作業"),
    line(
        "Shift+F10 / Menu / Alt+Enter",
        "The menu for the selection",
        "選取項目的選單",
    ),
    line("Ctrl+N", "New window", "新視窗"),
    line(
        "Ctrl+Alt+N / Ctrl+W",
        "Split view / close the right pane",
        "分割檢視／關閉右側窗格",
    ),
    line("Mouse back / forward", "Back / forward", "上一頁／下一頁"),
    line("Ctrl+wheel", "Larger or smaller items", "放大或縮小項目"),
    line("Ctrl+T / Ctrl+W", "New tab / close tab", "新分頁／關閉分頁"),
    line(
        "Ctrl+Shift+T",
        "Reopen the closed tab",
        "重新開啟關閉的分頁",
    ),
    line(
        "Ctrl+Tab / Ctrl+Shift+Tab",
        "Next / previous tab",
        "下一個／上一個分頁",
    ),
    line(
        "Alt+↑ / Alt+↓ (sidebar)",
        "Move a pinned folder",
        "移動釘選的資料夾",
    ),
    line("Delete (sidebar)", "Unpin the folder", "取消釘選資料夾"),
    line("F5 / Ctrl+R", "Reload", "重新整理"),
    line(
        "Escape",
        "Close what is open, clear the selection, then stop the running job",
        "關閉開啟的項目、清除選取，再按一次停止進行中的作業",
    ),
    line(
        "Ctrl+F6",
        "Move between the sidebar and the files",
        "在側邊欄和檔案之間切換",
    ),
    line("? / F1", "Show these shortcuts", "顯示這些快速鍵"),
];
