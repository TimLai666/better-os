//! Ticket 146: a `.trashinfo` `DeletionDate` is written on the local wall
//! clock, the way the trash specification and every file manager write it.
//!
//! The test sets `TZ` to a zone eight hours from UTC, so a writer that went
//! back to UTC would write a stamp eight hours off and fail. It is the only
//! test in this binary, so nothing else reads the environment while it
//! changes.

use std::fs;

use better_core::local_time::{TimeZone, reload_zone};
use files_platform::trash::{TrashDirectory, move_to_trash};

#[test]
fn a_trashed_file_records_its_deletion_on_the_local_wall_clock() {
    // SAFETY: the only test in this binary, before any conversion.
    unsafe { std::env::set_var("TZ", "CST-8") };
    reload_zone();

    let root = tempfile::tempdir().unwrap();
    let trash = TrashDirectory::new(root.path().join("Trash"));
    let source = root.path().join("report.txt");
    fs::write(&source, b"contents").unwrap();

    let item = move_to_trash(&trash, &source).expect("the file is trashed");
    let record = fs::read_to_string(&item.info_path).unwrap();
    let stamp = record
        .lines()
        .find_map(|line| line.strip_prefix("DeletionDate="))
        .expect("the record has a deletion date");

    let local = TimeZone::Fixed(8 * 3_600)
        .at(item.deleted_at_seconds)
        .civil
        .iso_text();
    let utc = TimeZone::UTC.at(item.deleted_at_seconds).civil.iso_text();
    assert_eq!(stamp, local);
    assert_ne!(stamp, utc);
}
