//! No Better Awake test may reach the developer's own files.
//!
//! `AwakeEngine::start` keeps its rules and history under `XDG_STATE_HOME` and
//! reads this machine's `/proc` and `/sys`, which is right for the service and
//! wrong for a test: a test that calls it appends to the person's real History
//! and runs against the rules they wrote. Tests build an engine with
//! `AwakeEngine::start_in` or `AwakeEngine::start_with_rules` over a temporary
//! directory instead. This reads the test code of both crates that build an
//! engine and fails on any call that would reach the real paths.

use std::path::{Path, PathBuf};

/// Calls that resolve a real per-user or per-machine path.
const FORBIDDEN: &[&str] = &[
    "AwakeEngine::start(",
    "from_default_path(",
    "Roots::system(",
];

/// The test code in one source file: all of a `tests/` file, and whatever
/// follows `#[cfg(test)]` in a `src/` file.
fn test_code(path: &Path, in_tests_directory: bool) -> String {
    let source = std::fs::read_to_string(path).unwrap();
    if in_tests_directory {
        return source;
    }
    match source.find("#[cfg(test)]") {
        Some(start) => source[start..].to_string(),
        None => String::new(),
    }
}

fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return files;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}

#[test]
fn no_awake_test_reaches_the_real_state_directory_or_this_machine() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut offences = Vec::new();

    for krate in ["awake-service", "awake-tray"] {
        for (directory, in_tests_directory) in [("tests", true), ("src", false)] {
            for path in rust_files(&crates.join(krate).join(directory)) {
                if path.file_name().is_some_and(|name| name == "hermetic.rs") {
                    continue;
                }
                let code = test_code(&path, in_tests_directory);
                for forbidden in FORBIDDEN {
                    if code.contains(forbidden) {
                        offences.push(format!("{}: {forbidden}", path.display()));
                    }
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "tests must keep every file in a temporary directory:\n{}",
        offences.join("\n")
    );
}
