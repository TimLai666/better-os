//! Issue #6: "no shell-string concatenation for file or application
//! launching".
//!
//! The rule is easy to keep on the day it is written and easy to lose two
//! tickets later, when someone reaches for `tar` or `rm -rf` because it is
//! three lines instead of thirty. This test is the tripwire: nothing in the
//! operation path may spawn a process at all, so there is no path by which a
//! filename could ever reach a shell.
//!
//! It scans the source rather than the behaviour deliberately. A behavioural
//! test would only catch a shell invocation on a path the test happens to
//! take; this catches one anywhere in the crate.

use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file in this crate and in the trash write side it owns.
fn sources() -> Vec<PathBuf> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for directory in [
        crate_root.join("src"),
        crate_root.join("benches"),
        crate_root.parent().unwrap().join("files-platform/src"),
    ] {
        collect(&directory, &mut files);
    }
    assert!(files.len() > 10, "the scan found almost nothing: {files:?}");
    files
}

/// The lines that are code, with their numbers. A doc comment explaining that
/// this crate never runs a command must not be mistaken for one that does.
fn code_lines(text: &str) -> Vec<(usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line))
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .collect()
}

fn collect(directory: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            into.push(path);
        }
    }
}

#[test]
fn no_file_operation_ever_spawns_a_process() {
    // The primitives, not the strings. A shell path written in a doc comment
    // or a test fixture is inert; what would make it dangerous is a way to
    // execute it, and there is none in this crate.
    let forbidden = [
        "process::Command",
        "Command::new",
        "libc::system",
        "libc::popen",
        "libc::execv",
        "libc::execl",
        "libc::fork",
        "libc::posix_spawn",
    ];
    let mut offences = Vec::new();
    for path in sources() {
        let text = fs::read_to_string(&path).expect("a readable source file");
        for (number, line) in code_lines(&text) {
            for needle in forbidden {
                if line.contains(needle) {
                    offences.push(format!("{}:{number} contains {needle}", path.display()));
                }
            }
        }
    }
    assert!(
        offences.is_empty(),
        "the operation path must not reach a shell:\n{}",
        offences.join("\n")
    );
}

#[test]
fn the_job_specification_has_nowhere_to_put_a_command() {
    // Not a scan: a structural fact. Every operation is built from typed paths
    // and names, and the only free-form `String` anywhere in a spec is a bulk
    // rename template, which is expanded into a filename and never executed.
    let text =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/spec.rs")).unwrap();
    for (number, line) in code_lines(&text) {
        assert!(
            !line.contains("Command"),
            "src/spec.rs:{number} mentions a command"
        );
    }
    // `Operation` is a closed enum, so a future variant carrying a command
    // line would have to be added here, in front of this test.
    assert!(text.contains("pub enum Operation {"));
}
