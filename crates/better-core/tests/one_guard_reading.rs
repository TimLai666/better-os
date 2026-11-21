//! The test guard is read in one place (ticket 178).
//!
//! `BETTER_OS_TEST_SYSTEMCTL_GUARD` used to be read as a log path in three
//! places and as a flag in three, and two of the flag readers took an empty
//! value as on while the rest took it as off. `better_core::test_guard` is
//! now the only reader of it and of `BETTER_OS_TEST_GUARD_LOG`; this fails
//! when any other Rust file in the workspace reads either one from the
//! environment, and names the file and line.

use std::path::{Path, PathBuf};

/// The one file allowed to read the variables.
const READER: &str = "crates/better-core/src/test_guard.rs";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace holds this crate")
        .to_path_buf()
}

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// Every environment read in `text` whose argument names a guard variable,
/// by its line number. A read is `var(` or `var_os(` (`std::env::var_os`,
/// `env::var`), and its argument runs to the closing parenthesis, which may
/// be on a later line.
fn guard_reads(text: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    for call in ["var_os(", "var("] {
        let mut from = 0;
        while let Some(found) = text[from..].find(call) {
            let start = from + found;
            from = start + call.len();
            // `env_var(` or `some_var(` is not a read of the environment.
            let before = text[..start].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let argument = &text[from..];
            let argument = &argument[..argument.find(')').unwrap_or(argument.len())];
            if argument.contains("GUARD") {
                lines.push(text[..start].lines().count().max(1));
            }
        }
    }
    lines
}

#[test]
fn only_the_test_guard_module_reads_the_guard_variables() {
    let root = workspace();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(
        files.iter().any(|file| file.ends_with(READER)),
        "the search did not reach {READER}"
    );
    let mut readers = Vec::new();
    for file in &files {
        if file.ends_with(READER) || file.ends_with("tests/one_guard_reading.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in guard_reads(&text) {
            let relative = file.strip_prefix(&root).unwrap_or(file);
            readers.push(format!("{}:{line}", relative.display()));
        }
    }
    assert!(
        readers.is_empty(),
        "read the test guard through better_core::test_guard (is_on or \
         TestGuard::from_environment), not the environment directly: {readers:?}"
    );
}

#[test]
fn the_search_finds_every_shape_of_a_direct_read() {
    let text = "fn a() {\n    if std::env::var_os(manager_platform::user_units::SYSTEMCTL_GUARD_VARIABLE).is_none() {}\n    let _ = env::var(\n        \"BETTER_OS_TEST_GUARD_LOG\",\n    );\n    let _ = std::env::var_os(\"HOME\");\n    let _ = some_var(GUARD);\n}\n";
    assert_eq!(guard_reads(text), vec![2, 3]);
}
