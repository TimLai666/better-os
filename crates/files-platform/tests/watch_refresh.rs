//! The watcher and the list model together: a change on disk updates one row,
//! and does not re-read the directory.
//!
//! This is the acceptance criterion "file watching produces incremental
//! refreshes rather than full re-listings" tested end to end. The proof that
//! it is incremental is that the directory is read exactly once — the reader
//! is never started again — while three separate changes each land in the
//! model.

use std::fs;
use std::time::{Duration, Instant};

use files_core::listing::{DirectoryReader, ListingRequest, ListingSession};
use files_core::{
    DirectoryModel, EntryId, EntryKind, HiddenPreference, ListingStatus, Location, RefreshEvent,
    SortOrder,
};
use files_platform::{DirectoryWatcher, LocalDirectoryReader, read_hidden_rules, refresh_for};

/// Loads a directory once and returns the finished model.
fn load(path: &std::path::Path) -> DirectoryModel {
    let reader = LocalDirectoryReader::new();
    let request = ListingRequest::new(Location::local(path).unwrap());
    let (mut session, sink) = ListingSession::start(&request);
    let mut model = DirectoryModel::new(
        Location::local(path).unwrap(),
        SortOrder::default(),
        HiddenPreference::default(),
    );
    model.restart(request.listing);
    reader.start(request, sink);
    let started = Instant::now();
    while !matches!(model.status(), ListingStatus::Complete)
        && started.elapsed() < Duration::from_secs(30)
    {
        for event in session.drain() {
            model.apply(event);
        }
    }
    model
}

/// Applies every refresh the watcher can produce within the deadline.
fn drain_into(
    model: &mut DirectoryModel,
    watcher: &DirectoryWatcher,
    directory: &files_core::LocalPath,
    deadline: Duration,
    until: impl Fn(&DirectoryModel) -> bool,
) {
    let rules = read_hidden_rules(directory.as_path());
    let started = Instant::now();
    while started.elapsed() < deadline {
        for event in watcher.poll() {
            if let Some(refresh) = refresh_for(directory, &rules, &event) {
                model.apply_refresh(refresh);
            }
        }
        if until(model) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_created_file_appears_as_one_new_row_without_relisting() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("existing.txt"), b"a").unwrap();

    let mut model = load(root.path());
    assert_eq!(model.total_len(), 1);

    let watcher = DirectoryWatcher::new(root.path()).unwrap();
    let directory = files_core::LocalPath::new(root.path()).unwrap();
    fs::write(root.path().join("added.txt"), b"bb").unwrap();

    drain_into(
        &mut model,
        &watcher,
        &directory,
        Duration::from_secs(10),
        |model| model.total_len() == 2,
    );

    assert_eq!(model.total_len(), 2, "the new file never reached the model");
    let added = model
        .get(&EntryId::Name("added.txt".to_string()))
        .expect("the added entry");
    assert_eq!(added.kind, EntryKind::File);
    assert_eq!(added.size, files_core::EntrySize::Bytes(2));
    // Still in sort order: the row was merged, not appended.
    let names: Vec<&str> = model
        .iter_visible()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, ["added.txt", "existing.txt"]);
}

#[test]
fn a_deleted_file_disappears_and_takes_its_selection_with_it() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("keep.txt"), b"a").unwrap();
    fs::write(root.path().join("remove.txt"), b"b").unwrap();

    let mut model = load(root.path());
    assert_eq!(model.total_len(), 2);
    let doomed = EntryId::Name("remove.txt".to_string());
    model.selection_mut().select_only(doomed.clone());

    let watcher = DirectoryWatcher::new(root.path()).unwrap();
    let directory = files_core::LocalPath::new(root.path()).unwrap();
    fs::remove_file(root.path().join("remove.txt")).unwrap();

    drain_into(
        &mut model,
        &watcher,
        &directory,
        Duration::from_secs(10),
        |model| model.total_len() == 1,
    );

    assert_eq!(model.total_len(), 1);
    assert!(model.get(&doomed).is_none());
    assert!(
        !model.selection().contains(&doomed),
        "a deleted entry must not stay selected"
    );
}

#[test]
fn a_grown_file_updates_its_size_in_place() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("growing.log");
    fs::write(&path, b"a").unwrap();

    let mut model = load(root.path());
    assert_eq!(
        model
            .get(&EntryId::Name("growing.log".to_string()))
            .map(|entry| entry.size),
        Some(files_core::EntrySize::Bytes(1))
    );

    let watcher = DirectoryWatcher::new(root.path()).unwrap();
    let directory = files_core::LocalPath::new(root.path()).unwrap();
    fs::write(&path, b"aaaaaaaaaa").unwrap();

    drain_into(
        &mut model,
        &watcher,
        &directory,
        Duration::from_secs(10),
        |model| {
            model
                .get(&EntryId::Name("growing.log".to_string()))
                .map(|entry| entry.size)
                == Some(files_core::EntrySize::Bytes(10))
        },
    );

    assert_eq!(
        model.total_len(),
        1,
        "a modification must not duplicate a row"
    );
    assert_eq!(
        model
            .get(&EntryId::Name("growing.log".to_string()))
            .map(|entry| entry.size),
        Some(files_core::EntrySize::Bytes(10))
    );
}

#[test]
fn a_created_dotfile_is_hidden_on_arrival_and_revealing_it_needs_no_reload() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("visible.txt"), b"a").unwrap();

    let mut model = load(root.path());
    let watcher = DirectoryWatcher::new(root.path()).unwrap();
    let directory = files_core::LocalPath::new(root.path()).unwrap();
    fs::write(root.path().join(".appeared"), b"b").unwrap();

    drain_into(
        &mut model,
        &watcher,
        &directory,
        Duration::from_secs(10),
        |model| model.total_len() == 2,
    );

    assert_eq!(model.total_len(), 2);
    assert_eq!(
        model.visible_len(),
        1,
        "a dotfile must arrive already marked hidden"
    );
    model.set_hidden_preference(HiddenPreference::showing_hidden());
    assert_eq!(model.visible_len(), 2);
}

#[test]
fn a_lost_event_queue_asks_for_a_reload_rather_than_leaving_a_wrong_list() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.txt"), b"a").unwrap();
    let mut model = load(root.path());
    // `Resynchronize` deliberately changes nothing on its own: the consumer
    // has to start a new listing, which is the only correct response to
    // "events were lost".
    assert!(!model.apply_refresh(RefreshEvent::Resynchronize));
    assert_eq!(model.total_len(), 1);
}
