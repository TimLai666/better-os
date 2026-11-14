//! What a right click offers, decided without a window.
//!
//! The menus follow folio's `FileMenu.qml`, `DriveMenu.qml` and the sidebar's
//! own menu: a file gets Open, Open With, Preview, Rename, Duplicate, Copy,
//! Cut, Copy Path, Compress or Extract, and Trash; a folder adds Open in New
//! Tab and Pin; empty space gets Paste, New Folder, New File, and Show Hidden
//! Files; the Trash gets Restore and Delete Permanently; a pinned folder in
//! the sidebar gets Rename, Move Up, Move Down, and Unpin; a drive gets Open
//! and Eject. Each line is a [`MenuCommand`] the session carries out, so the
//! menu a test reads is the menu the window draws.

use files_core::{SortDirection, SortKey, SortOrder};

use crate::commands::ArchiveActions;
use crate::i18n::Copy;

/// What a right click landed on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuTarget {
    /// The selected entries in the content area (the click selected the row
    /// first when it was not already selected).
    Selection,
    /// Empty space in the content area.
    Background,
    /// A row of the sidebar: its index in the sidebar's own list.
    Sidebar(usize),
}

/// One thing a menu line does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuCommand {
    Open,
    OpenInNewTab,
    OpenWith,
    Preview,
    Pin,
    Unpin,
    Rename,
    Duplicate,
    Copy,
    Cut,
    Paste,
    CopyPath,
    NewFolder,
    NewFile,
    Compress,
    Extract,
    ToggleHidden,
    SelectAll,
    MoveToTrash,
    DeletePermanently,
    Restore,
    EmptyTrash,
    OpenTerminal,
    // The sidebar.
    RenameBookmark,
    MoveBookmarkUp,
    MoveBookmarkDown,
    MoveBookmarkToTop,
    MoveBookmarkToBottom,
    RemoveBookmark,
    Eject,
    Mount,
    Format,
}

/// One line: a command with its words and key, or a divider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuLine {
    Command {
        command: MenuCommand,
        label: &'static str,
        shortcut: Option<&'static str>,
        enabled: bool,
        /// `Some` for a line drawn with a check mark when on.
        checked: Option<bool>,
    },
    Separator,
}

impl MenuLine {
    fn new(command: MenuCommand, label: &'static str, shortcut: Option<&'static str>) -> Self {
        MenuLine::Command {
            command,
            label,
            shortcut,
            enabled: true,
            checked: None,
        }
    }

    fn enabled(self, on: bool) -> Self {
        match self {
            MenuLine::Command {
                command,
                label,
                shortcut,
                checked,
                ..
            } => MenuLine::Command {
                command,
                label,
                shortcut,
                enabled: on,
                checked,
            },
            other => other,
        }
    }

    fn checked(self, on: bool) -> Self {
        match self {
            MenuLine::Command {
                command,
                label,
                shortcut,
                enabled,
                ..
            } => MenuLine::Command {
                command,
                label,
                shortcut,
                enabled,
                checked: Some(on),
            },
            other => other,
        }
    }

    pub fn command(&self) -> Option<MenuCommand> {
        match self {
            MenuLine::Command { command, .. } => Some(*command),
            MenuLine::Separator => None,
        }
    }
}

/// What the content-area menus need to know about the selection and the
/// folder.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionFacts {
    pub count: usize,
    /// Exactly one entry, and it is a folder.
    pub single_folder: bool,
    /// That folder is already in the sidebar.
    pub single_folder_pinned: bool,
    /// Every selected entry is a real file or folder, so it has a path.
    pub all_local: bool,
    pub archive: ArchiveActions,
    /// The folder can be written into.
    pub writable: bool,
    pub in_trash: bool,
    pub can_paste: bool,
    pub showing_hidden: bool,
    /// The folder (or the Trash) holds anything at all.
    pub has_items: bool,
}

/// The menu for the selected entries.
pub fn selection_menu(facts: &SelectionFacts, c: &'static Copy) -> Vec<MenuLine> {
    use MenuCommand as M;
    if facts.in_trash {
        return vec![
            MenuLine::new(M::Restore, c.restore_from_trash, None),
            MenuLine::Separator,
            MenuLine::new(
                M::DeletePermanently,
                c.delete_permanently,
                Some("Shift+Delete"),
            ),
        ];
    }
    let single = facts.count == 1;
    let mut lines = vec![MenuLine::new(M::Open, c.open, Some("Enter"))];
    if facts.single_folder {
        lines.push(MenuLine::new(M::OpenInNewTab, c.open_in_new_tab, None));
    } else {
        lines.push(MenuLine::new(M::OpenWith, c.open_with, Some("Ctrl+Shift+O")).enabled(single));
    }
    lines.push(MenuLine::new(M::Preview, c.preview, Some("Space")).enabled(single));
    if facts.single_folder {
        lines.push(if facts.single_folder_pinned {
            MenuLine::new(M::Unpin, c.unpin, Some("Ctrl+P"))
        } else {
            MenuLine::new(M::Pin, c.pin_to_sidebar, Some("Ctrl+P"))
        });
    }
    lines.push(MenuLine::Separator);
    lines.push(MenuLine::new(M::Cut, c.cut_items, Some("Ctrl+X")).enabled(facts.all_local));
    lines.push(MenuLine::new(M::Copy, c.copy_items, Some("Ctrl+C")).enabled(facts.all_local));
    if facts.single_folder && facts.can_paste {
        lines.push(MenuLine::new(M::Paste, c.paste_into_folder, Some("Ctrl+V")));
    }
    lines.push(
        MenuLine::new(M::CopyPath, c.copy_path, Some("Ctrl+Shift+C")).enabled(facts.all_local),
    );
    lines.push(MenuLine::Separator);
    lines.push(
        MenuLine::new(M::Rename, c.rename, Some("F2"))
            .enabled(single && facts.all_local && facts.writable),
    );
    lines.push(
        MenuLine::new(M::Duplicate, c.duplicate, Some("Ctrl+Shift+D"))
            .enabled(facts.all_local && facts.writable),
    );
    if facts.archive.extract {
        lines.push(MenuLine::new(M::Extract, c.extract, Some("Ctrl+Shift+E")));
    }
    lines.push(
        MenuLine::new(M::Compress, c.compress, Some("Ctrl+Shift+P"))
            .enabled(facts.archive.compress),
    );
    lines.push(MenuLine::Separator);
    lines.push(
        MenuLine::new(M::MoveToTrash, c.move_to_trash, Some("Delete"))
            .enabled(facts.all_local && facts.writable),
    );
    lines
}

/// The menu for empty space in a folder.
pub fn background_menu(facts: &SelectionFacts, c: &'static Copy) -> Vec<MenuLine> {
    use MenuCommand as M;
    if facts.in_trash {
        return vec![
            MenuLine::new(M::SelectAll, c.select_all, Some("Ctrl+A")),
            MenuLine::Separator,
            MenuLine::new(M::EmptyTrash, c.empty_trash, None).enabled(facts.has_items),
        ];
    }
    vec![
        MenuLine::new(M::OpenTerminal, c.open_terminal_here, None),
        MenuLine::Separator,
        MenuLine::new(M::Paste, c.paste_items, Some("Ctrl+V"))
            .enabled(facts.can_paste && facts.writable),
        MenuLine::new(M::CopyPath, c.copy_path, Some("Ctrl+Shift+C")),
        MenuLine::Separator,
        MenuLine::new(M::NewFolder, c.new_folder, Some("Ctrl+Shift+N")).enabled(facts.writable),
        MenuLine::new(M::NewFile, c.new_file, None).enabled(facts.writable),
        MenuLine::Separator,
        MenuLine::new(M::SelectAll, c.select_all, Some("Ctrl+A")),
        MenuLine::new(M::ToggleHidden, c.show_hidden, Some("Ctrl+H")).checked(facts.showing_hidden),
    ]
}

/// What a sidebar row is, for its menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidebarKind {
    /// A place such as Home or Documents.
    Place { pinned: bool },
    /// A pinned folder, at this position among the pins.
    Bookmark { index: usize, count: usize },
    /// A drive.
    Device { mounted: bool },
    /// Trash, Applications: nothing to do but open.
    View,
}

/// The menu for a sidebar row.
pub fn sidebar_menu(kind: SidebarKind, c: &'static Copy) -> Vec<MenuLine> {
    use MenuCommand as M;
    let mut lines = vec![MenuLine::new(M::Open, c.open, Some("Enter"))];
    match kind {
        SidebarKind::Place { pinned } => {
            lines.push(MenuLine::new(M::OpenInNewTab, c.open_in_new_tab, None));
            if !pinned {
                lines.push(MenuLine::new(M::Pin, c.pin_to_sidebar, None));
            }
        }
        SidebarKind::Bookmark { index, count } => {
            lines.push(MenuLine::new(M::OpenInNewTab, c.open_in_new_tab, None));
            lines.push(MenuLine::Separator);
            lines.push(MenuLine::new(
                M::RenameBookmark,
                c.rename_bookmark,
                Some("F2"),
            ));
            lines.push(
                MenuLine::new(M::MoveBookmarkUp, c.move_up, Some("Alt+Up")).enabled(index > 0),
            );
            lines.push(
                MenuLine::new(M::MoveBookmarkDown, c.move_down, Some("Alt+Down"))
                    .enabled(index + 1 < count),
            );
            lines.push(MenuLine::new(M::MoveBookmarkToTop, c.move_to_top, None).enabled(index > 0));
            lines.push(
                MenuLine::new(M::MoveBookmarkToBottom, c.move_to_bottom, None)
                    .enabled(index + 1 < count),
            );
            lines.push(MenuLine::Separator);
            lines.push(MenuLine::new(M::RemoveBookmark, c.unpin, Some("Delete")));
        }
        SidebarKind::Device { mounted } => {
            lines.push(MenuLine::Separator);
            // A drive that is not mounted offers Mount, as folio's does.
            // Eject is always there: stopping use of a device is never
            // argued with.
            if !mounted {
                lines.push(MenuLine::new(M::Mount, c.device_mount, None));
            }
            lines.push(MenuLine::new(M::Eject, c.device_eject, None));
            lines.push(MenuLine::Separator);
            lines.push(MenuLine::new(M::Format, c.format_drive, None));
        }
        SidebarKind::View => {
            lines.push(MenuLine::new(M::OpenInNewTab, c.open_in_new_tab, None));
        }
    }
    lines
}

/// The sorts `S` and `Shift+S` step through, in folio's order (`SortMenu.qml`):
/// name, kind, newest, oldest, size (biggest first).
pub const SORT_STEPS: [(SortKey, SortDirection); 5] = [
    (SortKey::Name, SortDirection::Ascending),
    (SortKey::Type, SortDirection::Ascending),
    (SortKey::Modified, SortDirection::Descending),
    (SortKey::Modified, SortDirection::Ascending),
    (SortKey::Size, SortDirection::Descending),
];

/// The next or previous sort after `order`. A sort that is not one of the
/// five steps (extension, or name descending) steps from the first.
pub fn step_sort(order: SortOrder, forward: bool) -> SortOrder {
    let at = SORT_STEPS
        .iter()
        .position(|(key, direction)| *key == order.key && *direction == order.direction);
    let len = SORT_STEPS.len();
    let next = match (at, forward) {
        (Some(at), true) => (at + 1) % len,
        (Some(at), false) => (at + len - 1) % len,
        (None, true) => 0,
        (None, false) => len - 1,
    };
    let (key, direction) = SORT_STEPS[next];
    SortOrder::new(key, direction).with_folders_first(order.folders_first)
}

/// Whether a file is a disk image Write to Drive offers: `.iso` or `.img`.
pub fn is_disk_image(path: &std::path::Path) -> bool {
    path.extension()
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| extension == "iso" || extension == "img")
}
