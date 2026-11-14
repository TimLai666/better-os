//! The right-click menus, as folio's `FileMenu.qml` and the sidebar define
//! them, decided without a window.

use files_core::{SortDirection, SortKey, SortOrder};
use files_gui::commands::ArchiveActions;
use files_gui::context_menu::{
    MenuCommand as M, MenuLine, SORT_STEPS, SelectionFacts, SidebarKind, background_menu,
    selection_menu, sidebar_menu, step_sort,
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
    assert!(enabled(
        &sidebar_menu(SidebarKind::Device { mounted: true }, &EN_US),
        M::Eject
    ));
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
    let lines = sidebar_menu(SidebarKind::Device { mounted: false }, &EN_US);
    assert!(enabled(&lines, M::Mount));
    assert!(enabled(&lines, M::Eject));
    let mounted = sidebar_menu(SidebarKind::Device { mounted: true }, &EN_US);
    assert!(!commands(&mounted).contains(&M::Mount));
}
