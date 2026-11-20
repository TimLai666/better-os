//! The configuration watch the gesture service and both windows run on
//! (ticket 131).
//!
//! A child process writes every watched file, so this process never holds a
//! write descriptor that a forking test thread could carry into a late
//! close-write (AGENTS.md).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use touchpad_gestures::{ConfigurationWatch, WatchState};

/// Runs a shell line with `$1` set to `path`, as a child.
fn child(script: &str, path: &Path) {
    let status = std::process::Command::new("/bin/sh")
        .args(["-c", script, "sh"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success(), "{script}");
}

/// Writes `path` the way the store does: beside it, then renamed over it.
fn child_writes(path: &Path) {
    child(
        "printf x > \"$1.partial\" && mv \"$1.partial\" \"$1\"",
        path,
    );
}

enum Heard {
    Change,
    State(WatchState),
}

fn watch(files: &[PathBuf], retry: Duration) -> (ConfigurationWatch, mpsc::Receiver<Heard>) {
    let (sender, receiver) = mpsc::channel();
    let states = sender.clone();
    let watch = ConfigurationWatch::start(
        files,
        retry,
        move || {
            let _ = sender.send(Heard::Change);
        },
        move |state| {
            let _ = states.send(Heard::State(state));
        },
    );
    (watch, receiver)
}

/// Waits for the watch to say it is watching, then for things to go quiet.
fn until_watching(heard: &mpsc::Receiver<Heard>) {
    loop {
        match heard.recv_timeout(Duration::from_secs(5)) {
            Ok(Heard::State(WatchState::Watching)) => break,
            Ok(_) => continue,
            Err(error) => panic!("never watching: {error}"),
        }
    }
    quiet(heard);
}

fn quiet(heard: &mpsc::Receiver<Heard>) {
    while heard.recv_timeout(Duration::from_millis(300)).is_ok() {}
}

fn next_change(heard: &mpsc::Receiver<Heard>) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        match heard.recv_timeout(left) {
            Ok(Heard::Change) => return true,
            Ok(Heard::State(_)) => continue,
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn a_write_to_a_watched_file_is_heard_and_one_to_any_other_file_is_not() {
    let root = tempfile::tempdir().unwrap();
    let gestures = root.path().join("gestures.json");
    let safe_mode = root.path().join("safe-mode");
    let (_watch, heard) = watch(
        &[gestures.clone(), safe_mode.clone()],
        Duration::from_secs(60),
    );
    until_watching(&heard);

    child_writes(&gestures);
    assert!(next_change(&heard));
    quiet(&heard);
    child_writes(&safe_mode);
    assert!(next_change(&heard));
    quiet(&heard);

    child_writes(&root.path().join("config.json"));
    assert!(
        heard.recv_timeout(Duration::from_millis(500)).is_err(),
        "a write to another file was heard"
    );
}

/// Review of ticket 131: the kernel drops a watch on a deleted directory, so
/// a directory deleted and made again is watched again and the owner is told
/// to read, rather than every later change being ignored with nothing said.
#[test]
fn a_directory_deleted_and_made_again_is_watched_again() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("touchpad");
    let gestures = directory.join("gestures.json");
    let (_watch, heard) = watch(std::slice::from_ref(&gestures), Duration::from_secs(60));
    until_watching(&heard);

    child("rm -rf \"$1\"", &directory);
    // The re-made watch asks for a read at once.
    assert!(next_change(&heard), "the deletion was not followed");
    quiet(&heard);

    child("mkdir -p \"$1\"", &directory);
    child_writes(&gestures);
    assert!(next_change(&heard), "a change after re-creation was missed");
}

/// Review of ticket 131: a watch that cannot be made says why and is tried
/// again on the timer, and the first one that works asks for a read.
#[test]
fn a_watch_that_cannot_be_made_says_why_and_is_tried_again() {
    let root = tempfile::tempdir().unwrap();
    // A file where the directory should be: the watch cannot be made.
    let blocked = root.path().join("touchpad");
    child("printf x > \"$1\"", &blocked);
    let gestures = blocked.join("gestures.json");
    let (_watch, heard) = watch(&[gestures], Duration::from_millis(200));

    match heard.recv_timeout(Duration::from_secs(5)) {
        Ok(Heard::State(WatchState::Unavailable(reason))) => {
            assert!(reason.contains("touchpad"), "{reason}");
        }
        _ => panic!("the failure was not said"),
    }
    child("rm -f \"$1\"", &blocked);
    until_watching(&heard);
}
