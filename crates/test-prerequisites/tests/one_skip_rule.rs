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

/// Whether a whole file is test code: under a `tests/` directory, or a test
/// module of its own (`tests.rs`, `first_call_tests.rs`).
fn is_test_file(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "tests")
        || path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem == "tests" || stem.ends_with("_tests"))
}

/// The words of a guard that probes the machine: a program, the user, a
/// locale, a kernel or filesystem feature. A test that returns right after
/// one is a skip, whether or not it says so.
const PROBES: &[&str] = &[
    "running_as_root",
    "geteuid",
    "getuid",
    "is_root",
    "Command::new(",
    "which",
    "locale",
    "xattr",
    "allocated_blocks",
    "/proc/",
    "/sys/",
    "installed",
    "available",
    "supported",
    "dbus-daemon",
    "this computer",
    "this machine",
];

/// Whether `word` appears in `text` as a word of its own (so `installed`
/// is found and `uninstalled` is not).
fn has_word(text: &str, word: &str) -> bool {
    let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_ascii_alphanumeric());
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        let starts_word = !word.starts_with(|c: char| c.is_ascii_alphanumeric());
        let ends_word = !word.ends_with(|c: char| c.is_ascii_alphanumeric());
        (starts_word || boundary(before)) && (ends_word || boundary(after))
    })
}

/// Whether a line leaves the function it is in.
fn is_return(line: &str) -> bool {
    let line = line.trim();
    matches!(line, "return;" | "return," | "return")
        || line.contains("=> return")
        || line.contains("else { return")
}

/// Whether a line starts a function, with its indentation.
fn function_start(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start().len();
    let mut rest = line.trim_start();
    for prefix in ["pub(crate) ", "pub(super) ", "pub ", "async ", "unsafe "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    rest.strip_prefix("fn ")
        .filter(|name| name.starts_with(|c: char| c.is_alphabetic() || c == '_'))
        .map(|_| indent)
}

/// The returns inside test functions whose guard probes the machine and does
/// not go through `test_prerequisites`, numbered from 1, with the guard.
fn silent_skips(text: &str, whole_file_is_test: bool) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let tests_from = if whole_file_is_test {
        0
    } else {
        lines
            .iter()
            .enumerate()
            .position(|(index, line)| {
                line.trim() == "#[cfg(test)]"
                    && lines
                        .get(index + 1)
                        .is_some_and(|next| next.trim_start().starts_with("mod "))
            })
            .unwrap_or(lines.len())
    };
    let mut found = Vec::new();
    let mut current: Option<(usize, usize, bool)> = None;
    for (index, line) in lines.iter().enumerate() {
        if let Some(indent) = function_start(line)
            && current.is_none_or(|(_, outer, _)| indent <= outer)
        {
            let attributes = lines[index.saturating_sub(4)..index].join(" ");
            let is_test = index >= tests_from
                && (attributes.contains("test]") || attributes.contains("::test"));
            current = Some((index, indent, is_test));
            continue;
        }
        let Some((start, _, true)) = current else {
            continue;
        };
        if line.trim_start().starts_with("//") || !is_return(line) {
            continue;
        }
        let guard = lines[(start + 1).max(index.saturating_sub(6))..=index].join("\n");
        if guard.contains("test_prerequisites") {
            continue;
        }
        if PROBES.iter().any(|probe| has_word(&guard, probe)) {
            found.push((index + 1, line.trim().to_string()));
        }
    }
    found
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
            for (number, guard) in silent_skips(&text, is_test_file(&file)) {
                found.push(format!(
                    "{}:{number}: returns from a test after a probe, saying nothing: {guard}",
                    file.display()
                ));
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

#[test]
fn the_check_sees_a_silent_skip_after_a_probe() {
    // The shapes ticket 177's review found: root, a program, no word said.
    let root =
        "#[test]\nfn a() {\n    if support::running_as_root() {\n        return;\n    }\n}\n";
    assert_eq!(silent_skips(root, true).len(), 1);
    let program = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn b() {\n        if Command::new(\"pdftoppm\").arg(\"-v\").output().is_err()\n        {\n            return;\n        }\n    }\n}\n";
    assert_eq!(silent_skips(program, false).len(), 1);
    let gvfs = "#[test]\nfn c() {\n    let Ok(trust) = Trust::system() else {\n        // No GVfs on this computer: nothing to find.\n        return;\n    };\n}\n";
    assert_eq!(silent_skips(gvfs, true).len(), 1);
    // Through the helper, outside a test, or after no probe: not a skip.
    let helper = "#[test]\nfn d() {\n    test_prerequisites::require_that!(\"a user other than root\", !running_as_root());\n}\n";
    assert!(silent_skips(helper, true).is_empty());
    let production = "fn e() {\n    if running_as_root() {\n        return;\n    }\n}\n";
    assert!(silent_skips(production, false).is_empty());
    let unrelated = "#[test]\nfn f() {\n    if let Some(root) = child_root() {\n        restore_after_the_previous_application_was_uninstalled(&root);\n        return;\n    }\n}\n";
    assert!(silent_skips(unrelated, true).is_empty());
}
