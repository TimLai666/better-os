//! `test_prerequisites` is the one place a test decides it cannot run here
//! (ticket 177): a skip on a developer's machine, a failure under CI. This
//! walks every crate's sources and fails when one prints a skip of its own or
//! reads the `CI` variable itself, which is how the tests that quietly passed
//! on CI without running were written before it.

use std::path::{Path, PathBuf};

fn rust_files(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// The string literals a line opens, roughly: what follows every other
/// double quote. Good enough for messages, which is what this looks for.
fn literals(line: &str) -> impl Iterator<Item = &str> {
    line.split('"').skip(1).step_by(2)
}

/// Why a line is a skip of its own, if it is one.
fn own_skip(line: &str) -> Option<&'static str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return None;
    }
    for literal in literals(line) {
        let lower = literal.to_ascii_lowercase();
        if lower.starts_with("skipping:") || lower.starts_with("skipped:") {
            return Some("prints a skip message of its own");
        }
        if lower.contains("not exercised") {
            return Some("says a check was not exercised and passes");
        }
    }
    let reads_ci = ["var(\"CI\")", "var_os(\"CI\")"]
        .iter()
        .any(|read| line.contains(read));
    reads_ci.then_some("reads the CI variable itself")
}

#[test]
fn no_test_skips_or_reads_ci_but_through_test_prerequisites() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf();
    let mut found = Vec::new();
    let mut scanned = 0;
    for member in std::fs::read_dir(&crates)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", crates.display()))
        .flatten()
    {
        if member.file_name() == "test-prerequisites" {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&member.path(), &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            scanned += 1;
            for (number, line) in text.lines().enumerate() {
                if let Some(why) = own_skip(line) {
                    found.push(format!(
                        "{}:{}: {why}: {}",
                        file.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        scanned > 100,
        "scanned only {scanned} files under {}",
        crates.display()
    );
    assert!(
        found.is_empty(),
        "a test that cannot run here goes through test_prerequisites::require! or \
         require_that!, which fails under CI:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_check_sees_the_shapes_it_was_written_for() {
    assert!(own_skip(r#"        eprintln!("skipping: dbus-daemon is not available");"#).is_some());
    assert!(
        own_skip(
            r#"                "skipping: gjs is not installed, so extension.js was not parsed. \"#
        )
        .is_some()
    );
    assert!(own_skip(r#"eprintln!("skipped: no dbus-daemon on this machine");"#).is_some());
    assert!(
        own_skip(
            r#"eprintln!("this filesystem refuses user extended attributes; not exercised");"#
        )
        .is_some()
    );
    assert!(own_skip(r#"assert!(std::env::var_os("CI").is_none(), "no locale on CI");"#).is_some());
    assert!(own_skip(r#"    // eprintln!("skipping: a comment is not a skip");"#).is_none());
    assert!(own_skip(r#"fn skipping(component_id: &str) -> PlanEntry {"#).is_none());
    assert!(
        own_skip(r#"format!("skip: the previous application is no longer installed")"#).is_none()
    );
    assert!(own_skip(r#"#[serde(tag = "skipped", rename_all = "snake_case")]"#).is_none());
}
