//! `app_catalog_core::DesktopEnvironments::shows` is the one desktop-entry
//! `OnlyShowIn`/`NotShowIn` rule (ticket 173). Better Monitor's autostart
//! list once matched the two keys itself; AGENTS.md says a rule two places
//! need lives in one place both call. This walks every other crate's non-test
//! sources and fails when one names either key in a string, which is what
//! reading it takes.

use std::path::{Path, PathBuf};

fn rust_files(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Whether a source file is a test module of its own.
fn is_test_file(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "tests" || stem.ends_with("_tests"))
}

/// The lines of a source file before its `#[cfg(test)] mod …`, with comments
/// left out, numbered from 1.
fn production_lines(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == "#[cfg(test)]"
            && lines
                .get(index + 1)
                .is_some_and(|next| next.trim_start().starts_with("mod "))
        {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        out.push((index + 1, *line));
    }
    out
}

/// Whether a line has a string literal that names one of the two keys.
fn names_a_show_in_key(line: &str) -> bool {
    line.split('"')
        .skip(1)
        .step_by(2)
        .any(|literal| literal.contains("OnlyShowIn") || literal.contains("NotShowIn"))
}

#[test]
fn no_crate_but_the_catalog_reads_only_show_in_or_not_show_in() {
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
        if member.file_name() == "app-catalog-core" {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&member.path().join("src"), &mut files);
        for file in files {
            if is_test_file(&file) {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            scanned += 1;
            for (number, line) in production_lines(&text) {
                if names_a_show_in_key(line) {
                    found.push(format!("{}:{number}: {}", file.display(), line.trim()));
                }
            }
        }
    }
    assert!(scanned > 100, "scanned only {scanned} files");
    assert!(
        found.is_empty(),
        "OnlyShowIn and NotShowIn are matched by app_catalog_core::DesktopEnvironments \
         (shows, shows_entry) alone:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_check_sees_the_copy_it_was_written_for() {
    assert!(names_a_show_in_key(
        r#"        let only = list("OnlyShowIn");"#
    ));
    assert!(names_a_show_in_key(
        r#"        let not = list("NotShowIn");"#
    ));
    assert!(!names_a_show_in_key("    // `OnlyShowIn` in a comment"));
}
