//! Ticket 146: the trash record a discard leaves has its `DeletionDate` on
//! the local wall clock, like every file manager's, so Better Files and the
//! desktop show the time it happened.
//!
//! The test sets `TZ` to a zone eight hours from UTC and `XDG_DATA_HOME` to a
//! scratch folder; a writer that went back to UTC would write a stamp eight
//! hours off and fail. It is the only test in this binary, so nothing else
//! reads the environment while it changes.

use std::fs;

use better_core::local_time::{CivilTime, TimeZone, reload_zone};

#[test]
fn a_discarded_copy_records_its_deletion_on_the_local_wall_clock() {
    let data = tempfile::tempdir().unwrap();
    // SAFETY: the only test in this binary, before any conversion.
    unsafe {
        std::env::set_var("TZ", "CST-8");
        std::env::set_var("XDG_DATA_HOME", data.path());
    }
    reload_zone();

    let source = data.path().join("notes.txt");
    fs::write(&source, b"contents").unwrap();
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    };
    let before = now();
    git_platform::patch::trash(&source, false).expect("the copy is trashed");
    let after = now();

    let info = data.path().join("Trash/info/notes.txt.trashinfo");
    let record = fs::read_to_string(&info).unwrap();
    let stamp = record
        .lines()
        .find_map(|line| line.strip_prefix("DeletionDate="))
        .expect("the record has a deletion date");
    let civil = CivilTime::parse_iso(stamp).expect("the stamp is a date and time");
    let local = TimeZone::Fixed(8 * 3_600).unix_at(civil);
    assert!(
        (before..=after).contains(&local),
        "{stamp} is not the local wall clock between {before} and {after}"
    );
    let as_utc = TimeZone::UTC.unix_at(civil);
    assert!(!(before..=after).contains(&as_utc));
}
