//! A password leaves its buffer only through two readers, and only in the
//! modules the security review audited
//! (`engineering/specs/login-screen-security.md`, "The password"):
//! `Secret::expose_str` (the text) and `GreetdMessage::frame_bytes` (the
//! framed greetd message, which holds it). Anywhere else a call could hand
//! the password to a `String`, a log line or an error, and nothing would stop
//! it. This test reads every crate's source and fails on any other use; it
//! also holds the audited files to their known readers, so a wrapper added
//! there (a second `pub fn` handing out `&str` or `&[u8]`) fails too.
//!
//! The future callers are added in the change that adds them, with its
//! review: ticket 262's PAM conversation (`expose_str`), ticket 264's greetd
//! socket writer (`frame_bytes`).

use std::path::{Path, PathBuf};

/// Each reader, with the files (relative to `crates/`) that may name it.
const READERS: &[(&str, &[&str])] = &[
    (
        "expose_str",
        &[
            // The definition and its own tests.
            "login-screen-core/src/secret.rs",
            // Writes it straight into a `GreetdMessage`.
            "login-screen-core/src/greetd.rs",
            // Compares it with the marker it was filled from.
            "login-screen-core/tests/secret_wiped_on_drop.rs",
            "login-screen-core/tests/one_secret_reader.rs",
        ],
    ),
    (
        "frame_bytes",
        &[
            // The definition and its own tests.
            "login-screen-core/src/greetd.rs",
            "login-screen-core/tests/secret_wiped_on_drop.rs",
            "login-screen-core/tests/one_secret_reader.rs",
        ],
    ),
    (
        // The buffer type itself: only its own module and the message builder.
        "SecretBytes",
        &[
            "login-screen-core/src/secret.rs",
            "login-screen-core/src/greetd.rs",
            "login-screen-core/tests/one_secret_reader.rs",
        ],
    ),
];

/// Every function in the audited files that returns `&str` or `&[u8]`, by
/// file: anything else there that does is a new way out.
const BORROWS: &[(&str, &[&str])] = &[
    (
        "login-screen-core/src/secret.rs",
        // LockedPages::bytes, SecretBytes::as_bytes (crate-private, read by
        // Secret and GreetdMessage), SecretBytes::storage and
        // Secret::storage (tests), Secret::expose_str.
        // Secret::lock_warning returns a constant, never the content.
        &["bytes", "as_bytes", "storage", "expose_str", "lock_warning"],
    ),
    (
        "login-screen-core/src/greetd.rs",
        // GreetdMessage::frame_bytes, and a test's `payload`.
        &["frame_bytes", "payload"],
    ),
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn crates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf()
}

#[test]
fn the_password_is_read_only_where_it_was_audited() {
    let crates = crates();
    let mut files = Vec::new();
    sources(&crates, &mut files);
    assert!(
        files.len() > 100,
        "only {} source files were read",
        files.len()
    );
    let mut found = Vec::new();
    for file in files {
        let relative = file.strip_prefix(&crates).unwrap_or(&file);
        let relative = relative.to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (name, allowed) in READERS {
            if allowed.contains(&relative.as_str()) {
                continue;
            }
            for (number, line) in text.lines().enumerate() {
                if line.contains(name) {
                    found.push(format!("{relative}:{}: {}", number + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a password reader is used outside the audited modules:\n{}",
        found.join("\n")
    );
}

/// The name of the function a line declares, when it returns a borrowed
/// string or byte slice.
fn borrowing_function(line: &str) -> Option<&str> {
    let start = line.find("fn ")? + 3;
    let rest = &line[start..];
    let name_end = rest.find(['(', '<'])?;
    let returns = rest.split("->").nth(1)?;
    (returns.contains("&str") || returns.contains("&[u8]") || returns.contains("&'"))
        .then(|| rest[..name_end].trim())
}

#[test]
fn the_audited_files_have_no_other_reader() {
    let crates = crates();
    for (file, known) in BORROWS {
        let text = std::fs::read_to_string(crates.join(file)).unwrap();
        for (number, line) in text.lines().enumerate() {
            if let Some(name) = borrowing_function(line) {
                assert!(
                    known.contains(&name),
                    "{file}:{}: `{name}` hands out a borrow of the buffer; review it and list it here",
                    number + 1
                );
            }
        }
    }
}

#[test]
fn the_reader_check_sees_a_wrapper() {
    assert_eq!(
        borrowing_function("    pub fn text(&self) -> &str {"),
        Some("text")
    );
    assert_eq!(
        borrowing_function("    pub fn raw<'a>(&'a self) -> &'a [u8] {"),
        Some("raw")
    );
    assert_eq!(borrowing_function("    pub fn len(&self) -> usize {"), None);
}

#[test]
fn every_audited_file_exists() {
    let crates = crates();
    for (_, files) in READERS {
        for file in *files {
            assert!(crates.join(file).is_file(), "{file}");
        }
    }
    for (file, _) in BORROWS {
        assert!(crates.join(file).is_file(), "{file}");
    }
}
