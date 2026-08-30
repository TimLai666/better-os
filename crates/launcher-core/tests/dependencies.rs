//! What `launcher-core` is allowed to depend on.
//!
//! Two of Issue #2's requirements are properties of the dependency graph, not
//! of any function: the search engine must be benchmarkable without starting
//! the GUI, and no code path may perform a network request. Both are easy to
//! assert once and easy to break by accident later, so they are asserted here
//! rather than trusted.
//!
//! The walk needs no network, no registry access, and no toolchain beyond the
//! one already running the test. It follows `[dependencies]` and
//! `[build-dependencies]` through the workspace's own manifests, then hands
//! each external package to the lockfile. That split matters: a lockfile
//! records development dependencies for workspace members, so walking it from
//! the top would charge `launcher-core` for the platform crate and the
//! temporary-directory crate that only its neighbour's benchmarks use. It
//! records no development dependencies for external packages, so the lockfile
//! is exactly right from that point down.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// Crates that would mean the launcher's ranking is entangled with a window
/// system or a render thread.
const FORBIDDEN_GUI: [&str; 5] = [
    "gpui",
    "gpui_platform",
    "gpui-component",
    "gpui-component-assets",
    "wayland-client",
];

/// Crates that would mean something in this graph can open a socket. The
/// launcher indexes and ranks locally; there is nothing for it to ask a
/// server.
const FORBIDDEN_NETWORK: [&str; 8] = [
    "reqwest",
    "hyper",
    "curl",
    "ureq",
    "tokio",
    "rustls",
    "native-tls",
    "zbus",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

/// Every crate directory the workspace declares as a member.
fn workspace_members(root: &Path) -> BTreeMap<String, PathBuf> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    let mut members = BTreeMap::new();
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with("members = [") {
            inside = true;
        } else if inside && line == "]" {
            break;
        } else if inside {
            let path = line.trim_end_matches(',').trim_matches('"');
            if let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) {
                members.insert(name.to_string(), root.join(path));
            }
        }
    }
    assert!(
        members.contains_key("launcher-core"),
        "launcher-core is not a workspace member"
    );
    members
}

/// The `[dependencies]` and `[build-dependencies]` a member manifest declares.
/// Development dependencies are deliberately not read: a benchmark's helper is
/// not something the shipped crate depends on.
fn manifest_dependencies(manifest_path: &Path) -> Vec<String> {
    let manifest = std::fs::read_to_string(manifest_path).expect("member manifest");
    let mut names = Vec::new();
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = matches!(line, "[dependencies]" | "[build-dependencies]");
            continue;
        }
        if !inside || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let key = line
            .split(['.', '='])
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches('"');
        if !key.is_empty() {
            names.push(key.to_string());
        }
    }
    names
}

/// Maps each locked package name to the names it depends on. Version suffixes
/// are dropped because this test only ever asks whether a name is present.
fn lock_graph(root: &Path) -> BTreeMap<String, Vec<String>> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).expect("the workspace lockfile");
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut package: Option<String> = None;
    let mut inside = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            package = None;
            inside = false;
        } else if let Some(value) = line.strip_prefix("name = ") {
            package = Some(value.trim_matches('"').to_string());
        } else if line.starts_with("dependencies = [") {
            inside = true;
        } else if inside && line == "]" {
            inside = false;
        } else if inside && let Some(owner) = &package {
            let entry = line.trim_end_matches(',').trim_matches('"');
            let dependency = entry.split_whitespace().next().unwrap_or(entry);
            if !dependency.is_empty() {
                graph
                    .entry(owner.clone())
                    .or_default()
                    .push(dependency.to_string());
            }
        }
    }
    graph
}

/// Everything `launcher-core` compiles against, transitively.
fn build_closure() -> BTreeSet<String> {
    let root = workspace_root();
    let members = workspace_members(&root);
    let lock = lock_graph(&root);

    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from(["launcher-core".to_string()]);
    while let Some(package) = queue.pop_front() {
        if !seen.insert(package.clone()) {
            continue;
        }
        let next = match members.get(&package) {
            // A workspace member: read its manifest, so its development
            // dependencies stay out of the answer.
            Some(directory) => manifest_dependencies(&directory.join("Cargo.toml")),
            // An external package: the lockfile already records only its
            // normal and build dependencies.
            None => lock.get(&package).cloned().unwrap_or_default(),
        };
        queue.extend(next);
    }
    seen
}

#[test]
fn the_dependency_graph_contains_no_gpui_crate() {
    let closure = build_closure();
    for forbidden in FORBIDDEN_GUI {
        assert!(
            !closure.contains(forbidden),
            "launcher-core depends on {forbidden}; the ranking seam has to stay benchmarkable \
             without a display backend"
        );
    }
    assert!(
        !closure.iter().any(|package| package.starts_with("gpui")),
        "launcher-core depends on a gpui crate: {closure:?}"
    );
}

#[test]
fn the_dependency_graph_contains_no_network_client() {
    let closure = build_closure();
    for forbidden in FORBIDDEN_NETWORK {
        assert!(
            !closure.contains(forbidden),
            "launcher-core depends on {forbidden}; indexing and ranking are local"
        );
    }
}

#[test]
fn the_graph_stays_small_enough_to_read() {
    // Not a style preference. This crate exists so ranking can be measured in
    // isolation, and a graph that quietly grows is how that stops being true.
    let closure = build_closure();
    assert!(
        closure.len() <= 12,
        "launcher-core's dependency closure grew to {}: {closure:?}",
        closure.len()
    );
}

#[test]
fn the_walk_actually_finds_edges() {
    // A walk that silently returned nothing would make every other assertion
    // in this file pass for the wrong reason.
    let closure = build_closure();
    assert!(
        closure.contains("app-catalog-core") && closure.contains("thiserror"),
        "the dependency walk lost real edges: {closure:?}"
    );
    assert!(
        !closure.contains("app-catalog-platform"),
        "the walk followed a development dependency: {closure:?}"
    );
}
