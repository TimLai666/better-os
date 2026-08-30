//! Proof that the Better Files domain and platform crates never link GPUI.
//!
//! Issue #6's performance rules are about not doing work on the render
//! thread, and the structural guarantee behind them is that these two crates
//! have no way to reach one. A comment saying so is not a guarantee; this
//! walks the real dependency closure in `Cargo.lock` and fails if GPUI
//! appears anywhere in it, including transitively through a crate somebody
//! adds later.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

/// One package's name and the packages it depends on, as `Cargo.lock` records
/// them.
fn parse_lock(contents: &str) -> HashMap<String, Vec<String>> {
    let mut packages = HashMap::new();
    let mut name: Option<String> = None;
    let mut dependencies: Vec<String> = Vec::new();
    let mut in_dependencies = false;

    let mut flush = |name: &mut Option<String>, dependencies: &mut Vec<String>| {
        if let Some(name) = name.take() {
            packages.insert(name, std::mem::take(dependencies));
        }
    };

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            flush(&mut name, &mut dependencies);
            in_dependencies = false;
            continue;
        }
        if in_dependencies {
            if trimmed == "]" {
                in_dependencies = false;
                continue;
            }
            // Entries read `"name",` or `"name version",`.
            let entry = trimmed.trim_matches(|c| c == '"' || c == ',' || c == ' ');
            if let Some(package) = entry.split_whitespace().next()
                && !package.is_empty()
            {
                dependencies.push(package.to_string());
            }
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("name = ") {
            name = Some(value.trim_matches('"').to_string());
        } else if trimmed == "dependencies = [" {
            in_dependencies = true;
        }
    }
    flush(&mut name, &mut dependencies);
    packages
}

fn closure(packages: &HashMap<String, Vec<String>>, roots: &[&str]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut queue: Vec<String> = roots.iter().map(|root| root.to_string()).collect();
    while let Some(package) = queue.pop() {
        if !seen.insert(package.clone()) {
            continue;
        }
        if let Some(dependencies) = packages.get(&package) {
            queue.extend(dependencies.iter().cloned());
        }
    }
    seen
}

fn workspace_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `<root>/crates/files-core`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn neither_files_crate_can_reach_gpui() {
    let lock = workspace_root().join("Cargo.lock");
    let contents = std::fs::read_to_string(&lock)
        .unwrap_or_else(|error| panic!("reading {}: {error}", lock.display()));
    let packages = parse_lock(&contents);
    assert!(
        packages.contains_key("files-core") && packages.contains_key("files-platform"),
        "the lockfile does not list the crates under test; it may be stale"
    );

    let reachable = closure(&packages, &["files-core", "files-platform"]);
    let forbidden: Vec<&String> = reachable
        .iter()
        .filter(|package| package.contains("gpui"))
        .collect();
    assert!(
        forbidden.is_empty(),
        "files-core and files-platform must not depend on GPUI, found: {forbidden:?}"
    );
}

#[test]
fn the_platform_crate_reaches_the_shared_catalog_rather_than_a_second_parser() {
    let lock = workspace_root().join("Cargo.lock");
    let contents = std::fs::read_to_string(lock).expect("Cargo.lock");
    let packages = parse_lock(&contents);
    let reachable = closure(&packages, &["files-platform"]);
    assert!(
        reachable.contains("app-catalog-core"),
        "the Applications location must be backed by the shared catalog"
    );
    // The `.desktop` parser lives in `app-catalog-core`. If a second one ever
    // appeared it would be a new crate here; the guard that matters is the
    // source-level one below.
    let sources = workspace_root().join("crates/files-platform/src");
    for file in std::fs::read_dir(sources).expect("platform sources") {
        let file = file.expect("directory entry");
        let text = std::fs::read_to_string(file.path()).expect("source file");
        assert!(
            !text.contains("[Desktop Entry]"),
            "{} parses desktop entries; that belongs to app-catalog-core alone",
            file.path().display()
        );
    }
}
