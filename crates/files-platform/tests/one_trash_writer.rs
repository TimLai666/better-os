//! `files_platform::trash` is the one trash writer (ticket 173). Better Git
//! once wrote its own `.trashinfo`, with its own collision rule and no
//! per-volume trash; AGENTS.md says a rule two places need lives in one place
//! both call. This walks every other crate's non-test sources and fails when
//! one spells a trash record itself.

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

/// Whether a line has a string literal that writes part of a trash record.
fn spells_a_trash_record(line: &str) -> bool {
    line.split('"').skip(1).step_by(2).any(|literal| {
        literal.contains("[Trash Info]")
            || literal.contains(".trashinfo")
            || literal.contains("DeletionDate=")
    })
}

#[test]
fn no_crate_but_files_platform_writes_a_trash_record() {
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
        if member.file_name() == "files-platform" {
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
                if spells_a_trash_record(line) {
                    found.push(format!("{}:{number}: {}", file.display(), line.trim()));
                }
            }
        }
    }
    assert!(scanned > 100, "scanned only {scanned} files");
    assert!(
        found.is_empty(),
        "trash records are written by files_platform::trash alone \
         (put_in_trash, trash_path):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_check_sees_the_copy_it_was_written_for() {
    assert!(spells_a_trash_record(
        r#"        "[Trash Info]\nPath={}\nDeletionDate={}\n","#
    ));
    assert!(spells_a_trash_record(
        r#"        info_name.push(".trashinfo");"#
    ));
    assert!(!spells_a_trash_record(
        "    // a `.trashinfo` named in a comment"
    ));
}

/// Two functions copy an item into a trash, and only two (ticket 173's
/// review): `files_platform::trash`'s `copy_to_trash` (through
/// `copy_tree`), for a caller with no job, and `files-operations`'
/// `copy_into_trash`, which copies with the job engine so a large tree
/// reports progress, can be cancelled, keeps its hard links, and is verified
/// before the original goes, then trashes the staged copy through
/// `move_to_trash`. The job engine depends on this crate, so this crate
/// cannot call it; the record is written here either way.
const TRASH_COPIERS: &[&str] = &[
    "files-platform/src/trash.rs",
    "files-operations/src/exec.rs",
];

#[test]
fn only_the_two_known_functions_copy_into_a_trash() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf();
    let mut found = Vec::new();
    for member in std::fs::read_dir(&crates).unwrap().flatten() {
        let mut files = Vec::new();
        rust_files(&member.path().join("src"), &mut files);
        for file in files {
            let relative = file.strip_prefix(&crates).unwrap_or(&file).to_path_buf();
            if is_test_file(&file)
                || TRASH_COPIERS
                    .iter()
                    .any(|allowed| relative == Path::new(allowed))
            {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for (number, line) in production_lines(&text) {
                let Some(at) = line.find("fn ") else {
                    continue;
                };
                let name: String = line[at + 3..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if name.contains("copy") && name.contains("trash") {
                    found.push(format!("{}:{number}: {}", file.display(), line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a third copy-into-trash function:\n{}",
        found.join("\n")
    );
}
