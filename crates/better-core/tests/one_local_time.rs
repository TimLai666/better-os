//! Ticket 146: one place turns a timestamp into local time.
//!
//! `better_core::local_time` is that place. This test reads every Rust file in
//! the workspace and fails on a second conversion: a TZif reader of its own,
//! a call into the C library's local-time functions, a copy of the civil-date
//! arithmetic, or one of the helpers the windows, the tray, and the command
//! line used to carry.

use std::fs;
use std::path::{Path, PathBuf};

/// What a second conversion looks like, and why it is refused.
const FORBIDDEN: &[(&str, &str)] = &[
    ("UtcOffset", "the old per-window offset type"),
    ("/etc/localtime", "a zone file reader of its own"),
    ("localtime_r", "a C library conversion of its own"),
    ("mktime", "a C library conversion of its own"),
    ("tm_gmtoff", "a C library offset read of its own"),
    ("fn tzset", "a C library zone reset of its own"),
    ("fn civil_from_days", "a copy of the civil-date arithmetic"),
    ("fn days_from_civil", "a copy of the civil-date arithmetic"),
    ("719_468", "a copy of the civil-date arithmetic"),
    ("fn utc_time", "the command line's old UTC-only helper"),
    ("fn clock_time", "a clock formatter of its own"),
    ("fn calendar_time", "a calendar formatter of its own"),
];

/// The one implementation, and this file, which has to name what it forbids.
const ALLOWED: &[&str] = &[
    "crates/better-core/src/local_time.rs",
    "crates/better-core/tests/one_local_time.rs",
];

fn rust_files(directory: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            into.push(path);
        }
    }
}

#[test]
fn no_crate_converts_local_time_on_its_own() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("better-core sits two levels under the workspace root");
    let mut files = Vec::new();
    rust_files(&workspace.join("crates"), &mut files);
    rust_files(&workspace.join("adapters"), &mut files);
    assert!(
        files.len() > 100,
        "the walk found {} files; it is not reading the workspace",
        files.len()
    );

    let mut found = Vec::new();
    for file in &files {
        let relative = file
            .strip_prefix(workspace)
            .expect("every file is under the workspace")
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWED.contains(&relative.as_str()) {
            continue;
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            for (pattern, why) in FORBIDDEN {
                if line.contains(pattern) {
                    found.push(format!("{relative}:{}: `{pattern}` ({why})", number + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "use better_core::local_time instead:\n{}",
        found.join("\n")
    );
}
