//! View-model tests. None of them opens a window.
//!
//! Every behaviour Better Files claims is asserted here against the same types
//! the renderer draws, which is the point of keeping the decisions out of the
//! GPUI layer: a display server is not needed to know whether Back is enabled,
//! whether a bookmark survived a restart, or whether a paused job offers a
//! Resume button.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use files_core::{
    DirectoryModel, Entry, EntryKind, HiddenPreference, History, LocalPath, Location,
    SortDirection, SortKey, SortOrder, TrashLocation,
};
use files_operations::{
    Conflict, ConflictKind, EngineConfig, JobEngine, JobSnapshot, JobState, OperationKind,
    Progress, RemainingTime, Resolution, ResolutionScope, Throughput,
    progress::{Confidence, ItemProgress},
};
use files_platform::{MountTable, ReaderConfig, UserDirectories};

use crate::bookmarks::{BookmarkFile, BookmarkStore, PinOutcome};
use crate::commands::{self, Clipboard, CommandRefusal};
use crate::content::{
    ContentView, NoHandlerReason, OpenOutcome, SelectionInput, header_click, route_open,
};
use crate::i18n::{EN_US, Locale, ZH_TW, copy};
use crate::keys::{Command, Focus, Modifiers, command_for};
use crate::layout::{
    ControlLayout, header_fits, label_width, sidebar_label_fits, toolbar_layout, visible_rows,
};
use crate::opcenter::{self, SessionHistory, choices_for, controls_for};
use crate::prefs::{FilesPreferences, ItemScale, LocalePreference, PreferenceStore, ViewMode};
use crate::reader::FilesReader;
use crate::session::{FilesSession, Notice, PendingDialog, SessionSetup};
use crate::sidebar::{
    Availability, FixedProbe, NoDeviceStates, SidebarInputs, SidebarSection, build_rows,
};
use crate::toolbar::{
    FixedValidator, PathRejection, breadcrumb, display_path, resolve_path_input, toolbar_state,
};

fn at(path: &str) -> Location {
    Location::local(path).unwrap()
}

fn local(path: &str) -> LocalPath {
    LocalPath::new(path).unwrap()
}

// --- Toolbar ------------------------------------------------------------

#[test]
fn the_toolbar_reads_its_buttons_off_the_history() {
    let mut history = History::new(at("/home/tim"));
    let state = toolbar_state(&history);
    assert!(!state.can_go_back);
    assert!(!state.can_go_forward);
    assert!(state.can_go_to_parent);
    assert_eq!(state.path_text, "/home/tim");

    history.visit(at("/home/tim/notes"));
    let state = toolbar_state(&history);
    assert!(state.can_go_back);
    assert!(!state.can_go_forward);

    history.back();
    let state = toolbar_state(&history);
    assert!(!state.can_go_back);
    assert!(state.can_go_forward);
}

#[test]
fn the_root_has_no_parent_so_up_is_disabled_there() {
    let history = History::new(at("/"));
    assert!(!toolbar_state(&history).can_go_to_parent);
}

#[test]
fn a_location_with_no_path_shows_its_uri_rather_than_an_invented_path() {
    assert_eq!(
        display_path(&Location::Trash(TrashLocation::Root)),
        "trash:///"
    );
    assert_eq!(display_path(&Location::Applications), "applications:///");
    assert_eq!(display_path(&at("/etc")), "/etc");
}

#[test]
fn the_path_field_accepts_a_path_a_tilde_and_a_uri() {
    let validator = FixedValidator::new(
        [PathBuf::from("/srv/data"), PathBuf::from("/home/tim")],
        [PathBuf::from("/srv/data/notes.txt")],
    );
    let home = PathBuf::from("/home/tim");

    assert_eq!(
        resolve_path_input("/srv/data", Some(&home), &validator),
        Ok(at("/srv/data"))
    );
    assert_eq!(
        resolve_path_input("  /srv/data  ", Some(&home), &validator),
        Ok(at("/srv/data"))
    );
    assert_eq!(
        resolve_path_input("~", Some(&home), &validator),
        Ok(at("/home/tim"))
    );
    assert_eq!(
        resolve_path_input("file:///srv/data", Some(&home), &validator),
        Ok(at("/srv/data"))
    );
    // A typed location that has no filesystem path is still a location.
    assert_eq!(
        resolve_path_input("trash:///", Some(&home), &validator),
        Ok(Location::Trash(TrashLocation::Root))
    );
}

#[test]
fn the_path_field_states_why_it_refused_rather_than_doing_nothing() {
    let validator = FixedValidator::new(
        [PathBuf::from("/srv/data")],
        [PathBuf::from("/srv/data/notes.txt")],
    );
    let home = PathBuf::from("/home/tim");
    for (input, expected) in [
        ("", PathRejection::Empty),
        ("   ", PathRejection::Empty),
        ("data", PathRejection::NotAbsolute),
        ("/srv/missing", PathRejection::NotFound),
        ("/srv/data/notes.txt", PathRejection::NotADirectory),
        ("smb://server/share", PathRejection::Unsupported),
        ("wat://nowhere", PathRejection::Unsupported),
    ] {
        assert_eq!(
            resolve_path_input(input, Some(&home), &validator),
            Err(expected),
            "input {input:?}"
        );
    }
}

#[test]
fn the_breadcrumb_is_built_from_the_path_not_from_the_history() {
    let crumbs = breadcrumb(&at("/home/tim/notes"));
    let names: Vec<&str> = crumbs.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["/", "home", "tim", "notes"]);
    assert_eq!(crumbs.last().unwrap().1, at("/home/tim/notes"));
}

// --- Keyboard -----------------------------------------------------------

#[test]
fn every_documented_shortcut_maps_to_its_command() {
    let content = Focus::Content;
    for (key, modifiers, expected) in [
        ("h", Modifiers::control(), Command::ToggleHidden),
        ("t", Modifiers::control(), Command::NewTab),
        ("w", Modifiers::control(), Command::CloseTab),
        ("t", Modifiers::control_shift(), Command::RestoreClosedTab),
        ("c", Modifiers::control(), Command::Copy),
        ("x", Modifiers::control(), Command::Cut),
        ("v", Modifiers::control(), Command::Paste),
        ("d", Modifiers::control(), Command::Duplicate),
        ("a", Modifiers::control(), Command::SelectAll),
        ("l", Modifiers::control(), Command::FocusPathField),
        ("n", Modifiers::control_shift(), Command::NewFolder),
        ("f2", Modifiers::NONE, Command::Rename),
        ("delete", Modifiers::NONE, Command::MoveToTrash),
        ("delete", Modifiers::shift(), Command::DeletePermanently),
        ("enter", Modifiers::NONE, Command::Open),
        ("left", Modifiers::alt(), Command::GoBack),
        ("right", Modifiers::alt(), Command::GoForward),
        ("up", Modifiers::alt(), Command::GoToParent),
        ("backspace", Modifiers::NONE, Command::GoToParent),
        ("up", Modifiers::NONE, Command::MoveUp),
        ("down", Modifiers::NONE, Command::MoveDown),
        ("pagedown", Modifiers::NONE, Command::PageDown),
        ("home", Modifiers::NONE, Command::MoveToStart),
        ("end", Modifiers::NONE, Command::MoveToEnd),
        ("escape", Modifiers::NONE, Command::ClearSelection),
        ("up", Modifiers::shift(), Command::ExtendUp),
        ("o", Modifiers::control(), Command::ToggleOperations),
    ] {
        assert_eq!(
            command_for(key, modifiers, None, content),
            Some(expected),
            "{key} with {modifiers:?}"
        );
    }
}

#[test]
fn the_sidebar_gives_alt_arrows_and_delete_to_the_bookmark_it_is_on() {
    assert_eq!(
        command_for("up", Modifiers::alt(), None, Focus::Sidebar),
        Some(Command::MoveBookmarkUp)
    );
    assert_eq!(
        command_for("down", Modifiers::alt(), None, Focus::Sidebar),
        Some(Command::MoveBookmarkDown)
    );
    assert_eq!(
        command_for("delete", Modifiers::NONE, None, Focus::Sidebar),
        Some(Command::RemoveBookmark)
    );
    // The same keys still navigate in the content area.
    assert_eq!(
        command_for("up", Modifiers::alt(), None, Focus::Content),
        Some(Command::GoToParent)
    );
}

#[test]
fn a_printable_key_becomes_type_ahead_and_a_named_key_does_not() {
    assert_eq!(
        command_for("r", Modifiers::NONE, Some("r"), Focus::Content),
        Some(Command::TypeAhead('r'))
    );
    assert_eq!(
        command_for("7", Modifiers::NONE, Some("7"), Focus::Content),
        Some(Command::TypeAhead('7'))
    );
    // A modifier claims the key first: Ctrl+C is Copy, never a `c`.
    assert_eq!(
        command_for("c", Modifiers::control(), Some("c"), Focus::Content),
        Some(Command::Copy)
    );
    assert_eq!(
        command_for("f7", Modifiers::NONE, None, Focus::Content),
        None
    );
}

// --- Content: sorting, selection, streaming ------------------------------

fn model_with(names: &[&str]) -> DirectoryModel {
    let mut model =
        DirectoryModel::new(at("/d"), SortOrder::default(), HiddenPreference::default());
    model.insert_batch(
        names
            .iter()
            .map(|name| {
                Entry::file(
                    *name,
                    local(&format!("/d/{name}")),
                    if name.ends_with('/') {
                        EntryKind::Directory
                    } else {
                        EntryKind::File
                    },
                )
            })
            .collect(),
    );
    model
}

#[test]
fn clicking_a_header_flips_the_direction_and_switching_columns_starts_ascending() {
    let order = SortOrder::default();
    let by_size = header_click(order, crate::content::ListColumn::Size);
    assert_eq!(by_size.key, SortKey::Size);
    assert_eq!(by_size.direction, SortDirection::Ascending);
    assert_eq!(by_size.folders_first, order.folders_first);

    let flipped = header_click(by_size, crate::content::ListColumn::Size);
    assert_eq!(flipped.direction, SortDirection::Descending);
    assert_eq!(flipped.key, SortKey::Size);
}

#[test]
fn arrows_move_one_row_in_the_list_and_one_grid_row_in_the_grid() {
    let mut model = model_with(&["a", "b", "c", "d", "e", "f"]);
    let mut view = ContentView::new(ViewMode::List, ItemScale::Medium);

    assert_eq!(view.apply(&mut model, SelectionInput::Click(0), 1), Some(0));
    assert_eq!(view.apply(&mut model, SelectionInput::Down, 1), Some(1));
    assert_eq!(view.apply(&mut model, SelectionInput::Down, 1), Some(2));
    // The grid's Down is a whole row of tiles.
    assert_eq!(view.apply(&mut model, SelectionInput::Down, 3), Some(5));
    // Clamped rather than wrapped: the last row stays the last row.
    assert_eq!(view.apply(&mut model, SelectionInput::Down, 3), Some(5));
    assert_eq!(view.apply(&mut model, SelectionInput::Home, 3), Some(0));
    assert_eq!(view.apply(&mut model, SelectionInput::End, 3), Some(5));
}

#[test]
fn control_click_adds_and_shift_click_takes_the_range() {
    let mut model = model_with(&["a", "b", "c", "d"]);
    let mut view = ContentView::new(ViewMode::List, ItemScale::Medium);

    view.apply(&mut model, SelectionInput::Click(0), 1);
    assert_eq!(model.selection().len(), 1);
    view.apply(&mut model, SelectionInput::ToggleClick(2), 1);
    assert_eq!(model.selection().len(), 2);
    view.apply(&mut model, SelectionInput::ToggleClick(2), 1);
    assert_eq!(model.selection().len(), 1);

    view.apply(&mut model, SelectionInput::Click(1), 1);
    view.apply(&mut model, SelectionInput::RangeClick(3), 1);
    assert_eq!(model.selection().len(), 3);

    view.apply(&mut model, SelectionInput::SelectAll, 1);
    assert_eq!(model.selection().len(), 4);
    view.apply(&mut model, SelectionInput::Clear, 1);
    assert!(model.selection().is_empty());
}

#[test]
fn entries_arriving_above_the_cursor_do_not_move_the_selection() {
    let mut model = model_with(&["m", "n", "o"]);
    let mut view = ContentView::new(ViewMode::List, ItemScale::Medium);
    view.apply(&mut model, SelectionInput::Click(1), 1);
    let selected = model.selection().cursor().cloned().unwrap();
    assert_eq!(view.cursor(), Some(1));

    // Twenty entries that all sort before the cursor arrive.
    model.insert_batch(
        (0..20)
            .map(|index| {
                let name = format!("a{index:02}");
                Entry::file(name.clone(), local(&format!("/d/{name}")), EntryKind::File)
            })
            .collect(),
    );
    view.resync(&model);

    assert_eq!(
        model.selection().cursor(),
        Some(&selected),
        "the selection names an entry, and the entry did not change"
    );
    assert_eq!(
        view.cursor(),
        Some(21),
        "the cursor index followed the entry rather than staying put"
    );
    assert_eq!(model.visible(21).map(Entry::id), Some(selected));
}

#[test]
fn a_removed_focused_entry_drops_the_cursor_rather_than_focusing_its_neighbour() {
    let mut model = model_with(&["a", "b", "c"]);
    let mut view = ContentView::new(ViewMode::List, ItemScale::Medium);
    view.apply(&mut model, SelectionInput::Click(1), 1);
    let id = model.selection().cursor().cloned().unwrap();
    model.remove(&id);
    view.resync(&model);
    assert_eq!(view.cursor(), None);
}

#[test]
fn type_ahead_accumulates_and_then_steps_through_the_matches() {
    let mut model = model_with(&["report-a", "report-b", "sales", "zebra"]);
    let mut view = ContentView::new(ViewMode::List, ItemScale::Medium);
    let now = Instant::now();

    assert_eq!(view.type_ahead_key(&mut model, 'r', now), Some(0));
    // A second `r` inside the window extends the buffer to `rr`, which matches
    // nothing, so nothing moves.
    assert_eq!(view.type_ahead_key(&mut model, 'e', now), Some(0));
    assert_eq!(view.type_ahead(), "re");

    // After the timeout the buffer resets and a single `r` steps to the next
    // match instead of sticking on the first.
    let later = now + crate::content::TYPE_AHEAD_TIMEOUT + Duration::from_millis(1);
    assert_eq!(view.type_ahead_key(&mut model, 'r', later), Some(1));
    assert_eq!(view.type_ahead(), "r");

    let later2 = later + crate::content::TYPE_AHEAD_TIMEOUT + Duration::from_millis(1);
    assert_eq!(view.type_ahead_key(&mut model, 's', later2), Some(2));
}

#[test]
fn the_grid_fits_as_many_tiles_as_the_width_allows_and_never_fewer_than_one() {
    let view = ContentView::new(ViewMode::Grid, ItemScale::Medium);
    assert_eq!(view.columns(1_000.0), 7);
    assert_eq!(view.columns(10.0), 1);
    let list = ContentView::new(ViewMode::List, ItemScale::Medium);
    assert_eq!(list.columns(1_000.0), 1);
}

#[test]
fn a_page_is_the_number_of_rows_the_viewport_shows() {
    assert_eq!(visible_rows(340.0, 34.0), 10);
    assert_eq!(visible_rows(0.0, 34.0), 1);
    assert_eq!(visible_rows(340.0, 0.0), 1);
}

// --- Open routing --------------------------------------------------------

#[test]
fn opening_a_directory_navigates_and_opening_a_file_reports_no_handler() {
    let directory = Entry::file("notes", local("/d/notes"), EntryKind::Directory);
    assert_eq!(
        route_open(&directory),
        OpenOutcome::Navigate(Box::new(at("/d/notes")))
    );

    let file = Entry::file("notes.txt", local("/d/notes.txt"), EntryKind::File);
    assert_eq!(
        route_open(&file),
        OpenOutcome::NoHandler(NoHandlerReason::File {
            name: "notes.txt".to_string()
        })
    );
    // The message names the file and says what is missing, in both languages.
    let OpenOutcome::NoHandler(reason) = route_open(&file) else {
        panic!("expected a no-handler outcome");
    };
    assert!(reason.message(&EN_US).contains("notes.txt"));
    assert!(reason.message(&ZH_TW).contains("notes.txt"));
}

#[test]
fn a_special_file_is_refused_with_a_reason() {
    let socket = Entry::file("sock", local("/d/sock"), EntryKind::Special);
    assert_eq!(
        route_open(&socket),
        OpenOutcome::Refused(files_core::OpenRefusal::NotOpenable)
    );
}

// --- Bookmarks -----------------------------------------------------------

const FOREIGN_FILE: &str = "\
file:///home/tim/Documents\n\
sftp://server.example/srv Remote share\n\
# a comment somebody's tooling wrote\n\
\n\
file:///home/tim/Pictures Photos\n\
recent:///\n";

#[test]
fn a_bookmark_file_round_trips_byte_for_byte_when_nothing_changed() {
    let file = BookmarkFile::parse(FOREIGN_FILE);
    assert_eq!(file.render(), FOREIGN_FILE);
    assert_eq!(file.len(), 2);
    assert_eq!(
        file.foreign_line_count(),
        4,
        "the sftp line, the comment, the blank line, and the recent line"
    );
}

#[test]
fn pinning_appends_and_never_touches_a_foreign_line() {
    let mut file = BookmarkFile::parse(FOREIGN_FILE);
    assert_eq!(file.pin(&at("/srv/data")), PinOutcome::Pinned);
    assert_eq!(file.pin(&at("/srv/data")), PinOutcome::AlreadyPinned);
    assert_eq!(
        file.pin(&Location::Applications),
        PinOutcome::NotPinnable,
        "a location with no filesystem path cannot be a folder bookmark"
    );

    let rendered = file.render();
    assert!(rendered.contains("sftp://server.example/srv Remote share"));
    assert!(rendered.contains("# a comment somebody's tooling wrote"));
    assert!(rendered.ends_with("file:///srv/data\n"));
    assert_eq!(file.len(), 3);
}

#[test]
fn reordering_moves_bookmarks_and_leaves_every_foreign_line_where_it_was() {
    let mut file = BookmarkFile::parse(FOREIGN_FILE);
    let before: Vec<String> = FOREIGN_FILE.lines().map(str::to_string).collect();

    assert!(file.move_down(0));
    let after: Vec<String> = file.render().lines().map(str::to_string).collect();

    // The two bookmarks swapped; lines 1, 2, 3 and 5 are untouched.
    assert_eq!(after[0], before[4]);
    assert_eq!(after[4], before[0]);
    for index in [1usize, 2, 3, 5] {
        assert_eq!(after[index], before[index], "foreign line {index} moved");
    }
    assert!(file.move_up(1));
    assert_eq!(file.render(), FOREIGN_FILE);
}

#[test]
fn moving_a_bookmark_to_an_arbitrary_slot_lands_where_it_was_dropped() {
    let mut file = BookmarkFile::parse("file:///a\nfile:///b\nfile:///c\n");
    assert!(file.move_to(0, 2));
    assert_eq!(file.render(), "file:///b\nfile:///c\nfile:///a\n");
    assert!(file.move_to(2, 0));
    assert_eq!(file.render(), "file:///a\nfile:///b\nfile:///c\n");
    assert!(!file.move_to(1, 1));
    assert!(!file.move_to(0, 9));
}

#[test]
fn a_label_renames_the_bookmark_and_not_the_directory() {
    let mut file = BookmarkFile::parse("file:///home/tim/Documents\n");
    assert_eq!(file.get(0).unwrap().display_name(), "Documents");
    assert!(file.set_label(0, "Work"));
    assert_eq!(file.get(0).unwrap().display_name(), "Work");
    assert_eq!(file.render(), "file:///home/tim/Documents Work\n");
    assert_eq!(
        file.get(0).unwrap().path().unwrap(),
        std::path::Path::new("/home/tim/Documents"),
        "the directory the bookmark points at is unchanged"
    );
    // Clearing the label falls back to the folder name rather than a blank row.
    assert!(file.set_label(0, "   "));
    assert_eq!(file.get(0).unwrap().display_name(), "Documents");
    assert_eq!(file.render(), "file:///home/tim/Documents\n");
}

#[test]
fn a_name_that_needs_escaping_survives_a_round_trip() {
    let mut file = BookmarkFile::default();
    file.pin(&at("/home/tim/My Photos & Videos"));
    let rendered = file.render();
    assert_eq!(rendered, "file:///home/tim/My%20Photos%20%26%20Videos");
    let reread = BookmarkFile::parse(&rendered);
    assert_eq!(
        reread.get(0).unwrap().path().unwrap(),
        std::path::Path::new("/home/tim/My Photos & Videos")
    );
}

#[test]
fn bookmarks_survive_a_restart_with_their_order_labels_and_foreign_lines() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("gtk-3.0/bookmarks");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, FOREIGN_FILE).unwrap();

    let store = BookmarkStore::at_path(&path);
    let mut file = store.load();
    file.pin(&at("/srv/data"));
    file.set_label(2, "Server");
    file.move_up(2);
    store.save(&file).unwrap();

    // A second process reads it back.
    let reopened = BookmarkStore::at_path(&path).load();
    let names: Vec<String> = reopened
        .bookmarks()
        .iter()
        .map(|bookmark| bookmark.display_name())
        .collect();
    assert_eq!(names, ["Documents", "Server", "Photos"]);
    assert_eq!(reopened.foreign_line_count(), 4);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("sftp://server.example/srv Remote share")
    );
}

#[test]
fn a_bookmark_whose_folder_is_gone_stays_visible_as_unavailable() {
    let mut bookmarks = BookmarkFile::default();
    bookmarks.pin(&at("/srv/present"));
    bookmarks.pin(&at("/srv/missing"));

    let directories = UserDirectories::default();
    let mounts = MountTable::new(Vec::new());
    let probe = FixedProbe::with([PathBuf::from("/srv/present")]);
    let inputs = SidebarInputs {
        directories: &directories,
        mounts: &mounts,
        bookmarks: &bookmarks,
        states: &NoDeviceStates,
        probe: &probe,
    };
    let rows = build_rows(&inputs, &EN_US);
    let favorites: Vec<_> = rows
        .iter()
        .filter(|row| row.section == SidebarSection::Favorites)
        .collect();

    assert_eq!(favorites.len(), 2, "the missing one is still listed");
    assert_eq!(favorites[0].availability, Availability::Available);
    assert_eq!(favorites[1].availability, Availability::Unavailable);
    assert_eq!(bookmarks.len(), 2, "nothing was deleted");
}

// --- Sidebar -------------------------------------------------------------

#[test]
fn the_sidebar_keeps_its_four_sections_distinct() {
    let home = tempfile::tempdir().unwrap();
    let directories = UserDirectories::from_values(Some(home.path()), None);
    let mut bookmarks = BookmarkFile::default();
    bookmarks.pin(&at("/srv/data"));
    let mounts = MountTable::new(Vec::new());
    let inputs = SidebarInputs {
        directories: &directories,
        mounts: &mounts,
        bookmarks: &bookmarks,
        states: &NoDeviceStates,
        probe: &FixedProbe::default(),
    };
    let rows = build_rows(&inputs, &EN_US);

    for section in SidebarSection::ALL {
        let count = rows.iter().filter(|row| row.section == section).count();
        if section == SidebarSection::Devices {
            assert_eq!(count, 0, "no external mounts in this fixture");
        } else {
            assert!(count > 0, "{section:?} has no rows");
        }
    }
    // Applications is one row, and it is not a directory.
    let applications: Vec<_> = rows
        .iter()
        .filter(|row| row.section == SidebarSection::Applications)
        .collect();
    assert_eq!(applications.len(), 1);
    assert_eq!(applications[0].location, Location::Applications);
    assert!(applications[0].location.as_local_path().is_none());

    // Trash is a place, and it is always reachable.
    let trash = rows
        .iter()
        .find(|row| row.location == Location::Trash(TrashLocation::Root))
        .expect("the Trash row");
    assert_eq!(trash.section, SidebarSection::Places);
    assert_eq!(trash.availability, Availability::Available);
}

#[test]
fn a_pinned_home_and_the_built_in_home_are_two_rows_in_two_sections() {
    let home = tempfile::tempdir().unwrap();
    let directories = UserDirectories::from_values(Some(home.path()), None);
    let home_location = directories.home().cloned().unwrap();
    let mut bookmarks = BookmarkFile::default();
    assert_eq!(bookmarks.pin(&home_location), PinOutcome::Pinned);

    let mounts = MountTable::new(Vec::new());
    let inputs = SidebarInputs {
        directories: &directories,
        mounts: &mounts,
        bookmarks: &bookmarks,
        states: &NoDeviceStates,
        probe: &FixedProbe::with([home.path().to_path_buf()]),
    };
    let rows = build_rows(&inputs, &EN_US);
    let matching: Vec<_> = rows
        .iter()
        .filter(|row| row.location == home_location)
        .collect();
    assert_eq!(matching.len(), 2);
    assert_eq!(matching[0].section, SidebarSection::Places);
    assert_eq!(matching[1].section, SidebarSection::Favorites);
    assert!(matching[0].bookmark_index.is_none());
    assert_eq!(matching[1].bookmark_index, Some(0));
}

#[test]
fn a_device_with_no_storage_service_reports_unknown_rather_than_ready() {
    let mounts = MountTable::new(vec![files_platform::MountPoint {
        mount_point: PathBuf::from("/media/tim/PHOTOS"),
        source: "/dev/sdb1".to_string(),
        filesystem: "vfat".to_string(),
        read_only: false,
        identity: storage_core::DeviceIdentity::from_evidence(storage_core::IdentityEvidence {
            // Nothing but the kernel name, which is what a mount table can
            // honestly report without the storage service.
            device_path: "/dev/sdb1".to_string(),
            ..storage_core::IdentityEvidence::default()
        }),
    }]);
    let directories = UserDirectories::default();
    let bookmarks = BookmarkFile::default();
    let inputs = SidebarInputs {
        directories: &directories,
        mounts: &mounts,
        bookmarks: &bookmarks,
        states: &NoDeviceStates,
        probe: &FixedProbe::with([PathBuf::from("/media/tim/PHOTOS")]),
    };
    let rows = build_rows(&inputs, &EN_US);
    let device = rows
        .iter()
        .find(|row| row.section == SidebarSection::Devices)
        .expect("the device row");

    assert_eq!(device.label, "PHOTOS", "the mount point, not the /dev node");
    assert_eq!(
        device.device_state,
        Some(storage_core::DeviceStateKind::Unknown)
    );
    assert!(
        !device.device_state.unwrap().permits_direct_removal(),
        "unknown must never read as safe to unplug"
    );
    assert!(
        device.identity_volatile,
        "a kernel name alone is a volatile identity, and the row says so"
    );
}

// --- Operation center ----------------------------------------------------

/// A real `JobId`, which only the engine can mint.
///
/// The synthetic snapshots below exist to exercise the mapping from a job's
/// state to the controls a row offers, for states an in-process engine cannot
/// be parked in on demand. They still carry an id the engine actually issued,
/// so nothing here invents a type the rest of the workspace guards.
fn sample_id() -> files_operations::JobId {
    use std::sync::OnceLock;
    static SAMPLE: OnceLock<(tempfile::TempDir, Arc<JobEngine>, files_operations::JobId)> =
        OnceLock::new();
    SAMPLE
        .get_or_init(|| {
            let root = tempfile::tempdir().unwrap();
            let engine = Arc::new(JobEngine::new(EngineConfig {
                store: None,
                ..EngineConfig::default()
            }));
            let handle = engine
                .submit(files_operations::JobSpec::new(
                    files_operations::Operation::CreateFolder {
                        parent: LocalPath::new(root.path()).unwrap(),
                        name: "sample".into(),
                    },
                ))
                .expect("the engine accepted a folder creation");
            let id = handle.id();
            engine.wait(id, Duration::from_secs(10));
            (root, engine, id)
        })
        .2
}

fn snapshot(kind: OperationKind, state: JobState) -> JobSnapshot {
    JobSnapshot {
        id: sample_id(),
        kind,
        state,
        progress: Progress {
            items_total: 10,
            items_done: 4,
            items_failed: 0,
            items_skipped: 0,
            bytes_total: 1_000,
            bytes_done: 250,
        },
        item: ItemProgress {
            bytes_total: 100,
            bytes_done: 50,
        },
        current: Some(PathBuf::from("/srv/data/report.txt")),
        throughput: Throughput {
            bytes_per_second: Some(5_000_000.0),
            items_per_second: Some(2.0),
        },
        remaining: RemainingTime {
            estimate: Some(Duration::from_secs(90)),
            confidence: Confidence::Medium,
        },
        conflict: None,
        failures: Vec::new(),
        checksums: Vec::new(),
        log: files_operations::OperationLog::default(),
    }
}

#[test]
fn the_controls_a_job_offers_are_the_ones_the_engine_would_accept() {
    let running = snapshot(OperationKind::Copy, JobState::Running);
    let controls = controls_for(&running);
    assert!(controls.pause && controls.cancel);
    assert!(!controls.resume && !controls.retry);

    let paused = snapshot(OperationKind::Copy, JobState::Paused);
    let controls = controls_for(&paused);
    assert!(controls.resume && controls.cancel);
    assert!(!controls.pause);

    // A rename is not pausable, so no Pause button is drawn for one.
    let rename = snapshot(OperationKind::Rename, JobState::Running);
    assert!(!controls_for(&rename).pause);
    assert!(!OperationKind::Rename.supports_pause());

    let done = snapshot(OperationKind::Copy, JobState::Completed);
    let controls = controls_for(&done);
    assert!(!controls.pause && !controls.resume && !controls.cancel && !controls.retry);
}

#[test]
fn retry_is_offered_only_for_a_failed_job_that_has_failed_items() {
    let mut failed = snapshot(OperationKind::Copy, JobState::Failed);
    assert!(!controls_for(&failed).retry, "nothing to retry");
    failed.failures.push((
        PathBuf::from("/srv/data/report.txt"),
        files_operations::OperationError::Cancelled {
            path: PathBuf::from("/srv/data/report.txt"),
        },
    ));
    assert!(controls_for(&failed).retry);
}

#[test]
fn a_conflict_offers_overwrite_only_where_overwriting_would_settle_it() {
    let exists = Conflict::exists(
        Some(PathBuf::from("/a/report.txt")),
        PathBuf::from("/b/report.txt"),
    );
    assert_eq!(
        choices_for(&exists),
        vec![
            Resolution::Skip,
            Resolution::Overwrite,
            Resolution::Rename,
            Resolution::Cancel
        ]
    );

    let full = Conflict {
        kind: ConflictKind::NoSpace,
        source: None,
        destination: PathBuf::from("/b/report.txt"),
        existing: None,
    };
    assert_eq!(
        choices_for(&full),
        vec![Resolution::Skip, Resolution::Cancel],
        "a full disk is not made emptier by overwriting"
    );
}

#[test]
fn a_waiting_job_surfaces_its_prompt_and_the_answer_carries_its_scope() {
    let mut waiting = snapshot(OperationKind::Copy, JobState::WaitingOnConflict);
    waiting.conflict = Some(Conflict::exists(
        Some(PathBuf::from("/a/report.txt")),
        PathBuf::from("/b/report.txt"),
    ));
    let prompt = opcenter::first_conflict(std::slice::from_ref(&waiting), &EN_US)
        .expect("a prompt for the waiting job");
    assert_eq!(prompt.destination, PathBuf::from("/b/report.txt"));

    assert_eq!(
        prompt.decision(Resolution::Overwrite, false).scope,
        ResolutionScope::ThisItem
    );
    assert_eq!(
        prompt.decision(Resolution::Overwrite, true).scope,
        ResolutionScope::ApplyToRemaining
    );
}

#[test]
fn a_row_never_shows_an_estimate_without_saying_how_much_it_is_worth() {
    let row = opcenter::job_row(&snapshot(OperationKind::Copy, JobState::Running), &EN_US);
    assert!(row.remaining_label.contains("1m 30s"));
    assert!(row.remaining_label.contains(EN_US.confidence_medium));

    let mut unknown = snapshot(OperationKind::Copy, JobState::Queued);
    unknown.remaining = RemainingTime {
        estimate: None,
        confidence: Confidence::None,
    };
    let row = opcenter::job_row(&unknown, &EN_US);
    assert_eq!(row.remaining_label, EN_US.confidence_none);
}

#[test]
fn a_job_with_no_measurable_total_draws_no_fraction_rather_than_zero() {
    let mut unknown = snapshot(OperationKind::Trash, JobState::Running);
    unknown.progress = Progress::default();
    let row = opcenter::job_row(&unknown, &EN_US);
    assert_eq!(row.fraction, None);
    assert_eq!(row.bytes_label, "—");
}

#[test]
fn the_session_history_keeps_one_row_per_finished_job() {
    let mut history = SessionHistory::default();
    history.record(opcenter::job_row(
        &snapshot(OperationKind::Copy, JobState::Running),
        &EN_US,
    ));
    assert!(history.is_empty(), "a running job is not history yet");

    history.record(opcenter::job_row(
        &snapshot(OperationKind::Copy, JobState::Completed),
        &EN_US,
    ));
    history.record(opcenter::job_row(
        &snapshot(OperationKind::Copy, JobState::Completed),
        &EN_US,
    ));
    assert_eq!(history.len(), 1, "the same job id is one row");
}

#[test]
fn the_toolbar_badge_counts_only_the_jobs_that_have_not_finished() {
    let jobs = vec![
        snapshot(OperationKind::Copy, JobState::Running),
        snapshot(OperationKind::Move, JobState::Completed),
        snapshot(OperationKind::Trash, JobState::WaitingOnConflict),
    ];
    assert_eq!(opcenter::active_count(&jobs), 2);
}

// --- Commands ------------------------------------------------------------

#[test]
fn a_paste_is_a_copy_job_or_a_move_job_depending_on_the_clipboard() {
    let destination = at("/srv/dest");
    let sources = vec![local("/srv/a.txt")];

    let copy = commands::paste(&Clipboard::Copy(sources.clone()), &destination).unwrap();
    assert_eq!(copy.kind(), OperationKind::Copy);

    let cut = commands::paste(&Clipboard::Cut(sources), &destination).unwrap();
    assert_eq!(cut.kind(), OperationKind::Move);

    assert_eq!(
        commands::paste(&Clipboard::Empty, &destination),
        Err(CommandRefusal::NothingToActOn)
    );
    assert_eq!(
        commands::paste(
            &Clipboard::Copy(vec![local("/srv/a.txt")]),
            &Location::Applications
        ),
        Err(CommandRefusal::NotAFilesystemLocation)
    );
}

#[test]
fn a_name_that_cannot_be_used_is_refused_before_a_job_is_built() {
    let location = at("/srv/dest");
    for name in ["", "   ", ".", "..", "a/b"] {
        assert_eq!(
            commands::new_folder(&location, name).err(),
            Some(CommandRefusal::UnusableName),
            "name {name:?}"
        );
    }
    assert!(commands::new_folder(&location, "Reports").is_ok());
}

#[test]
fn put_back_is_refused_anywhere_but_the_trash() {
    let items = vec![files_operations::TrashItemRef::new("/trash", "item")];
    assert_eq!(
        commands::restore_from_trash(&at("/srv"), items.clone()).err(),
        Some(CommandRefusal::NotInTrash)
    );
    assert!(
        commands::restore_from_trash(&Location::Trash(TrashLocation::Root), items)
            .unwrap()
            .validate()
            .is_ok()
    );
}

// --- Preferences ---------------------------------------------------------

#[test]
fn preferences_are_global_and_survive_a_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = PreferenceStore::at_path(root.path().join("better-os/files/view.json"));
    assert_eq!(store.load().preferences, FilesPreferences::default());

    let mut preferences = FilesPreferences::default();
    preferences.show_hidden = true;
    preferences.view_mode = ViewMode::List;
    preferences.scale = ItemScale::Large;
    preferences.locale = LocalePreference::ZhTw;
    preferences.set_order(
        SortOrder::new(SortKey::Size, SortDirection::Descending).with_folders_first(false),
    );
    store.save(&preferences).unwrap();

    let reloaded = PreferenceStore::at_path(store.path()).load();
    assert_eq!(reloaded.problem, None);
    assert_eq!(reloaded.preferences, preferences);
    assert!(reloaded.preferences.hidden().show_hidden);
    assert_eq!(reloaded.preferences.order().key, SortKey::Size);
    assert!(!reloaded.preferences.order().folders_first);
    // Every new tab starts from the same value: that is the whole policy.
    assert_eq!(
        reloaded.preferences.view_preferences().order,
        reloaded.preferences.order()
    );
}

#[test]
fn an_unreadable_preferences_file_falls_back_and_says_so() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("view.json");
    fs::write(&path, "{ not json").unwrap();
    let loaded = PreferenceStore::at_path(&path).load();
    assert_eq!(loaded.preferences, FilesPreferences::default());
    assert!(
        loaded
            .problem
            .as_deref()
            .is_some_and(|problem| problem.starts_with("files.prefs.error.unreadable")),
        "the window is told, rather than silently reverting the user's settings"
    );
}

// --- Sessions ------------------------------------------------------------

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    engine: Arc<JobEngine>,
}

fn fixture() -> (Fixture, FilesSession) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(home.join("Documents")).unwrap();
    fs::create_dir_all(home.join("Documents/reports")).unwrap();
    fs::write(home.join("Documents/notes.txt"), b"hello").unwrap();
    fs::write(home.join("Documents/.secret"), b"hidden").unwrap();

    let engine = Arc::new(JobEngine::new(EngineConfig {
        store: None,
        ..EngineConfig::default()
    }));
    let session = FilesSession::new(SessionSetup {
        start: Location::local(home.join("Documents")).unwrap(),
        preferences: FilesPreferences::default(),
        preference_store: PreferenceStore::at_path(root.path().join("prefs.json")),
        bookmark_store: BookmarkStore::at_path(root.path().join("bookmarks")),
        directories: UserDirectories::from_values(Some(&home), None),
        mounts: MountTable::new(Vec::new()),
        reader: Arc::new(FilesReader::new(ReaderConfig::new(), None)),
        engine: engine.clone(),
    });
    (
        Fixture {
            _root: root,
            home,
            engine,
        },
        session,
    )
}

/// Drains until the active listing finishes, with a deadline so a broken
/// reader fails the test rather than hanging it.
fn settle(session: &mut FilesSession) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.is_listing() && Instant::now() < deadline {
        session.pump();
        std::thread::yield_now();
    }
    session.pump();
    assert!(!session.is_listing(), "the listing never finished");
}

#[test]
fn a_session_lists_its_starting_directory_and_hides_dotfiles_by_default() {
    let (_fixture, mut session) = fixture();
    settle(&mut session);
    let names: Vec<String> = session
        .pane()
        .model()
        .iter_visible()
        .map(|entry| entry.name.clone())
        .collect();
    assert_eq!(names, ["reports", "notes.txt"]);
    assert_eq!(session.pane().model().total_len(), 3);
}

#[test]
fn ctrl_h_reveals_hidden_entries_immediately_and_persists_the_choice() {
    let (_fixture, mut session) = fixture();
    settle(&mut session);
    let token = session.pane().cancellation_token();

    session.dispatch(Command::ToggleHidden, 1, 10);
    assert_eq!(session.pane().model().visible_len(), 3);
    assert!(
        token.is_none_or(|token| !token.is_cancelled()),
        "revealing hidden entries must not restart the listing"
    );
    assert!(session.preferences.show_hidden);

    session.dispatch(Command::ToggleHidden, 1, 10);
    assert_eq!(session.pane().model().visible_len(), 2);
    assert!(!session.preferences.show_hidden);
}

#[test]
fn closing_a_tab_keeps_its_history_and_reopening_restores_it() {
    let (fixture, mut session) = fixture();
    settle(&mut session);

    let second = session.open_tab(Location::local(&fixture.home).unwrap(), true);
    settle(&mut session);
    session.navigate_to(Location::local(fixture.home.join("Documents")).unwrap());
    settle(&mut session);
    session.navigate_to(Location::local(fixture.home.join("Documents/reports")).unwrap());
    settle(&mut session);
    assert!(session.pane().history().can_go_back());

    session.close_tab(second);
    assert_eq!(session.tabs().len(), 1);
    assert!(session.tabs().can_restore());

    session.restore_tab();
    settle(&mut session);
    assert_eq!(
        session.location(),
        &Location::local(fixture.home.join("Documents/reports")).unwrap()
    );
    assert!(
        session.pane().history().can_go_back(),
        "a reopened tab keeps the history it was closed with"
    );
    session.go_back();
    settle(&mut session);
    assert_eq!(
        session.location(),
        &Location::local(fixture.home.join("Documents")).unwrap()
    );
}

#[test]
fn the_last_tab_cannot_be_closed_and_the_window_says_why() {
    let (_fixture, mut session) = fixture();
    let id = session.active_tab();
    session.close_tab(id);
    assert_eq!(session.tabs().len(), 1);
    assert_eq!(
        session.notice,
        Some(Notice::Navigation(files_core::NavigationError::LastTab))
    );
    assert_eq!(
        session.notice.as_ref().unwrap().message(&EN_US),
        EN_US.last_tab_stays_open
    );
}

#[test]
fn reopening_with_nothing_closed_says_so_rather_than_doing_nothing() {
    let (_fixture, mut session) = fixture();
    session.dispatch(Command::RestoreClosedTab, 1, 10);
    assert_eq!(
        session.notice,
        Some(Notice::Navigation(
            files_core::NavigationError::NothingToRestore
        ))
    );
}

#[test]
fn a_new_tab_inherits_the_global_view_preferences() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    session.dispatch(Command::ToggleHidden, 1, 10);
    session.set_sort_key(SortKey::Size);

    let second = session.open_tab(Location::local(&fixture.home).unwrap(), true);
    settle(&mut session);
    let tab = session.tabs().get(second).unwrap();
    assert!(tab.preferences().hidden.show_hidden);
    assert_eq!(tab.preferences().order.key, SortKey::Size);
    assert_eq!(session.pane().preferences().order.key, SortKey::Size);
}

#[test]
fn opening_a_directory_navigates_and_opening_a_file_leaves_a_notice() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    session.apply_selection(SelectionInput::Click(0), 1);
    session.dispatch(Command::Open, 1, 10);
    settle(&mut session);
    assert_eq!(
        session.location(),
        &Location::local(fixture.home.join("Documents/reports")).unwrap()
    );

    session.go_back();
    settle(&mut session);
    session.apply_selection(SelectionInput::Click(1), 1);
    session.dispatch(Command::Open, 1, 10);
    assert!(matches!(session.notice, Some(Notice::NoHandler(_))));
    assert_eq!(
        session.location(),
        &Location::local(fixture.home.join("Documents")).unwrap(),
        "a file does not navigate"
    );
}

#[test]
fn a_typed_path_navigates_and_a_bad_one_leaves_the_tab_where_it_was() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let validator = crate::toolbar::FilesystemValidator;

    session.submit_path(
        fixture.home.join("Documents/reports").to_str().unwrap(),
        Some(&fixture.home),
        &validator,
    );
    settle(&mut session);
    assert_eq!(
        session.location(),
        &Location::local(fixture.home.join("Documents/reports")).unwrap()
    );

    session.submit_path("/definitely/not/here", Some(&fixture.home), &validator);
    assert_eq!(session.notice, Some(Notice::Path(PathRejection::NotFound)));
    assert_eq!(
        session.location(),
        &Location::local(fixture.home.join("Documents/reports")).unwrap()
    );
}

#[test]
fn pinning_writes_a_bookmark_and_leaves_the_directory_where_it_is() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let target = Location::local(fixture.home.join("Documents/reports")).unwrap();

    session.pin(&target);
    assert_eq!(session.bookmarks.len(), 1);
    assert!(
        fixture.home.join("Documents/reports").is_dir(),
        "pinning must never move the directory"
    );

    session.pin(&target);
    assert_eq!(session.notice, Some(Notice::AlreadyPinned));
    assert_eq!(session.bookmarks.len(), 1);

    session.remove_bookmark(0);
    assert_eq!(session.bookmarks.len(), 0);
    assert!(
        fixture.home.join("Documents/reports").is_dir(),
        "removing a bookmark must never delete the directory"
    );
}

#[test]
fn the_keyboard_reorders_favourites_without_a_pointer() {
    let (fixture, mut session) = fixture();
    session.pin(&Location::local(fixture.home.join("Documents")).unwrap());
    session.pin(&Location::local(fixture.home.join("Documents/reports")).unwrap());
    session.focus = Focus::Sidebar;
    session.sidebar_cursor = Some(1);

    session.dispatch(Command::MoveBookmarkUp, 1, 10);
    let names: Vec<String> = session
        .bookmarks
        .bookmarks()
        .iter()
        .map(|bookmark| bookmark.display_name())
        .collect();
    assert_eq!(names, ["reports", "Documents"]);
    assert_eq!(session.sidebar_cursor, Some(0));

    session.dispatch(Command::RemoveBookmark, 1, 10);
    assert_eq!(session.bookmarks.len(), 1);
}

#[test]
fn a_permanent_delete_asks_before_it_builds_a_job() {
    let (_fixture, mut session) = fixture();
    settle(&mut session);
    session.apply_selection(SelectionInput::SelectAll, 1);

    session.dispatch(Command::DeletePermanently, 1, 10);
    let Some(PendingDialog::ConfirmDelete { targets }) = session.dialog.clone() else {
        panic!("a permanent delete must raise a confirmation");
    };
    assert_eq!(targets.len(), 2);
    assert!(
        session.jobs.is_empty(),
        "nothing is submitted until the question is answered"
    );
}

#[test]
fn copy_and_paste_build_a_real_copy_job_that_runs() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let destination = fixture.home.join("Documents/reports");

    session.apply_selection(SelectionInput::Click(1), 1);
    session.dispatch(Command::Copy, 1, 10);
    assert_eq!(session.clipboard.len(), 1);

    session.navigate_to(Location::local(&destination).unwrap());
    settle(&mut session);
    session.dispatch(Command::Paste, 1, 10);

    let id = session.jobs.first().map(|job| job.id).expect("a job");
    let finished = fixture
        .engine
        .wait(id, Duration::from_secs(10))
        .expect("the copy finished");
    assert_eq!(finished.state, JobState::Completed);
    assert!(destination.join("notes.txt").is_file());
}

/// Milestone M37, at the model level.
#[test]
fn closing_a_window_leaves_its_jobs_running() {
    let (fixture, mut session) = fixture();
    settle(&mut session);

    // A copy big enough that it is still running when the window goes away.
    let source = fixture.home.join("Documents/big.bin");
    fs::write(&source, vec![0u8; 8 * 1024 * 1024]).unwrap();
    let destination = fixture.home.join("Documents/reports");

    session.clipboard = Clipboard::Copy(vec![LocalPath::new(&source).unwrap()]);
    session.navigate_to(Location::local(&destination).unwrap());
    settle(&mut session);
    session.dispatch(Command::Paste, 1, 10);
    let id = session.jobs.first().map(|job| job.id).expect("a job");

    // The window closes. Everything the window owned goes with it.
    drop(session);

    let finished = fixture
        .engine
        .wait(id, Duration::from_secs(30))
        .expect("the job is still known to the engine");
    assert_eq!(
        finished.state,
        JobState::Completed,
        "a copy is not a window's work"
    );
    assert_eq!(
        fs::metadata(destination.join("big.bin")).unwrap().len(),
        8 * 1024 * 1024
    );
    // And the engine's own record of it is still consistent.
    let snapshot = fixture.engine.snapshot(id).expect("the snapshot survived");
    assert_eq!(snapshot.progress.items_failed, 0);
    assert_eq!(snapshot.progress.bytes_done, 8 * 1024 * 1024);
}

#[test]
fn a_location_this_build_cannot_list_says_so_rather_than_looking_empty() {
    let (_fixture, mut session) = fixture();
    session.navigate_to(Location::Applications);
    settle(&mut session);
    assert!(matches!(
        session.pane().model().status(),
        files_core::ListingStatus::Failed(files_core::ListingError::NotListable(_))
    ));
    assert_eq!(
        crate::content::unlistable_reason(session.location(), &EN_US),
        None,
        "Applications is listable in principle; this build simply has no catalog wired in yet"
    );
}

// --- Localization and overflow -------------------------------------------

/// Every label the toolbar and the sidebar draw, in one language.
fn chrome_labels(c: &'static crate::i18n::Copy) -> Vec<&'static str> {
    vec![
        c.go_back,
        c.go_forward,
        c.go_to_parent,
        c.reload,
        c.view_grid,
        c.view_list,
        c.show_hidden,
        c.hide_hidden,
        c.sort_by,
        c.ascending,
        c.descending,
        c.folders_first,
        c.item_size,
        c.operation_center,
        c.new_tab,
        c.close_tab,
        c.reopen_closed_tab,
    ]
}

#[test]
fn every_column_header_fits_its_column_in_both_languages() {
    for c in [&EN_US, &ZH_TW] {
        for column in crate::content::ListColumn::ALL {
            assert!(
                header_fits(column.header(c), column.width()),
                "{:?} header {:?} does not fit {}",
                column,
                column.header(c),
                column.width()
            );
        }
    }
}

#[test]
fn every_sidebar_label_fits_the_sidebar_at_every_supported_scale() {
    for c in [&EN_US, &ZH_TW] {
        let mut labels: Vec<&'static str> = SidebarSection::ALL
            .into_iter()
            .map(|section| section.title(c))
            .collect();
        labels.extend([
            c.place_home,
            c.place_desktop,
            c.place_documents,
            c.place_downloads,
            c.place_music,
            c.place_pictures,
            c.place_videos,
            c.place_templates,
            c.place_public,
            c.place_trash,
        ]);
        for scale in [1.0, 1.25, 1.5] {
            for label in &labels {
                assert!(
                    sidebar_label_fits(label, scale),
                    "{label:?} does not fit the sidebar at {scale}"
                );
            }
        }
    }
}

/// The empty-state sentences are the one sidebar text that is allowed not to
/// fit on a line, because they are drawn as wrapping prose rather than as a
/// truncated row label. This asserts that they genuinely need to wrap, so the
/// exemption is a measured fact rather than a hole in the coverage above.
#[test]
fn the_sidebar_empty_state_sentences_are_the_ones_that_wrap() {
    for sentence in [EN_US.no_devices, EN_US.no_favorites] {
        assert!(
            !sidebar_label_fits(sentence, 1.25),
            "{sentence:?} fits on one line at 125%, so it belongs in the label check"
        );
    }
    // The Chinese sentences are short enough to fit. They are still drawn as
    // wrapping prose, so the exemption is about how the element is built
    // rather than about how long one translation happens to be.
    assert!(sidebar_label_fits(ZH_TW.no_devices, 1.5));
}

#[test]
fn the_toolbar_wraps_rather_than_clipping_when_the_window_is_small_or_scaled() {
    for c in [&EN_US, &ZH_TW] {
        let labels = chrome_labels(c);
        // A wide window at 100% keeps the controls on one line.
        assert_eq!(
            toolbar_layout(2_200.0, 1.0, &labels[..6]),
            ControlLayout::Inline,
            "a wide window should not wrap a short control row"
        );
        // Everything else wraps, which is a supported outcome; clipping is not.
        for (width, scale) in [
            (1_280.0, 1.0),
            (1_280.0, 1.25),
            (1_280.0, 1.5),
            (900.0, 1.0),
            (900.0, 1.5),
        ] {
            assert_eq!(
                toolbar_layout(width, scale, &labels),
                ControlLayout::Wrapped,
                "the full control row at {width}px and {scale}x must wrap"
            );
        }
    }
}

#[test]
fn the_detailed_list_scrolls_sideways_rather_than_clipping_a_column() {
    let total: f32 = crate::content::ListColumn::ALL
        .iter()
        .map(|column| column.width())
        .sum();
    assert_eq!(
        crate::layout::table_layout(1_600.0, 1.0, total),
        crate::layout::TableLayout::Fits
    );
    assert_eq!(
        crate::layout::table_layout(1_280.0, 1.5, total),
        crate::layout::TableLayout::HorizontalScroll
    );
}

#[test]
fn both_languages_define_every_string_and_none_of_them_is_empty() {
    for c in [&EN_US, &ZH_TW] {
        for label in chrome_labels(c) {
            assert!(!label.trim().is_empty());
        }
        for state in JobState::ALL {
            assert!(!crate::i18n::job_state_label(state, c).trim().is_empty());
        }
        for kind in [
            OperationKind::CreateFile,
            OperationKind::CreateFolder,
            OperationKind::Rename,
            OperationKind::BulkRename,
            OperationKind::Copy,
            OperationKind::Move,
            OperationKind::Duplicate,
            OperationKind::Trash,
            OperationKind::RestoreFromTrash,
            OperationKind::PermanentDelete,
            OperationKind::Checksum,
        ] {
            assert!(!crate::i18n::job_kind_label(kind, c).trim().is_empty());
        }
    }
}

#[test]
fn switching_language_re_renders_the_notice_that_is_on_screen() {
    let notice = Notice::Path(PathRejection::NotFound);
    assert_eq!(notice.message(&EN_US), EN_US.path_not_found);
    assert_eq!(notice.message(&ZH_TW), ZH_TW.path_not_found);
    assert_ne!(notice.message(&EN_US), notice.message(&ZH_TW));
}

#[test]
fn a_wide_script_label_is_measured_as_wide() {
    assert!(label_width("垃圾桶") > label_width("Trash"));
    assert_eq!(copy(Locale::ZhTw).place_trash, "垃圾桶");
    assert_eq!(copy(Locale::EnUs).place_trash, "Trash");
}

// --- Formatting ----------------------------------------------------------

#[test]
fn sizes_and_times_read_the_way_a_file_manager_shows_them() {
    assert_eq!(crate::format::bytes(0), "0 B");
    assert_eq!(crate::format::bytes(999), "999 B");
    assert_eq!(crate::format::bytes(1_024), "1.00 KB");
    assert_eq!(crate::format::bytes(1_048_576), "1.00 MB");
    assert_eq!(crate::format::bytes(15 * 1_048_576), "15.0 MB");
    assert_eq!(crate::format::bytes(150 * 1_048_576), "150 MB");

    assert_eq!(
        crate::format::file_time(Some(files_core::FileTime::new(0, 0))),
        "1970-01-01 00:00"
    );
    assert_eq!(
        crate::format::file_time(Some(files_core::FileTime::new(1_700_000_000, 0))),
        "2023-11-14 22:13"
    );
    assert_eq!(crate::format::file_time(None), "—");

    // A directory has no byte count and says so rather than showing zero.
    assert_eq!(
        crate::format::entry_size(files_core::EntrySize::Unknown),
        "—"
    );
    assert_eq!(
        crate::format::entry_size(files_core::EntrySize::NotApplicable),
        "—"
    );
    assert_eq!(crate::format::duration(Duration::from_secs(3_725)), "1h 2m");
}
