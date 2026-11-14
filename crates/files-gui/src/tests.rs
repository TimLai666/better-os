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
use crate::content::{ContentView, SelectionInput, header_click};
use crate::i18n::{EN_US, Locale, ZH_TW, copy};
use crate::keys::{Command, Focus, Modifiers, command_for};
use crate::layout::{
    ControlLayout, header_fits, label_width, sidebar_label_fits, split_viewport_width,
    toolbar_layout, visible_rows,
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
        ("p", Modifiers::control_shift(), Command::Compress),
        ("e", Modifiers::control_shift(), Command::Extract),
    ] {
        assert_eq!(
            command_for(key, modifiers, None, content),
            Some(expected),
            "{key} with {modifiers:?}"
        );
    }
}

#[test]
fn compress_and_extract_need_both_modifiers_and_take_nothing_already_bound() {
    let content = Focus::Content;
    // Without Shift, Ctrl+P pins the folder, as in folio, and Ctrl+E stays
    // free.
    assert_eq!(
        command_for("p", Modifiers::control(), None, content),
        Some(Command::TogglePin)
    );
    assert_eq!(command_for("e", Modifiers::control(), None, content), None);
    // And the shortcuts already taken with Ctrl+Shift keep their meaning.
    // Ctrl+Shift+C copies the path as text, as in folio.
    for (key, expected) in [
        ("c", Command::CopyPath),
        ("n", Command::NewFolder),
        ("o", Command::OpenWith),
        ("t", Command::RestoreClosedTab),
    ] {
        assert_eq!(
            command_for(key, Modifiers::control_shift(), None, content),
            Some(expected),
            "Ctrl+Shift+{key}"
        );
    }
}

#[test]
fn the_format_chooser_is_driven_from_the_keyboard() {
    use crate::keys::{ChooserKey, chooser_key_for};
    for (key, modifiers, expected) in [
        ("down", Modifiers::NONE, Some(ChooserKey::Next)),
        ("right", Modifiers::NONE, Some(ChooserKey::Next)),
        ("tab", Modifiers::NONE, Some(ChooserKey::Next)),
        ("up", Modifiers::NONE, Some(ChooserKey::Previous)),
        ("left", Modifiers::NONE, Some(ChooserKey::Previous)),
        ("tab", Modifiers::shift(), Some(ChooserKey::Previous)),
        ("enter", Modifiers::NONE, Some(ChooserKey::Confirm)),
        ("space", Modifiers::NONE, Some(ChooserKey::Confirm)),
        ("escape", Modifiers::NONE, Some(ChooserKey::Cancel)),
        // Nothing else reaches the window behind the chooser.
        ("delete", Modifiers::NONE, None),
        ("c", Modifiers::control(), None),
    ] {
        assert_eq!(
            chooser_key_for(key, modifiers),
            expected,
            "{key} with {modifiers:?}"
        );
    }
}

#[test]
fn the_chooser_starts_on_zip_and_wraps_both_ways() {
    use crate::commands::{CompressChooser, CompressRequest};
    use files_operations::ArchiveFormat;
    let mut chooser = CompressChooser::new(CompressRequest {
        parent: LocalPath::new("/home/user").unwrap(),
        sources: vec![LocalPath::new("/home/user/report.pdf").unwrap()],
        stem: "report".into(),
    });
    assert_eq!(chooser.focused(), ArchiveFormat::ALL[0]);
    assert_eq!(chooser.focused(), ArchiveFormat::Zip);
    for expected in ArchiveFormat::ALL.iter().skip(1) {
        chooser.next();
        assert_eq!(chooser.focused(), *expected);
    }
    chooser.next();
    assert_eq!(chooser.focused(), ArchiveFormat::Zip, "wraps forwards");
    chooser.previous();
    assert_eq!(
        chooser.focused(),
        ArchiveFormat::ALL[ArchiveFormat::ALL.len() - 1],
        "wraps backwards"
    );
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

#[test]
fn each_half_of_a_split_window_lays_out_at_its_own_width() {
    // 1,000 wide with the 48 px navigation pane: 951 left for the two
    // halves after the divider, 475.5 each. A pane subtracts the navigation
    // pane from its viewport, so it is handed 48 + 475.5.
    let viewport = split_viewport_width(1_000.0, 48.0);
    assert_eq!(viewport, 523.5);
    // What the list's columns and the grid's tiles are computed from is
    // half, not the whole window.
    let content = viewport - 48.0;
    assert!(content * 2.0 + 1.0 <= 1_000.0 - 48.0 + 0.01);
    let grid = ContentView::new(ViewMode::Grid, ItemScale::Medium);
    assert!(grid.columns(content - 40.0) < grid.columns(1_000.0 - 48.0 - 40.0));
    assert!(
        crate::content::visible_columns(content - 48.0).len()
            < crate::content::visible_columns(1_000.0 - 48.0 - 48.0).len()
    );
    // A window too narrow for two halves never hands out a negative width.
    assert_eq!(split_viewport_width(40.0, 48.0), 48.0);
}

// --- Open routing --------------------------------------------------------

#[test]
fn opening_an_entry_routes_by_what_it_is() {
    // The routing itself lives in `files_core::open_intent`, which is a closed
    // enum rather than a path, so the three cases are three different actions.
    let directory = Entry::file("notes", local("/d/notes"), EntryKind::Directory);
    assert_eq!(
        files_core::open_intent(&directory),
        files_core::OpenIntent::Navigate(Box::new(at("/d/notes")))
    );

    let file = Entry::file("notes.txt", local("/d/notes.txt"), EntryKind::File);
    assert_eq!(
        files_core::open_intent(&file),
        files_core::OpenIntent::OpenFile {
            path: local("/d/notes.txt"),
            mime: None,
        }
    );

    let socket = Entry::file("socket", local("/d/socket"), EntryKind::Special);
    assert_eq!(
        files_core::open_intent(&socket),
        files_core::OpenIntent::Refused(files_core::OpenRefusal::NotOpenable)
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
fn a_folder_landing_on_a_folder_also_offers_merge_and_says_nothing_is_deleted() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("a/photos")).unwrap();
    fs::create_dir_all(root.path().join("b/photos")).unwrap();
    let folders = Conflict::exists(
        Some(root.path().join("a/photos")),
        root.path().join("b/photos"),
    );
    assert!(opcenter::folders_clash(&folders));
    assert_eq!(
        choices_for(&folders),
        vec![
            Resolution::Skip,
            Resolution::Merge,
            Resolution::Overwrite,
            Resolution::Rename,
            Resolution::Cancel
        ]
    );
    for copy in [&EN_US, &crate::i18n::ZH_TW] {
        assert!(
            copy.conflict_merge_note.contains("trash")
                || copy.conflict_merge_note.contains("垃圾桶")
        );
        assert!(
            copy.conflict_replace_note.contains("trash")
                || copy.conflict_replace_note.contains("垃圾桶")
        );
        assert!(!crate::i18n::resolution_label(Resolution::Merge, copy).is_empty());
    }
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
        // A clash answered in a test never reaches the person's trash.
        displaced_trash: Some(root.path().join("Trash")),
        ..EngineConfig::default()
    }));
    let reader = Arc::new(FilesReader::new(
        ReaderConfig::new(),
        None,
        crate::apps::CatalogHandle::empty(Default::default()),
    ));
    let session = FilesSession::new(SessionSetup {
        start: Location::local(home.join("Documents")).unwrap(),
        preferences: FilesPreferences::default(),
        preference_store: PreferenceStore::at_path(root.path().join("prefs.json")),
        view_store: crate::settings_store::ViewStateStore::at_path(root.path().join("views.json")),
        bookmark_store: BookmarkStore::at_path(root.path().join("bookmarks")),
        directories: UserDirectories::from_values(Some(&home), None),
        mounts: MountTable::new(Vec::new()),
        reader: reader.clone(),
        engine: engine.clone(),
        catalog: reader.catalog().clone(),
        defaults: Box::new(crate::openwith::SessionDefaults::fixed([])),
        spawner: Box::new(app_catalog_platform::RecordingSpawner::new()),
        link: Box::new(crate::devices::NoDeviceLink),
        preview: crate::preview::PreviewPanel::default(),
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
fn a_view_is_remembered_for_its_own_folder_and_the_rest_keep_the_default() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let documents = fixture.home.join("Documents");
    let reports = documents.join("reports");

    session.set_view_mode(ViewMode::List);
    session.set_sort_key(SortKey::Size);
    session.toggle_hidden();
    assert_eq!(session.pane().model().visible_len(), 3);

    session.navigate_to(Location::local(&reports).unwrap());
    settle(&mut session);
    assert_eq!(
        session.preferences.view_mode,
        ViewMode::Grid,
        "reports has the default"
    );
    assert_eq!(session.preferences.order().key, SortKey::Name);
    assert!(!session.preferences.show_hidden);

    session.go_back();
    settle(&mut session);
    assert_eq!(
        session.preferences.view_mode,
        ViewMode::List,
        "Documents kept its own"
    );
    assert_eq!(session.preferences.order().key, SortKey::Size);
    assert_eq!(
        session.pane().model().visible_len(),
        3,
        "and its hidden files"
    );
    assert!(session.folder_has_own_view());

    // Nothing was written into the folders themselves.
    let names: Vec<String> = fs::read_dir(&documents)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names
            .iter()
            .all(|name| name != ".directory" && name != ".DS_Store")
    );

    // A restart reads the same file back, and remembers the last folder.
    let store = crate::settings_store::ViewStateStore::at_path(
        fixture.home.parent().unwrap().join("views.json"),
    );
    let state = store.load();
    assert_eq!(state.view_for(Some(&documents)).mode, ViewMode::List);
    assert_eq!(state.last_folder(), Some(documents.as_path()));
}

#[test]
fn using_a_view_everywhere_makes_it_the_default() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    session.set_scale(ItemScale::Large);
    session.use_view_everywhere();
    session.navigate_to(Location::local(fixture.home.join("Documents/reports")).unwrap());
    settle(&mut session);
    assert_eq!(session.preferences.scale, ItemScale::Large);
    assert!(!session.folder_has_own_view());
}

#[test]
fn the_sort_steps_through_folios_five_orders() {
    let (_fixture, mut session) = fixture();
    settle(&mut session);
    session.step_sort(true);
    assert_eq!(session.preferences.order().key, SortKey::Type);
    session.step_sort(false);
    assert_eq!(session.preferences.order().key, SortKey::Name);
}

#[test]
fn ctrl_z_undoes_the_folder_just_made_into_the_trash_and_ctrl_y_brings_it_back() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let trash = fixture.home.parent().unwrap().join("Trash");
    session.set_undo_trash(trash.clone());
    let documents = fixture.home.join("Documents");
    session.submit(files_operations::JobSpec::new(
        files_operations::Operation::CreateFolder {
            parent: LocalPath::new(&documents).unwrap(),
            name: "Made".into(),
        },
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.history.can_undo() && Instant::now() < deadline {
        session.pump();
        std::thread::yield_now();
    }
    assert!(documents.join("Made").is_dir());
    assert_eq!(
        session.history.undo_name(&EN_US).as_deref(),
        Some("Undo New Folder")
    );

    session.dispatch(Command::Undo, 1, 10);
    assert!(!documents.join("Made").exists());
    assert!(
        trash.join("files/Made").is_dir(),
        "undo puts it in the trash"
    );
    assert_eq!(
        session.notice.as_ref().unwrap().message(&EN_US),
        "Undid New Folder."
    );

    session.dispatch(Command::Redo, 1, 10);
    assert!(documents.join("Made").is_dir());

    // With nothing left to redo, it says so.
    session.dispatch(Command::Redo, 1, 10);
    assert_eq!(
        session.notice.as_ref().unwrap().message(&EN_US),
        EN_US.nothing_to_redo
    );
}

#[test]
fn a_filter_narrows_this_folder_and_is_gone_in_the_next() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    fs::write(fixture.home.join("Documents/Notes-2.TXT"), b"x").unwrap();
    session.reload();
    settle(&mut session);
    session.set_filter("NOTES");
    let names: Vec<String> = (0..session.row_count())
        .filter_map(|row| session.entry_at(row).map(|entry| entry.name.clone()))
        .collect();
    assert_eq!(names, ["Notes-2.TXT", "notes.txt"]);
    assert_eq!(session.filter_status(), Some((2, 4)));

    session.navigate_to(Location::local(fixture.home.join("Documents/reports")).unwrap());
    settle(&mut session);
    session.go_back();
    settle(&mut session);
    assert_eq!(session.filter(), "");
    assert_eq!(session.filter_status(), None);
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
fn a_new_tab_takes_the_view_of_the_folder_it_opens() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    // Documents gets its own view.
    session.dispatch(Command::ToggleHidden, 1, 10);
    session.set_sort_key(SortKey::Size);

    // Home has none, so a tab there opens with the default.
    let home = session.open_tab(Location::local(&fixture.home).unwrap(), true);
    settle(&mut session);
    let tab = session.tabs().get(home).unwrap();
    assert!(!tab.preferences().hidden.show_hidden);
    assert_eq!(tab.preferences().order.key, SortKey::Name);

    // A second tab on Documents opens the way Documents was left.
    let documents = session.open_tab(
        Location::local(fixture.home.join("Documents")).unwrap(),
        true,
    );
    settle(&mut session);
    let tab = session.tabs().get(documents).unwrap();
    assert!(tab.preferences().hidden.show_hidden);
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
    // Ticket 35 replaced the "no handler is wired up" notice with real
    // routing. The fixture reader detects no MIME type, so the honest answer
    // is that there is nothing to choose from rather than a chooser opened
    // for a type nobody knows.
    assert_eq!(session.notice, Some(Notice::NoMimeType));
    assert!(session.chooser.is_none());
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
    // A network location is the one this build genuinely cannot list.
    session.navigate_to(Location::parse_uri("smb://server/share"));
    settle(&mut session);
    assert!(matches!(
        session.pane().model().status(),
        files_core::ListingStatus::Failed(files_core::ListingError::NotListable(_))
    ));
    assert_eq!(
        crate::content::unlistable_reason(session.location(), &EN_US),
        Some(EN_US.not_listable_here)
    );
}

#[test]
fn the_applications_location_lists_from_the_catalog_and_is_not_a_directory() {
    let (_fixture, mut session) = fixture();
    session.navigate_to(Location::Applications);
    settle(&mut session);
    // The empty catalog the fixture carries lists as an empty location, not as
    // a failure: "no applications" and "this cannot be listed" are different
    // sentences.
    assert!(matches!(
        session.pane().model().status(),
        files_core::ListingStatus::Complete
    ));
    assert_eq!(
        crate::content::unlistable_reason(session.location(), &EN_US),
        None
    );
    // The rule the whole location exists to keep.
    assert_eq!(session.location().as_local_path(), None);
    assert!(!session.location().is_filesystem_backed());
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
        c.compress,
        c.extract,
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
            OperationKind::Archive,
            OperationKind::Extract,
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

#[test]
fn an_image_without_a_thumbnail_gets_one_made_in_the_background() {
    let root = tempfile::tempdir().unwrap();
    let photo = root.path().join("photo.png");
    image::RgbaImage::from_pixel(400, 400, image::Rgba([10, 120, 200, 255]))
        .save(&photo)
        .unwrap();
    let cache_root = root.path().join("thumbnails");
    let thumbnails = crate::views::Thumbnails::with_cache(Some(
        files_preview::thumbnails::ThumbnailCache::at(&cache_root),
    ));

    // The first ask returns at once with nothing, and the work is queued.
    assert_eq!(thumbnails.for_file(&photo, "image/png"), None);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let made = loop {
        if let Some(path) = thumbnails.for_file(&photo, "image/png") {
            break path;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the thumbnail was never made"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(
        made,
        cache_root.join("large").join(format!(
            "{}.png",
            files_preview::thumbnails::hash(&files_preview::thumbnails::file_uri(&photo))
        ))
    );
    assert!(thumbnails.ready.load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn escape_clears_the_selection_first_and_then_cancels_the_running_job() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    // A copy parked on a clash stays running until it is answered.
    let documents = fixture.home.join("Documents");
    fs::create_dir_all(fixture.home.join("other")).unwrap();
    fs::write(fixture.home.join("other/notes.txt"), b"clash").unwrap();
    session.submit(files_operations::JobSpec::new(
        files_operations::Operation::Copy {
            sources: vec![LocalPath::new(fixture.home.join("other/notes.txt")).unwrap()],
            destination: LocalPath::new(&documents).unwrap(),
        },
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session
        .jobs
        .iter()
        .any(|job| job.state == files_operations::JobState::WaitingOnConflict)
    {
        assert!(Instant::now() < deadline, "the copy never parked");
        session.refresh_jobs();
        std::thread::yield_now();
    }
    session.notice = None;
    session.apply_selection(SelectionInput::SelectAll, 1);
    session.dispatch(Command::ClearSelection, 1, 10);
    assert!(
        session.jobs.iter().any(|job| !job.state.is_terminal()),
        "the first Escape only cleared the selection"
    );
    session.notice = None;
    session.dispatch(Command::ClearSelection, 1, 10);
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.jobs.iter().any(|job| !job.state.is_terminal()) {
        assert!(Instant::now() < deadline, "the job was not cancelled");
        session.refresh_jobs();
        std::thread::yield_now();
    }
    assert_eq!(fs::read(documents.join("notes.txt")).unwrap(), b"hello");
}

#[test]
fn ctrl_f6_moves_the_keyboard_to_the_sidebar_and_back() {
    let (_fixture, mut session) = fixture();
    assert_eq!(session.focus, Focus::Content);
    session.dispatch(Command::CyclePane, 1, 10);
    assert_eq!(session.focus, Focus::Sidebar);
    session.dispatch(Command::CyclePane, 1, 10);
    assert_eq!(session.focus, Focus::Content);
}

/// A server that wants "ann" / "secret".
struct FakeServer {
    folder: std::path::PathBuf,
}

impl crate::network::Mounter for FakeServer {
    fn mount(
        &self,
        _address: &str,
        credentials: Option<&crate::network::Credentials>,
    ) -> Result<std::path::PathBuf, crate::network::MountError> {
        match credentials {
            Some(given) if given.user == "ann" && given.password == "secret" => {
                Ok(self.folder.clone())
            }
            _ => Err(crate::network::MountError::NeedsPassword),
        }
    }
}

#[test]
fn a_server_address_asks_for_a_password_then_opens_the_mounted_folder() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let mounted = fixture.home.join("Documents/reports");
    session.set_mounter(std::sync::Arc::new(FakeServer {
        folder: mounted.clone(),
    }));
    session.submit_path(
        "sftp://nas/home",
        None,
        &crate::toolbar::FilesystemValidator,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.is_mounting() {
        assert!(Instant::now() < deadline);
        session.pump();
        std::thread::yield_now();
    }
    assert_eq!(
        session.dialog,
        Some(PendingDialog::ServerLogin {
            address: "sftp://nas/home".into()
        })
    );
    session.dialog = None;
    session.mount_server(
        "sftp://nas/home".into(),
        Some(crate::network::Credentials {
            user: "ann".into(),
            password: "secret".into(),
        }),
    );
    while session.is_mounting() {
        assert!(Instant::now() < deadline);
        session.pump();
        std::thread::yield_now();
    }
    assert_eq!(session.location(), &Location::local(&mounted).unwrap());
}

#[test]
fn the_side_preview_follows_a_click_without_waiting_for_anything_else() {
    let (_fixture, mut session) = fixture();
    settle(&mut session);
    if !session.preview.open {
        session.toggle_preview();
    }
    session.apply_selection(SelectionInput::Clear, 1);
    session.pump();
    assert_eq!(
        session.preview.placeholder(&EN_US),
        Some(EN_US.preview_nothing_selected)
    );
    let notes = session
        .pane()
        .model()
        .iter_visible()
        .position(|entry| entry.name == "notes.txt")
        .unwrap();
    // Only the click; no pump in between.
    session.apply_selection(SelectionInput::Click(notes), 1);
    assert_ne!(
        session.preview.placeholder(&EN_US),
        Some(EN_US.preview_nothing_selected),
        "the pane is preparing notes.txt, not asking for a selection"
    );
}

/// Two folders each with a `notes.txt`: going from one to the other shows
/// the second file, not the first one's preview kept because the names
/// match.
#[test]
fn the_same_name_in_another_folder_is_a_new_preview() {
    let root = tempfile::tempdir().unwrap();
    let mut panel = crate::preview::PreviewPanel::default();
    panel.toggle();
    let mut ask = |folder: &str| {
        let dir = root.path().join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("notes.txt"), folder).unwrap();
        let entry = Entry::file(
            "notes.txt",
            files_core::location::LocalPath::new(dir.join("notes.txt")).unwrap(),
            EntryKind::File,
        );
        panel.request_for(Some(&entry), &Location::local(&dir).unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        while panel.is_waiting() && Instant::now() < deadline {
            panel.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        match panel.slot() {
            crate::preview::PreviewSlot::Ready(preview) => match preview.as_ref() {
                files_preview::Preview::Text(text) => format!("{text:?}"),
                other => format!("{other:?}"),
            },
            other => format!("{other:?}"),
        }
    };
    assert!(ask("first").contains("first"));
    assert!(ask("second").contains("second"), "the second folder's file");
}

/// At 960 wide with the operation center open, the list's columns are
/// chosen for what is left beside it (the panel is 420 wide, not the
/// preview's 320), so the last one steps aside instead of being cut off.
#[test]
fn the_list_columns_fit_beside_every_open_panel() {
    use crate::layout::side_panels_width;
    assert_eq!(side_panels_width(false, false, false, false), 0.0);
    assert_eq!(side_panels_width(false, true, false, false), 320.0);
    // A compact window draws no preview.
    assert_eq!(side_panels_width(false, true, true, false), 0.0);
    assert_eq!(side_panels_width(false, true, false, true), 740.0);
    assert_eq!(side_panels_width(true, true, false, false), 360.0);

    let content = 960.0 - 48.0 - side_panels_width(false, false, true, true) - 48.0;
    let shown = crate::content::visible_columns(content);
    let used: f32 = shown
        .iter()
        .filter(|column| **column != crate::content::ListColumn::Name)
        .map(|column| column.width() + 8.0)
        .sum();
    assert!(
        used + crate::content::MIN_NAME_WIDTH <= content,
        "{shown:?} need {used} beside the name in {content}"
    );
}

/// Done things read as a success and news as information; only what needs
/// a look uses the warning style.
#[test]
fn a_notice_is_drawn_in_the_tone_of_what_it_says() {
    use crate::session::NoticeTone;
    assert_eq!(
        Notice::Undo {
            undoing: true,
            outcome: Ok(crate::undo::ChangeKind::Move),
        }
        .tone(),
        NoticeTone::Success
    );
    let erased = |failure: Option<String>| Notice::Erased {
        label: "Stick".into(),
        format: true,
        failure,
    };
    assert_eq!(erased(None).tone(), NoticeTone::Success);
    assert_eq!(
        erased(Some("storage.erase.busy".into())).tone(),
        NoticeTone::Warning
    );
    assert_eq!(
        Notice::WritingImage {
            label: "Stick".into(),
            percent: 40,
        }
        .tone(),
        NoticeTone::Information
    );
}

// --- Restart after an update (ticket 105) ---------------------------------

#[test]
fn a_restart_reopens_the_tabs_the_front_tab_and_its_selection() {
    let (first, mut before) = fixture();
    settle(&mut before);
    before.open_tab(Location::local(first.home.clone()).unwrap(), false);
    before.open_tab(Location::Trash(TrashLocation::Root), false);
    before.select_by_name("notes.txt");
    before.operations_open = true;
    before.set_search_text("note");
    let view = before.view();
    assert_eq!(view.tabs.len(), 3);
    assert_eq!(view.active, 0);
    assert_eq!(view.selected, vec!["notes.txt".to_string()]);

    // Through the hand-off's own encoding, as the next program reads it.
    let carried: crate::restart::FilesView = serde_json::from_str(
        &serde_json::to_string(&crate::restart::FilesView {
            session: view.clone(),
            path_draft: Some("/home/someone/Doc".into()),
            scroll: 240.0,
        })
        .unwrap(),
    )
    .unwrap();

    // The new program starts in the home folder, as a fresh window would.
    let (_other, mut after) = fixture();
    assert!(after.restore_view(&carried.session));
    settle(&mut after);
    let reopened = after.view();
    assert_eq!(reopened.tabs, view.tabs);
    assert_eq!(reopened.active, 0);
    assert_eq!(reopened.selected, vec!["notes.txt".to_string()]);
    assert_eq!(reopened.search, "note");
    assert!(reopened.operations_open);
    assert_eq!(carried.path_draft.as_deref(), Some("/home/someone/Doc"));
}

#[test]
fn a_restart_brings_back_a_tab_that_was_not_the_first_in_front() {
    let (first, mut before) = fixture();
    settle(&mut before);
    let home = before.open_tab(Location::local(first.home.clone()).unwrap(), true);
    assert_eq!(before.active_tab(), home);
    let view = before.view();
    assert_eq!(view.active, 1);

    let (_other, mut after) = fixture();
    assert!(after.restore_view(&view));
    assert_eq!(after.tabs().active_index(), 1);
    assert_eq!(
        after.location(),
        &Location::local(first.home.clone()).unwrap()
    );
}

#[test]
fn a_location_that_cannot_be_reopened_is_left_out() {
    let (_fixture, mut session) = fixture();
    let started = session.location().clone();
    let view = crate::restart::SessionView {
        tabs: vec!["device://some-disk/x".into(), "bogus".into()],
        ..Default::default()
    };
    assert!(!session.restore_view(&view), "nothing to reopen");
    assert_eq!(session.location(), &started, "the start folder is kept");
    assert_eq!(session.tabs().len(), 1);
}

#[test]
fn an_operation_or_an_open_dialog_holds_the_restart() {
    use better_ui::restart::Blocker;
    let (_fixture, mut session) = fixture();
    assert_eq!(session.restart_blocked(), None);
    session.jobs = vec![snapshot(OperationKind::Copy, JobState::Running)];
    assert_eq!(
        session.restart_blocked(),
        Some(Blocker::Task),
        "a running copy"
    );
    session.jobs = vec![snapshot(OperationKind::Copy, JobState::Paused)];
    assert_eq!(
        session.restart_blocked(),
        Some(Blocker::Task),
        "a paused copy is not finished"
    );
    session.jobs = vec![snapshot(OperationKind::Copy, JobState::Completed)];
    assert_eq!(session.restart_blocked(), None, "a finished one is history");
    session.dialog = Some(PendingDialog::NewFolder);
    assert_eq!(
        session.restart_blocked(),
        Some(Blocker::Dialog),
        "a dialog with typed input"
    );
}

// --- Ticket 119: the view follows the folder ------------------------------

/// Pumps until `done` holds or five seconds pass; whether it held.
fn pump_until(session: &mut FilesSession, done: impl Fn(&FilesSession) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        session.pump();
        if done(session) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Waits until every pane's watch is in place and the reading it asked
/// for is done.
fn settle_watches(session: &mut FilesSession) {
    let tabs = session.tabs().len();
    assert!(
        pump_until(session, |s| s.watched_folders().len() == tabs
            && !s.is_refreshing()
            && !s.is_listing()),
        "the watches never settled: watched {:?}, refreshing {}, listing {}, at {:?}",
        session.watched_folders(),
        session.is_refreshing(),
        session.is_listing(),
        session.location()
    );
}

/// Lets go of the kernel watches under `folder` (every window's), so what
/// a test sees next did not come from a watch.
fn unwatch(session: &mut FilesSession, folder: &std::path::Path) {
    settle_watches(session);
    files_platform::WatchHub::global().suspend_under(folder);
    assert!(
        pump_until(session, |s| s.watched_folders().is_empty()),
        "the watches were not let go of"
    );
}

fn visible_names(pane: &files_core::Pane) -> Vec<String> {
    pane.model()
        .iter_visible()
        .map(|entry| entry.name.clone())
        .collect()
}

fn shows(session: &FilesSession, name: &str) -> bool {
    visible_names(session.pane())
        .iter()
        .any(|shown| shown == name)
}

#[test]
fn a_change_made_by_another_program_shows_without_a_reload() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let documents = fixture.home.join("Documents");
    // The watch is asked for with the listing; its arrival reads once more.
    settle_watches(&mut session);
    assert_eq!(session.watched_folders(), vec![documents.clone()]);
    let readings = session.pane().readings();

    fs::write(documents.join("arrived.txt"), b"one").unwrap();
    assert!(
        pump_until(&mut session, |s| shows(s, "arrived.txt")),
        "created"
    );

    fs::write(documents.join("arrived.txt"), b"one two three").unwrap();
    assert!(
        pump_until(&mut session, |s| {
            s.pane()
                .model()
                .get(&files_core::EntryId::Name("arrived.txt".into()))
                .is_some_and(|entry| entry.size == files_core::EntrySize::Bytes(13))
        }),
        "modified"
    );

    fs::rename(documents.join("arrived.txt"), documents.join("renamed.txt")).unwrap();
    assert!(
        pump_until(&mut session, |s| shows(s, "renamed.txt")
            && !shows(s, "arrived.txt")),
        "renamed: {:?}",
        visible_names(session.pane())
    );

    fs::remove_file(documents.join("renamed.txt")).unwrap();
    assert!(
        pump_until(&mut session, |s| !shows(s, "renamed.txt")),
        "removed"
    );
    assert_eq!(
        session.pane().readings(),
        readings,
        "single changes are applied as rows, the folder is not read again"
    );
}

#[test]
fn the_selection_survives_changes_and_a_vanished_entry_leaves_it() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let documents = fixture.home.join("Documents");
    session.pump();
    let notes = session
        .pane()
        .model()
        .iter_visible()
        .position(|entry| entry.name == "notes.txt")
        .unwrap();
    session.apply_selection(SelectionInput::Click(notes), 1);
    fs::write(documents.join("aaa-first.txt"), b"x").unwrap();
    fs::write(documents.join("doomed.txt"), b"x").unwrap();
    assert!(pump_until(&mut session, |s| shows(s, "aaa-first.txt")
        && shows(s, "doomed.txt")));
    let notes_id = files_core::EntryId::Name("notes.txt".into());
    assert!(session.pane().model().selection().contains(&notes_id));

    let doomed = session
        .pane()
        .model()
        .iter_visible()
        .position(|entry| entry.name == "doomed.txt")
        .unwrap();
    session.apply_selection(SelectionInput::Click(doomed), 1);
    fs::remove_file(documents.join("doomed.txt")).unwrap();
    assert!(pump_until(&mut session, |s| !shows(s, "doomed.txt")));
    assert!(session.pane().model().selection().is_empty());
}

#[test]
fn a_job_better_files_runs_updates_every_pane_on_the_folder_at_once() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let documents = fixture.home.join("Documents");
    session.open_tab(Location::local(&documents).unwrap(), false);
    settle(&mut session);
    // No watch: the job's own result has to arrive without one.
    unwatch(&mut session, &documents);
    let trash = fixture.home.parent().unwrap().join("Trash");
    session.submit(files_operations::JobSpec::new(
        files_operations::Operation::Trash {
            sources: vec![LocalPath::new(documents.join("notes.txt")).unwrap()],
            trash_root: Some(trash),
        },
    ));
    let gone = |s: &FilesSession| {
        s.panes_for_tests()
            .all(|pane| !visible_names(pane).contains(&"notes.txt".to_string()))
    };
    assert!(
        pump_until(&mut session, gone),
        "both tabs drop the trashed file"
    );
    assert_eq!(session.panes_for_tests().count(), 2);
}

#[test]
fn undo_updates_the_folder_without_starting_its_listing_over() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let trash = fixture.home.parent().unwrap().join("Trash");
    session.set_undo_trash(trash);
    let documents = fixture.home.join("Documents");
    session.submit(files_operations::JobSpec::new(
        files_operations::Operation::CreateFolder {
            parent: LocalPath::new(&documents).unwrap(),
            name: "Made".into(),
        },
    ));
    assert!(pump_until(&mut session, |s| s.history.can_undo() && shows(s, "Made")));
    unwatch(&mut session, &documents);
    session.dispatch(Command::Undo, 1, 10);
    assert!(pump_until(&mut session, |s| !shows(s, "Made")));
}

#[test]
fn a_burst_of_ten_thousand_files_settles_without_reading_per_file() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    settle_watches(&mut session);
    let readings = session.pane().readings();
    let documents = fixture.home.join("Documents");
    for index in 0..10_000 {
        fs::write(documents.join(format!("burst-{index:05}.txt")), b"").unwrap();
    }
    // Every file shows, plus the three that were there.
    assert!(
        pump_until(&mut session, |s| s.pane().model().total_len() == 10_003
            && !s.pane().is_refreshing()),
        "{} rows",
        session.pane().model().total_len()
    );
    let read = session.pane().readings() - readings;
    assert!(read <= 4, "the folder was read {read} times for one burst");
}

#[test]
fn a_finished_job_is_shown_once_and_can_be_dismissed_or_cleared() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let documents = fixture.home.join("Documents");
    for name in ["One", "Two", "Three"] {
        session.submit(files_operations::JobSpec::new(
            files_operations::Operation::CreateFolder {
                parent: LocalPath::new(&documents).unwrap(),
                name: name.into(),
            },
        ));
    }
    assert!(pump_until(&mut session, |s| s.finished.len() == 3));
    assert!(
        session.job_rows().is_empty(),
        "no finished job is also a running card"
    );

    let first = session.finished.rows()[0].id;
    session.dismiss_finished(first);
    assert_eq!(session.finished.len(), 2);
    assert!(session.finished.rows().iter().all(|row| row.id != first));
    assert!(
        fixture.engine.snapshot(first).is_none(),
        "the engine let go of it too"
    );
    session.pump();
    assert_eq!(
        session.finished.len(),
        2,
        "a dismissed job does not come back"
    );

    session.clear_finished();
    assert!(session.finished.is_empty());
    session.pump();
    assert!(session.finished.is_empty());
}

#[test]
fn the_finished_list_and_the_bookkeeping_stay_bounded() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let documents = fixture.home.join("Documents");
    for index in 0..150 {
        session.submit(files_operations::JobSpec::new(
            files_operations::Operation::CreateFolder {
                parent: LocalPath::new(&documents).unwrap(),
                name: format!("folder-{index}").into(),
            },
        ));
        session.pump();
    }
    assert!(pump_until(&mut session, |s| s
        .jobs
        .iter()
        .all(|job| job.state.is_terminal())
        && documents.join("folder-149").is_dir()));
    session.pump();
    assert_eq!(session.finished.len(), opcenter::FINISHED_SHOWN);
    assert!(session.jobs.len() <= files_operations::FINISHED_KEPT);
    assert!(fixture.engine.jobs().len() <= files_operations::FINISHED_KEPT);
}

#[test]
fn copy_move_rename_and_delete_each_show_at_once_without_a_watch() {
    use files_operations::{DeleteConfirmation, DeleteTarget, JobSpec, Operation};
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let documents = fixture.home.join("Documents");
    let reports = documents.join("reports");
    // A background tab on the destination, so both ends are on screen.
    session.open_tab(Location::local(&reports).unwrap(), false);
    settle(&mut session);
    session.pump();
    unwatch(&mut session, &fixture.home);
    let local = |path: &std::path::Path| LocalPath::new(path).unwrap();
    let in_tab = |s: &FilesSession, folder: &std::path::Path, name: &str| {
        s.panes_for_tests().any(|pane| {
            pane.location() == &Location::local(folder).unwrap()
                && visible_names(pane).iter().any(|shown| shown == name)
        })
    };

    session.submit(JobSpec::new(Operation::Copy {
        sources: vec![local(&documents.join("notes.txt"))],
        destination: local(&reports),
    }));
    assert!(
        pump_until(&mut session, |s| in_tab(s, &reports, "notes.txt")),
        "copy"
    );

    session.submit(JobSpec::new(Operation::Rename {
        path: local(&documents.join("notes.txt")),
        new_name: "renamed.txt".into(),
    }));
    assert!(
        pump_until(&mut session, |s| in_tab(s, &documents, "renamed.txt")
            && !in_tab(s, &documents, "notes.txt")),
        "rename"
    );

    session.submit(JobSpec::new(Operation::Move {
        sources: vec![local(&documents.join("renamed.txt"))],
        destination: local(&reports),
    }));
    assert!(
        pump_until(&mut session, |s| in_tab(s, &reports, "renamed.txt")
            && !in_tab(s, &documents, "renamed.txt")),
        "move"
    );

    session.submit(JobSpec::new(Operation::PermanentDelete {
        targets: vec![DeleteTarget::Path(local(&reports.join("notes.txt")))],
        confirmation: DeleteConfirmation::explicit(),
    }));
    assert!(
        pump_until(&mut session, |s| !in_tab(s, &reports, "notes.txt")),
        "delete"
    );
}

/// A folder with `count` files, for readings that take a moment.
fn crowded(root: &std::path::Path, count: usize) -> std::path::PathBuf {
    let folder = root.join("crowded");
    fs::create_dir_all(&folder).unwrap();
    for index in 0..count {
        fs::write(folder.join(format!("item-{index:05}.txt")), b"").unwrap();
    }
    folder
}

/// A reader that reads the folder the moment it is asked and hands the
/// reading over only when released: a reading taken before a change and
/// finished after it.
struct HeldReader {
    release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

impl HeldReader {
    fn new() -> Self {
        Self {
            release: Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
        }
    }

    fn release(&self) {
        let (lock, signal) = &*self.release;
        *lock.lock().unwrap() = true;
        signal.notify_all();
    }
}

impl files_core::listing::DirectoryReader for HeldReader {
    fn start(
        &self,
        request: files_core::listing::ListingRequest,
        mut sink: files_core::listing::ListingSink,
    ) {
        let (mut taken, taken_sink) = files_core::listing::ListingSession::start(&request);
        files_platform::list_directory_blocking(&request, &ReaderConfig::new(), taken_sink);
        let entries: Vec<files_core::Entry> = taken
            .drain()
            .into_iter()
            .filter_map(|event| match event {
                files_core::listing::ListingEvent::Batch(batch) => Some(batch.entries),
                _ => None,
            })
            .flatten()
            .collect();
        let release = Arc::clone(&self.release);
        std::thread::spawn(move || {
            let (lock, signal) = &*release;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = signal.wait(released).unwrap();
            }
            for entry in entries {
                if sink.push(entry).is_err() {
                    return;
                }
            }
            let _ = sink.finish();
        });
    }
}

#[test]
fn a_change_during_a_re_reading_is_not_undone_by_it() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    settle_watches(&mut session);
    let documents = fixture.home.join("Documents");
    // A reading is taken now and handed over later.
    let held = HeldReader::new();
    session.pane_mut().refresh(&held);
    assert!(session.is_refreshing());
    // The folder changes after the reading was taken.
    fs::write(documents.join("late.txt"), b"x").unwrap();
    fs::remove_file(documents.join("notes.txt")).unwrap();
    // The watch reports both while the reading is outstanding.
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        session.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    held.release();
    assert!(
        pump_until(&mut session, |s| !s.is_refreshing()
            && shows(s, "late.txt")
            && !shows(s, "notes.txt")),
        "a stale reading undid the change: {:?}",
        visible_names(session.pane())
    );
}

#[test]
fn a_change_between_the_listing_and_the_watch_is_not_lost() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let folder = crowded(&fixture.home, 2_000);
    for round in 0..5 {
        session.navigate_to(Location::local(&fixture.home).unwrap());
        settle_watches(&mut session);
        // Changes made the moment the folder is opened, before any frame:
        // a file added, and one the listing may already have carried
        // removed.
        session.navigate_to(Location::local(&folder).unwrap());
        let added = format!("new-{round}.txt");
        let removed = format!("item-{:05}.txt", 1_000 + round);
        fs::write(folder.join(&added), b"x").unwrap();
        fs::remove_file(folder.join(&removed)).unwrap();
        settle_watches(&mut session);
        assert!(
            pump_until(&mut session, |s| shows(s, &added) && !shows(s, &removed)),
            "round {round}"
        );
    }
}

#[test]
fn rows_stay_on_screen_while_the_folder_is_read_again() {
    let (fixture, mut session) = fixture();
    let folder = crowded(&fixture.home, 3_000);
    session.navigate_to(Location::local(&folder).unwrap());
    settle_watches(&mut session);
    let reader = files_platform::LocalDirectoryReader::new();
    session.pane_mut().refresh(&reader);
    // The list never empties, which is what would send the view back to
    // its top: the scroll offset is kept because the rows are.
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.is_refreshing() && Instant::now() < deadline {
        session.pump();
        assert_eq!(session.pane().model().total_len(), 3_000);
    }
    assert!(!session.is_refreshing());
}

#[test]
fn a_folder_deleted_or_renamed_under_the_view_sends_it_to_the_folder_above() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    for rename in [false, true] {
        let folder = fixture.home.join("Documents/going");
        fs::create_dir_all(folder.join("inside")).unwrap();
        session.navigate_to(Location::local(&folder).unwrap());
        settle_watches(&mut session);
        if rename {
            fs::rename(&folder, fixture.home.join("Documents/gone-elsewhere")).unwrap();
        } else {
            fs::remove_dir_all(&folder).unwrap();
        }
        let documents = Location::local(fixture.home.join("Documents")).unwrap();
        assert!(
            pump_until(&mut session, |s| s.location() == &documents),
            "renamed: {rename}; at {:?}",
            session.location()
        );
        assert_eq!(
            session.notice.as_ref().map(|notice| notice.message(&EN_US)),
            Some(EN_US.folder_gone.replace("{folder}", "going"))
        );
        let _ = fs::remove_dir_all(fixture.home.join("Documents/gone-elsewhere"));
    }
}

#[test]
fn retry_of_a_job_the_engine_let_go_of_says_why() {
    let (fixture, mut session) = fixture();
    settle(&mut session);
    let documents = fixture.home.join("Documents");
    session.submit(files_operations::JobSpec::new(
        files_operations::Operation::CreateFolder {
            parent: LocalPath::new(&documents).unwrap(),
            name: "Once".into(),
        },
    ));
    assert!(pump_until(&mut session, |s| s.finished.len() == 1));
    let id = session.finished.rows()[0].id;
    assert!(fixture.engine.forget(id));
    session.retry_job(id);
    assert_eq!(
        session.notice.as_ref().map(|notice| notice.message(&EN_US)),
        Some(EN_US.retry_gone.to_string())
    );
}

#[test]
fn one_window_dismissing_a_job_leaves_it_for_another_that_has_not_filed_it() {
    let (fixture, mut first) = fixture();
    settle(&mut first);
    // A second window on the same engine, which has not pumped yet.
    let _second = fixture.engine.attach_window();
    let documents = fixture.home.join("Documents");
    first.submit(files_operations::JobSpec::new(
        files_operations::Operation::CreateFolder {
            parent: LocalPath::new(&documents).unwrap(),
            name: "Shared".into(),
        },
    ));
    assert!(pump_until(&mut first, |s| s.finished.len() == 1));
    let id = first.finished.rows()[0].id;
    first.clear_finished();
    assert!(first.finished.is_empty());
    assert!(
        fixture.engine.snapshot(id).is_some(),
        "the other window still has to file it"
    );
    fixture.engine.mark_recorded(id);
    assert!(fixture.engine.forget(id));
}
