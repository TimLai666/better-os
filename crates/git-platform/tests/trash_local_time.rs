//! Ticket 146: the trash record a discard leaves has its `DeletionDate` on
//! the local wall clock, like every file manager's, so Better Files and the
//! desktop show the time it happened. Since ticket 173 the record is written
//! by Better Files' trash writer; this checks the discard still reaches it.
//!
//! The test sets `TZ` to a zone eight hours from UTC and `XDG_DATA_HOME` to a
//! scratch folder; a writer that went back to UTC would write a stamp eight
//! hours off and fail. It is the only test in this binary, so nothing else
//! reads the environment while it changes.

use std::fs;
use std::path::Path;
use std::process::Command;

use better_core::local_time::{CivilTime, TimeZone, reload_zone};
use git_platform::{FileSelection, Git, SystemGit};

fn git(repository: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

#[test]
fn a_discarded_copy_records_its_deletion_on_the_local_wall_clock() {
    let data = tempfile::tempdir().unwrap();
    let repository = data.path().join("repo");
    fs::create_dir_all(&repository).unwrap();
    // SAFETY: the only test in this binary, before any conversion.
    unsafe {
        std::env::set_var("TZ", "CST-8");
        std::env::set_var("XDG_DATA_HOME", data.path());
        std::env::set_var("HOME", data.path());
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        for key in ["GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"] {
            std::env::set_var(key, "Better Git Test");
        }
        for key in ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"] {
            std::env::set_var(key, "test@example.invalid");
        }
    }
    reload_zone();

    git(&repository, &["init", "-q", "-b", "main"]);
    fs::write(repository.join("notes.txt"), b"committed\n").unwrap();
    git(&repository, &["add", "notes.txt"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    fs::write(repository.join("notes.txt"), b"edited\n").unwrap();

    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    };
    let before = now();
    SystemGit
        .discard_files(&repository, &[FileSelection::whole("notes.txt")])
        .expect("the change is discarded");
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
    assert_eq!(
        fs::read(data.path().join("Trash/files/notes.txt")).unwrap(),
        b"edited\n"
    );
}
