//! The right-click menus, as folio's `FileMenu.qml` and the sidebar define
//! them, decided without a window.

use files_core::{SortDirection, SortKey, SortOrder};
use files_gui::commands::ArchiveActions;
use files_gui::context_menu::{
    AutoMountLine, MenuCommand as M, MenuLine, SORT_STEPS, SelectionFacts, SidebarKind,
    background_menu, selection_menu, sidebar_menu, step_sort,
};
use files_gui::i18n::{EN_US, ZH_TW};

fn commands(lines: &[MenuLine]) -> Vec<M> {
    lines.iter().filter_map(MenuLine::command).collect()
}

fn enabled(lines: &[MenuLine], wanted: M) -> bool {
    lines.iter().any(|line| {
        matches!(line, MenuLine::Command { command, enabled: true, .. } if *command == wanted)
    })
}

fn file() -> SelectionFacts {
    SelectionFacts {
        count: 1,
        all_local: true,
        writable: true,
        archive: ArchiveActions {
            compress: true,
            extract: false,
        },
        ..SelectionFacts::default()
    }
}

#[test]
fn a_file_offers_open_with_and_never_pin() {
    let lines = selection_menu(&file(), &EN_US);
    let got = commands(&lines);
    for wanted in [
        M::Open,
        M::OpenWith,
        M::Preview,
        M::Cut,
        M::Copy,
        M::CopyPath,
        M::Rename,
        M::Duplicate,
        M::Compress,
        M::MoveToTrash,
    ] {
        assert!(got.contains(&wanted), "{wanted:?} is missing");
    }
    assert!(!got.contains(&M::Pin));
    assert!(!got.contains(&M::Extract));
}

#[test]
fn a_folder_opens_in_a_new_tab_and_pins_or_unpins() {
    let mut facts = file();
    facts.single_folder = true;
    let got = commands(&selection_menu(&facts, &EN_US));
    assert!(got.contains(&M::OpenInNewTab));
    assert!(got.contains(&M::Pin));
    facts.single_folder_pinned = true;
    facts.can_paste = true;
    let got = commands(&selection_menu(&facts, &EN_US));
    assert!(got.contains(&M::Unpin));
    assert!(got.contains(&M::Paste), "paste into the folder");
}

#[test]
fn several_entries_cannot_be_renamed_or_previewed_at_once() {
    let mut facts = file();
    facts.count = 3;
    let lines = selection_menu(&facts, &EN_US);
    assert!(!enabled(&lines, M::Rename));
    assert!(!enabled(&lines, M::Preview));
    assert!(enabled(&lines, M::MoveToTrash));
}

#[test]
fn an_archive_offers_extract() {
    let mut facts = file();
    facts.archive.extract = true;
    assert!(commands(&selection_menu(&facts, &EN_US)).contains(&M::Extract));
}

#[test]
fn a_read_only_folder_greys_out_what_writes() {
    let mut facts = file();
    facts.writable = false;
    let lines = selection_menu(&facts, &EN_US);
    assert!(!enabled(&lines, M::Rename));
    assert!(!enabled(&lines, M::MoveToTrash));
    assert!(enabled(&lines, M::Copy));
}

#[test]
fn the_trash_restores_or_deletes_for_good() {
    let facts = SelectionFacts {
        count: 2,
        in_trash: true,
        ..SelectionFacts::default()
    };
    assert_eq!(
        commands(&selection_menu(&facts, &EN_US)),
        vec![M::Restore, M::DeletePermanently]
    );
}

#[test]
fn empty_space_pastes_and_makes_new_things() {
    let facts = SelectionFacts {
        writable: true,
        can_paste: true,
        showing_hidden: true,
        ..SelectionFacts::default()
    };
    let lines = background_menu(&facts, &EN_US);
    assert!(enabled(&lines, M::Paste));
    assert!(enabled(&lines, M::NewFolder));
    assert!(lines.iter().any(|line| matches!(
        line,
        MenuLine::Command {
            command: M::ToggleHidden,
            checked: Some(true),
            ..
        }
    )));
    let nothing = background_menu(&SelectionFacts::default(), &EN_US);
    assert!(!enabled(&nothing, M::Paste));
}

#[test]
fn a_pinned_folder_moves_renames_and_unpins_from_the_sidebar() {
    let first = sidebar_menu(SidebarKind::Bookmark { index: 0, count: 2 }, &EN_US);
    assert!(!enabled(&first, M::MoveBookmarkUp));
    assert!(enabled(&first, M::MoveBookmarkDown));
    assert!(enabled(&first, M::RemoveBookmark));
    assert!(enabled(&first, M::RenameBookmark));
    let last = sidebar_menu(SidebarKind::Bookmark { index: 1, count: 2 }, &EN_US);
    assert!(!enabled(&last, M::MoveBookmarkDown));
}

#[test]
fn a_drive_ejects_and_a_place_pins() {
    assert!(enabled(&sidebar_menu(device(true), &EN_US), M::Eject));
    assert!(
        commands(&sidebar_menu(SidebarKind::Place { pinned: false }, &EN_US)).contains(&M::Pin)
    );
    assert!(
        !commands(&sidebar_menu(SidebarKind::Place { pinned: true }, &EN_US)).contains(&M::Pin)
    );
}

#[test]
fn every_menu_line_has_words_in_both_languages() {
    let facts = file();
    for copy in [&EN_US, &ZH_TW] {
        for line in selection_menu(&facts, copy)
            .into_iter()
            .chain(background_menu(&facts, copy))
            .chain(sidebar_menu(
                SidebarKind::Bookmark { index: 0, count: 1 },
                copy,
            ))
        {
            if let MenuLine::Command { label, .. } = line {
                assert!(!label.trim().is_empty());
            }
        }
    }
}

#[test]
fn s_steps_through_the_five_sorts_and_wraps() {
    let mut order = SortOrder::default();
    let mut seen = Vec::new();
    for _ in 0..SORT_STEPS.len() {
        order = step_sort(order, true);
        seen.push((order.key, order.direction));
    }
    assert_eq!(seen[0], (SortKey::Type, SortDirection::Ascending));
    assert_eq!(seen[4], (SortKey::Name, SortDirection::Ascending));
    let back = step_sort(SortOrder::default(), false);
    assert_eq!(
        (back.key, back.direction),
        (SortKey::Size, SortDirection::Descending)
    );
    // Folders-first is kept.
    let kept = step_sort(SortOrder::default().with_folders_first(false), true);
    assert!(!kept.folders_first);
}

#[test]
fn empty_space_opens_a_terminal_and_the_trash_empties_only_when_it_holds_something() {
    let here = background_menu(&SelectionFacts::default(), &EN_US);
    assert!(enabled(&here, M::OpenTerminal));
    let empty = SelectionFacts {
        in_trash: true,
        ..SelectionFacts::default()
    };
    assert!(!enabled(&background_menu(&empty, &EN_US), M::EmptyTrash));
    let full = SelectionFacts {
        in_trash: true,
        has_items: true,
        ..SelectionFacts::default()
    };
    assert!(enabled(&background_menu(&full, &EN_US), M::EmptyTrash));
}

#[test]
fn a_drive_that_is_not_mounted_offers_mount_and_still_ejects() {
    let lines = sidebar_menu(device(false), &EN_US);
    assert!(enabled(&lines, M::Mount));
    assert!(enabled(&lines, M::Eject));
    let mounted = sidebar_menu(device(true), &EN_US);
    assert!(!commands(&mounted).contains(&M::Mount));
}

fn device(mounted: bool) -> SidebarKind {
    SidebarKind::Device {
        mounted,
        ejectable: true,
        formattable: true,
        auto_mount: AutoMountLine::None,
        failed: false,
        withheld: None,
    }
}

fn checked(lines: &[MenuLine], wanted: M) -> Option<(bool, bool)> {
    lines.iter().find_map(|line| match line {
        MenuLine::Command {
            command,
            enabled,
            checked,
            ..
        } if *command == wanted => Some(((*checked)?, *enabled)),
        _ => None,
    })
}

/// Ticket 127: the drive's menu carries its "Mount automatically", locked
/// where the system decides, and Retry for one that did not mount.
#[test]
fn a_drive_s_menu_turns_mounting_automatically_on_and_off() {
    let with = |auto_mount: AutoMountLine, failed: bool| SidebarKind::Device {
        mounted: !failed,
        ejectable: false,
        formattable: false,
        auto_mount,
        failed,
        withheld: None,
    };
    let on = sidebar_menu(
        with(
            AutoMountLine::Shown {
                on: true,
                changeable: true,
            },
            false,
        ),
        &EN_US,
    );
    assert_eq!(checked(&on, M::MountAutomatically), Some((true, true)));
    assert!(!commands(&on).contains(&M::Eject));
    assert!(!commands(&on).contains(&M::Format));
    let system = sidebar_menu(
        with(
            AutoMountLine::Shown {
                on: true,
                changeable: false,
            },
            false,
        ),
        &ZH_TW,
    );
    assert_eq!(checked(&system, M::MountAutomatically), Some((true, false)));
    let failed = sidebar_menu(
        with(
            AutoMountLine::Shown {
                on: true,
                changeable: true,
            },
            true,
        ),
        &EN_US,
    );
    assert!(enabled(&failed, M::RetryMount));
    assert!(!commands(&failed).contains(&M::Mount));
    let none = sidebar_menu(with(AutoMountLine::None, false), &EN_US);
    assert!(!commands(&none).contains(&M::MountAutomatically));
}

// --- Ticket 171: nothing the running system uses is ejected or formatted ---

use files_gui::context_menu::{Withheld, device_row_kind, drive_row_kind};
use files_gui::devices::DeviceRow;
use files_gui::drives::{Drive, DriveKind, SystemUse};

/// A device the storage service lists, as the window holds it.
fn service_row(system_use: SystemUse) -> DeviceRow {
    DeviceRow {
        object_path: "/org/freedesktop/UDisks2/block_devices/sdb2".into(),
        label: "rootfs".into(),
        identity: "uuid:root".into(),
        identity_volatile: false,
        mount_point: Some("/".into()),
        policy: storage_core::RemovalPolicy::DirectRemoval,
        state: storage_core::DeviceStateKind::Unknown,
        blockers: Vec::new(),
        unsafe_removal: None,
        drive: Some("/org/freedesktop/UDisks2/drives/USB".into()),
        drive_name: Some("SanDisk Extreme".into()),
        size: 128_000_000_000,
        whole_disk: false,
        filesystem: Some("ext4".into()),
        erasing: false,
        system_use,
    }
}

/// The drive listing's drive for it.
fn listed(kind: DriveKind, system_use: SystemUse, ejectable: bool) -> Drive {
    Drive {
        key: "/org/freedesktop/UDisks2/block_devices/sdb2".into(),
        label: None,
        kind,
        mount_point: Some("/".into()),
        object_path: Some("/org/freedesktop/UDisks2/block_devices/sdb2".into()),
        device_path: Some("/dev/sdb2".into()),
        drive_object: Some("/org/freedesktop/UDisks2/drives/USB".into()),
        backing: None,
        locked: false,
        uuid: Some("ROOT".into()),
        size: 128_000_000_000,
        ejectable,
        system_use,
        capacity: None,
        auto_mount: None,
        form: None,
    }
}

fn notes(lines: &[MenuLine]) -> Vec<&'static str> {
    lines
        .iter()
        .filter_map(|line| match line {
            MenuLine::Note(text) => Some(*text),
            _ => None,
        })
        .collect()
}

#[test]
fn the_running_system_on_a_usb_drive_offers_neither_eject_nor_format_and_says_why() {
    // The storage service lists it (UDisks2 does not mark a USB drive as
    // the system's) and the sidebar pairs it with the system drive's row.
    let drive = listed(DriveKind::System, SystemUse::RunningSystem, false);
    for system_use in [SystemUse::RunningSystem, SystemUse::Free] {
        // Even a service that called it free does not outvote the listing.
        let kind = device_row_kind(&service_row(system_use), Some(&drive));
        for (copy, line) in [
            (
                &EN_US,
                "This drive holds the running system, so it can't be ejected or formatted.",
            ),
            (&ZH_TW, "這個磁碟上有正在執行的系統，所以不能退出或格式化。"),
        ] {
            let lines = sidebar_menu(kind, copy);
            assert!(!commands(&lines).contains(&M::Eject));
            assert!(!commands(&lines).contains(&M::Format));
            assert_eq!(notes(&lines), [line]);
        }
    }
    // The listing's own row for it says the same.
    let lines = sidebar_menu(drive_row_kind(&drive), &EN_US);
    assert!(!commands(&lines).contains(&M::Eject));
    assert_eq!(notes(&lines).len(), 1);
}

#[test]
fn a_second_partition_of_the_system_disk_is_the_system_s_too() {
    // A data partition on the USB drive the system runs from: an ordinary
    // removable drive to UDisks2, held because its drive holds `/`.
    let mut data = listed(DriveKind::Removable, SystemUse::RunningSystem, false);
    data.mount_point = Some("/media/me/DATA".into());
    let mut row = service_row(SystemUse::RunningSystem);
    row.mount_point = Some("/media/me/DATA".into());
    let lines = sidebar_menu(device_row_kind(&row, Some(&data)), &EN_US);
    assert!(!commands(&lines).contains(&M::Eject));
    assert!(!commands(&lines).contains(&M::Format));
    assert_eq!(
        Withheld::of(&SystemUse::SystemFolder {
            path: "/home".into()
        }),
        Some(Withheld::SystemFolder)
    );
}

#[test]
fn an_ordinary_usb_stick_offers_eject_and_format_with_no_note() {
    let mut stick = listed(DriveKind::Removable, SystemUse::Free, true);
    stick.mount_point = Some("/media/me/STICK".into());
    let mut row = service_row(SystemUse::Free);
    row.mount_point = Some("/media/me/STICK".into());
    let lines = sidebar_menu(device_row_kind(&row, Some(&stick)), &EN_US);
    assert!(enabled(&lines, M::Eject));
    assert!(enabled(&lines, M::Format));
    assert!(notes(&lines).is_empty());
    // A blank stick the listing does not have: the service's answer alone.
    let lines = sidebar_menu(device_row_kind(&row, None), &EN_US);
    assert!(enabled(&lines, M::Format));
    // Its own row in the listing, when the service is not there.
    let lines = sidebar_menu(drive_row_kind(&stick), &EN_US);
    assert!(enabled(&lines, M::Eject));
}

#[test]
fn a_drive_whose_system_use_is_unknown_is_shown_as_unknown_and_not_offered() {
    let mut stick = listed(DriveKind::Removable, SystemUse::Unknown, false);
    stick.mount_point = Some("/media/me/STICK".into());
    let mut row = service_row(SystemUse::Unknown);
    row.mount_point = Some("/media/me/STICK".into());
    for kind in [
        device_row_kind(&row, Some(&stick)),
        device_row_kind(&row, None),
        drive_row_kind(&stick),
    ] {
        for (copy, line) in [
            (
                &EN_US,
                "Better Files can't tell whether the running system uses this drive, so it can't be ejected or formatted.",
            ),
            (
                &ZH_TW,
                "無法確定系統是否正在使用這個磁碟，所以不能退出或格式化。",
            ),
        ] {
            let lines = sidebar_menu(kind, copy);
            assert!(!commands(&lines).contains(&M::Eject));
            assert!(!commands(&lines).contains(&M::Format));
            assert_eq!(notes(&lines), [line]);
        }
    }
    // A free listing and an unknown service: unknown wins.
    let free = listed(DriveKind::Removable, SystemUse::Free, true);
    let lines = sidebar_menu(device_row_kind(&row, Some(&free)), &EN_US);
    assert!(!commands(&lines).contains(&M::Eject));
}
