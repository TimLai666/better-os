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
    /// Turns the drive's "Mount automatically" on or off (ticket 127).
    MountAutomatically,
    /// Mounts again a drive that is on and did not mount.
    RetryMount,
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
    /// A line that does nothing and says why something is missing.
    Note(&'static str),
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
            MenuLine::Note(_) | MenuLine::Separator => None,
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
    Device {
        mounted: bool,
        /// Whether Eject is offered.
        ejectable: bool,
        /// Whether Format is offered (a device the storage service reports).
        formattable: bool,
        auto_mount: AutoMountLine,
        /// It is on and did not mount: Retry.
        failed: bool,
        /// Why Eject and Format are not offered (ticket 171): the drive
        /// holds the running system, or that cannot be told.
        withheld: Option<Withheld>,
    },
    /// Trash, Applications: nothing to do but open.
    View,
}

/// Why a drive's menu offers neither Eject nor Format (ticket 171), from
/// [`crate::drives::system_use_of`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Withheld {
    RunningSystem,
    SystemFolder,
    Swap,
    Unknown,
}

impl Withheld {
    /// `None` for a drive that holds nothing of the running system.
    pub fn of(system: &storage_platform::SystemUse) -> Option<Self> {
        use storage_platform::SystemUse as U;
        match system {
            U::Free => None,
            U::RunningSystem => Some(Withheld::RunningSystem),
            U::SystemFolder { .. } => Some(Withheld::SystemFolder),
            U::Swap => Some(Withheld::Swap),
            U::Unknown => Some(Withheld::Unknown),
        }
    }

    /// The one line the menu says, which is also what a refusal says.
    pub fn line(self, c: &'static Copy) -> &'static str {
        match self {
            Withheld::RunningSystem => c.errors.system_running,
            Withheld::SystemFolder => c.errors.system_folder,
            Withheld::Swap => c.errors.system_swap,
            Withheld::Unknown => c.errors.system_unknown,
        }
    }
}

/// The "Mount automatically" line a drive's menu has.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoMountLine {
    /// No such choice (a phone, a share, a drive with no UUID).
    None,
    /// Checked or not, and whether it can be changed here (not the system
    /// drive, not a drive the system's own mount table names).
    Shown { on: bool, changeable: bool },
}

impl AutoMountLine {
    pub fn of(drive: &drive_mounts::listing::Drive) -> Self {
        match &drive.auto_mount {
            None => AutoMountLine::None,
            Some(auto) => AutoMountLine::Shown {
                on: auto.on,
                changeable: auto.locked.is_none(),
            },
        }
    }
}

/// What the menu of a drive row the storage service lists is, paired with
/// the drive listing's own drive when it has one. Eject and Format only for
/// a drive that holds nothing of the running system (ticket 171), never
/// from a constant.
pub fn device_row_kind(
    device: &crate::devices::DeviceRow,
    drive: Option<&drive_mounts::listing::Drive>,
) -> SidebarKind {
    let system = crate::drives::system_use_of(drive, Some(device));
    let free = system.permits_removal();
    SidebarKind::Device {
        mounted: device.mount_point.is_some(),
        ejectable: free,
        formattable: free,
        auto_mount: drive.map_or(AutoMountLine::None, AutoMountLine::of),
        failed: drive.is_some_and(drive_mounts::choice::failed),
        withheld: Withheld::of(&system),
    }
}

/// What the menu of a drive row only the drive listing has is: a drive the
/// storage service does not list (an internal partition, a share, a
/// phone) is never formatted from here.
pub fn drive_row_kind(drive: &drive_mounts::listing::Drive) -> SidebarKind {
    let system = crate::drives::system_use_of(Some(drive), None);
    SidebarKind::Device {
        mounted: drive.mount_point.is_some(),
        ejectable: drive.ejectable
            && drive.kind != drive_mounts::listing::DriveKind::System
            && system.permits_removal(),
        formattable: false,
        auto_mount: AutoMountLine::of(drive),
        failed: drive_mounts::choice::failed(drive),
        withheld: Withheld::of(&system),
    }
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
        SidebarKind::Device {
            mounted,
            ejectable,
            formattable,
            auto_mount,
            failed,
            withheld,
        } => {
            // Nothing the running system uses is ejected or formatted,
            // whatever the caller worked out (ticket 171).
            let (ejectable, formattable) = match withheld {
                Some(_) => (false, false),
                None => (ejectable, formattable),
            };
            lines.push(MenuLine::Separator);
            // A drive that is not mounted offers Mount, as folio's does, and
            // one that is on and did not mount offers Retry. Eject is always
            // there on what can be ejected: stopping use of a device is never
            // argued with.
            if failed {
                lines.push(MenuLine::new(M::RetryMount, c.retry_mount, None));
            } else if !mounted {
                lines.push(MenuLine::new(M::Mount, c.device_mount, None));
            }
            if ejectable {
                lines.push(MenuLine::new(M::Eject, c.device_eject, None));
            }
            if let AutoMountLine::Shown { on, changeable } = auto_mount {
                lines.push(MenuLine::Separator);
                lines.push(
                    MenuLine::new(M::MountAutomatically, c.mount_automatically, None)
                        .checked(on)
                        .enabled(changeable),
                );
            }
            if formattable {
                lines.push(MenuLine::Separator);
                lines.push(MenuLine::new(M::Format, c.format_drive, None));
            }
            if let Some(withheld) = withheld {
                lines.push(MenuLine::Separator);
                lines.push(MenuLine::Note(withheld.line(c)));
            }
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
