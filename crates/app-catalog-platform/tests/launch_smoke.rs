//! A real launch, against a real fixture entry, recording what the started
//! process actually received.
//!
//! `RecordingSpawner` proves what the catalog *plans*. This proves what the
//! kernel *delivers*: the launched program writes its own `argv` out, and the
//! test reads it back. A file name containing shell metacharacters has to
//! arrive as one untouched argument, which it cannot do if anything along the
//! way built a command string.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use app_catalog_core::{
    ApplicationRecord, DesktopFile, DesktopId, EntryScope, LaunchTarget, NoProbe,
};
use app_catalog_platform::{LaunchOutcome, Launcher, SystemSpawner};

/// Writes a program that records each argument it received on its own line.
fn write_recorder(path: &Path, output: &Path) {
    let script = format!(
        "#!/bin/sh\n: > '{output}'\nfor argument in \"$@\"; do\n  printf '%s\\n' \"$argument\" >> '{output}'\ndone\n",
        output = output.display()
    );
    fs::write(path, script).expect("recorder script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("recorder permissions");
}

fn wait_for(path: &Path, expected_lines: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(contents) = fs::read_to_string(path) {
            let lines: Vec<String> = contents.lines().map(str::to_string).collect();
            if lines.len() >= expected_lines {
                return lines;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the launched process never recorded {expected_lines} arguments"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_launched_process_receives_the_exact_argument_vector() {
    let root = tempfile::tempdir().expect("temporary root");
    let recorder = root.path().join("recorder");
    let output = root.path().join("argv.txt");
    write_recorder(&recorder, &output);

    let entry_path = root.path().join("recorder.desktop");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Recorder\nExec={} --open %F\n",
        recorder.display()
    );
    fs::write(&entry_path, &entry).expect("fixture entry");

    let file = DesktopFile::parse(&entry).expect("parsed entry");
    let record = ApplicationRecord::from_desktop_file(
        DesktopId::new("recorder.desktop").unwrap(),
        entry_path,
        EntryScope::User,
        &file,
        &NoProbe,
    )
    .expect("record");

    // Every one of these would be mangled or acted on by a shell.
    let hostile = root
        .path()
        .join("a b; touch pwned $(id) `id` 'q' \"r\".txt");
    fs::write(&hostile, b"data").expect("hostile file");
    let targets = vec![
        LaunchTarget::path(hostile.clone()).expect("hostile target"),
        LaunchTarget::path(root.path().join("second file.txt")).expect("second target"),
    ];

    let spawner = SystemSpawner;
    let launcher = Launcher::new(&spawner);
    let outcome = launcher
        .launch(&record, None, &targets, None)
        .expect("launch");
    assert_eq!(outcome, LaunchOutcome::Started { processes: 1 });

    let arguments = wait_for(&output, 3);
    assert_eq!(
        arguments,
        vec![
            "--open".to_string(),
            hostile.display().to_string(),
            root.path().join("second file.txt").display().to_string(),
        ]
    );
    assert!(
        !root.path().join("pwned").exists(),
        "a shell interpreted the launch target"
    );
}

#[test]
fn a_single_file_entry_really_starts_one_process_per_file() {
    let root = tempfile::tempdir().expect("temporary root");
    let recorder = root.path().join("recorder");
    let output = root.path().join("argv.txt");
    write_recorder(&recorder, &output);

    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Recorder\nExec={} %f\n",
        recorder.display()
    );
    let file = DesktopFile::parse(&entry).expect("parsed entry");
    let record = ApplicationRecord::from_desktop_file(
        DesktopId::new("recorder.desktop").unwrap(),
        root.path().join("recorder.desktop"),
        EntryScope::User,
        &file,
        &NoProbe,
    )
    .expect("record");

    let targets = vec![
        LaunchTarget::path(root.path().join("one.txt")).expect("target"),
        LaunchTarget::path(root.path().join("two.txt")).expect("target"),
    ];
    let spawner = SystemSpawner;
    let outcome = Launcher::new(&spawner)
        .launch(&record, None, &targets, None)
        .expect("launch");
    assert_eq!(outcome, LaunchOutcome::Started { processes: 2 });
    // Each process truncates the file and writes its single argument, so the
    // recorded file always ends up with exactly one line.
    let recorded = wait_for(&output, 1);
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0].ends_with(".txt"));
}
