//! Where Better Manager's window is allowed to reach for a desktop default.
//!
//! Issue #10 requires that GPUI code never runs `gsettings`, `xdg-mime`, a
//! shell command, or a privileged operation, and that the GUI, the CLI, and
//! diagnostics share one `defaults-core` path. Both halves of that are checked
//! here rather than left to review: the shipped dependency list must not name
//! the adapter crate at all, and no source file may name one either.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The `[dependencies]` table of one workspace crate, as plain names. Only the
/// shipped table is read: a development dependency is never linked into the
/// binary and must not be charged to it.
fn shipped_dependencies(manifest: &Path) -> Vec<String> {
    let body = std::fs::read_to_string(manifest).expect("a readable manifest");
    let mut names = Vec::new();
    let mut inside = false;
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[dependencies]";
            continue;
        }
        if !inside || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            // `name.workspace = true` and `name = { … }` both name the crate
            // before the first dot.
            let name = key.trim().split('.').next().unwrap_or_default();
            names.push(name.to_string());
        }
    }
    names
}

#[test]
fn the_window_reaches_the_platform_adapters_only_through_defaults_core() {
    let dependencies = shipped_dependencies(&crate_root().join("Cargo.toml"));

    assert!(
        dependencies.iter().any(|name| name == "defaults-core"),
        "the Defaults screens must plan through defaults-core"
    );
    assert!(
        !dependencies.iter().any(|name| name == "defaults-platform"),
        "manager-gui must not depend on the adapter crate directly"
    );

    // And the path through defaults-core actually exists, so this is a
    // redirection rather than a missing capability.
    let core = crate_root()
        .parent()
        .expect("crates directory")
        .join("defaults-core/Cargo.toml");
    assert!(
        shipped_dependencies(&core)
            .iter()
            .any(|name| name == "defaults-platform"),
        "defaults-core is the crate that owns the adapters"
    );
}

#[test]
fn no_screen_names_a_setting_backend_or_runs_a_command() {
    let source = crate_root().join("src");
    let forbidden = [
        "defaults_platform",
        "gsettings",
        "xdg-mime",
        "std::process",
        "Command::new",
        "dconf",
    ];

    for file in std::fs::read_dir(&source).expect("the crate's sources") {
        let path = file.expect("a readable entry").path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        // Test modules are not compiled into the shipped binary, and they quote
        // the machine keys the adapters return.
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("_tests.rs") || name == "tests.rs")
        {
            continue;
        }
        let body = std::fs::read_to_string(&path).expect("a readable source file");
        for needle in forbidden {
            assert!(
                !body.contains(needle),
                "{} names {needle}; the GUI changes nothing itself",
                path.display()
            );
        }
    }
}
