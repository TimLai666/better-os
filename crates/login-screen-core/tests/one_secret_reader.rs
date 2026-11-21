//! A password leaves its `Secret` only through `Secret::expose_str`, and only
//! in the modules the security review audited
//! (`engineering/specs/login-screen-security.md`, "The password"): the greetd
//! message builder, and the `Secret` itself. Anywhere else a call could hand
//! the text to a `String`, a log line or an error, and nothing would stop it.
//! This test reads every crate's source and fails on any other use. A new
//! audited reader (ticket 262's PAM conversation) is added to `AUDITED` in the
//! change that adds it, with the review of that module.

use std::path::{Path, PathBuf};

/// The files, relative to `crates/`, that may call it.
const AUDITED: &[&str] = &[
    // The definition and its own tests.
    "login-screen-core/src/secret.rs",
    // Writes it straight into a `GreetdMessage`.
    "login-screen-core/src/greetd.rs",
    // Compares it with the marker it was filled from.
    "login-screen-core/tests/secret_wiped_on_drop.rs",
    // This file.
    "login-screen-core/tests/one_secret_reader.rs",
];

const NAME: &str = "expose_str";

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

#[test]
fn the_password_is_read_only_where_it_was_audited() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();
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
        if AUDITED.contains(&relative.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (number, line) in text.lines().enumerate() {
            if line.contains(NAME) {
                found.push(format!("{relative}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "Secret::expose_str is called outside the audited modules:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_audited_file_exists() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for file in AUDITED {
        assert!(crates.join(file).is_file(), "{file}");
    }
}
