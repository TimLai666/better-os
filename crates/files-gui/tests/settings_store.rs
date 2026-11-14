//! The per-folder view store: one default, an override per changed folder,
//! the last folder, nothing written into the folders themselves.

use std::path::Path;

use files_core::{SortDirection, SortKey, SortOrder};
use files_gui::prefs::{ItemScale, ViewMode};
use files_gui::settings_store::{FolderView, MAX_OVERRIDES, ViewState, ViewStateStore};

fn list_view() -> FolderView {
    FolderView::new(
        ViewMode::List,
        ItemScale::default(),
        false,
        SortOrder::default(),
    )
}

#[test]
fn a_folder_without_an_override_uses_the_default() {
    let state = ViewState::default();
    assert_eq!(state.view_for(Some(Path::new("/x"))), FolderView::default());
    assert_eq!(state.view_for(None), FolderView::default());
}

#[test]
fn a_change_is_kept_for_that_folder_only() {
    let mut state = ViewState::default();
    state.set_view(Some(Path::new("/photos")), list_view());
    assert_eq!(
        state.view_for(Some(Path::new("/photos"))).mode,
        ViewMode::List
    );
    assert_eq!(
        state.view_for(Some(Path::new("/music"))).mode,
        ViewMode::Grid
    );
    assert!(state.has_override(Path::new("/photos")));
}

#[test]
fn a_change_back_to_the_default_forgets_the_override() {
    let mut state = ViewState::default();
    state.set_view(Some(Path::new("/photos")), list_view());
    state.set_view(Some(Path::new("/photos")), FolderView::default());
    assert!(!state.has_override(Path::new("/photos")));
    assert_eq!(state.override_count(), 0);
}

#[test]
fn a_place_without_a_folder_changes_the_default() {
    let mut state = ViewState::default();
    state.set_view(None, list_view());
    assert_eq!(state.default_view().mode, ViewMode::List);
}

#[test]
fn applying_everywhere_sets_the_default_and_drops_every_override() {
    let mut state = ViewState::default();
    state.set_view(Some(Path::new("/a")), list_view());
    let big = FolderView::new(
        ViewMode::Grid,
        ItemScale::Large,
        false,
        SortOrder::default(),
    );
    state.apply_everywhere(big);
    assert_eq!(state.override_count(), 0);
    assert_eq!(
        state.view_for(Some(Path::new("/a"))).scale,
        ItemScale::Large
    );
}

#[test]
fn sort_and_hidden_round_trip_through_the_view() {
    let order =
        SortOrder::new(SortKey::Modified, SortDirection::Descending).with_folders_first(false);
    let view = FolderView::new(ViewMode::Grid, ItemScale::Small, true, order);
    assert_eq!(view.order(), order);
    assert!(view.view_preferences().hidden.show_hidden);
}

#[test]
fn overrides_for_folders_that_are_gone_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let kept = dir.path().join("kept");
    std::fs::create_dir(&kept).unwrap();
    let gone = dir.path().join("gone");
    let mut state = ViewState::default();
    state.set_view(Some(&kept), list_view());
    state.set_view(Some(&gone), list_view());
    state.set_last_folder(Some(&gone));
    assert_eq!(state.prune(|path| path.is_dir()), 1);
    assert!(state.has_override(&kept));
    assert!(!state.has_override(&gone));
    assert_eq!(state.last_folder(), None);
}

#[test]
fn the_file_round_trips_and_prunes_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("work");
    std::fs::create_dir(&folder).unwrap();
    let store = ViewStateStore::at_path(dir.path().join("state/views.json"));
    let mut state = ViewState::default();
    state.set_view(Some(&folder), list_view());
    state.set_view(Some(&dir.path().join("missing")), list_view());
    state.set_last_folder(Some(&folder));
    store.save(&state).unwrap();

    let loaded = store.load();
    assert_eq!(loaded.view_for(Some(&folder)).mode, ViewMode::List);
    assert_eq!(loaded.override_count(), 1);
    assert_eq!(loaded.last_folder(), Some(folder.as_path()));
    // Nothing is written into the folder itself.
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
}

#[test]
fn a_missing_or_corrupt_file_is_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let store = ViewStateStore::at_path(dir.path().join("views.json"));
    assert_eq!(store.load(), ViewState::default());
    std::fs::write(store.path(), "{ not json").unwrap();
    assert_eq!(store.load(), ViewState::default());
}

#[test]
fn the_file_never_grows_past_its_bound() {
    let mut state = ViewState::default();
    for index in 0..MAX_OVERRIDES + 5 {
        state.set_view(Some(Path::new(&format!("/f/{index}"))), list_view());
    }
    assert_eq!(state.override_count(), MAX_OVERRIDES);
    // The oldest went first.
    assert!(!state.has_override(Path::new("/f/0")));
    assert!(state.has_override(Path::new(&format!("/f/{}", MAX_OVERRIDES + 4))));
}

#[test]
fn the_store_sits_beside_another_file() {
    let store = ViewStateStore::beside(Path::new("/tmp/x/view.json"));
    assert_eq!(store.path(), Path::new("/tmp/x/views.json"));
}
