//! `better_core::host` is the one Rust reader of os-release (AGENTS.md: a
//! client and the daemon that resolve the host differently is the ticket 43
//! and 45 defect). This walks every crate's non-test sources and fails when
//! one names an os-release path itself. `install.sh` is shell, keeps its own
//! copy of the rule by design, and is not under `crates/`.

use std::path::{Path, PathBuf};

/// Files allowed to name the path outside better-core, with the reason. None
/// today; an entry here needs a reason a reviewer can check.
const EXCEPTIONS: &[&str] = &[];

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

/// Whether a source file is a test module of its own (`tests.rs`,
/// `drives_bus_tests.rs`).
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

/// Whether a line has a string literal that names os-release.
fn names_os_release(line: &str) -> bool {
    line.split('"')
        .skip(1)
        .step_by(2)
        .any(|literal| literal.contains("os-release"))
}

#[test]
fn no_crate_but_better_core_reads_os_release() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf();
    let mut found = Vec::new();
    let Ok(members) = std::fs::read_dir(&crates) else {
        panic!("cannot list {}", crates.display());
    };
    let mut scanned = 0;
    for member in members.flatten() {
        if member.file_name() == "better-core" {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&member.path().join("src"), &mut files);
        for file in files {
            if is_test_file(&file) {
                continue;
            }
            let relative = file.strip_prefix(&crates).unwrap_or(&file);
            if EXCEPTIONS
                .iter()
                .any(|allowed| relative == Path::new(allowed))
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            scanned += 1;
            for (number, line) in production_lines(&text) {
                if names_os_release(line) {
                    found.push(format!(
                        "crates/{}:{number}: {}",
                        relative.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(scanned > 100, "only {scanned} source files were scanned");
    assert!(
        found.is_empty(),
        "os-release is read only through better_core::host::read_os_release_under:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scan_sees_a_reader_and_skips_comments_and_test_modules() {
    let source = "// reads \"/etc/os-release\"\nlet a = read(\"/etc/os-release\");\n\
                  #[cfg(test)]\nfn helper() { read(\"os-release\") }\n\
                  #[cfg(test)]\nmod tests { const X: &str = \"etc/os-release\"; }\n";
    let hits: Vec<usize> = production_lines(source)
        .into_iter()
        .filter(|(_, line)| names_os_release(line))
        .map(|(number, _)| number)
        .collect();
    // The comment is skipped, the reader and the test-only helper outside
    // the module are seen, the module is not.
    assert_eq!(hits, [2, 4]);
    assert!(is_test_file(Path::new("src/drives_bus_tests.rs")));
    assert!(!is_test_file(Path::new("src/host.rs")));
}
