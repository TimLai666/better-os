//! What the shipped launcher is allowed to depend on.
//!
//! Issue #2 requires that the launcher performs no network request.
//! `launcher-core` proves the strong form of that for the ranking seam: its
//! closure contains nothing that can open a socket at all. The binary cannot
//! make that claim, and this file says why rather than lowering the bar until
//! it passes.
//!
//! Two things are in the binary's graph that can reach a network. The session
//! bus stack, because single-instance activation needs it — zbus can speak
//! D-Bus over TCP even though this build only ever connects to the address the
//! desktop published. And an HTTP client, `zed-reqwest` with hyper and rustls
//! under it, which arrives through `gpui-component-assets` and is therefore in
//! every Better OS desktop binary, not only this one. No Better OS code calls
//! either of them over a network.
//!
//! So what is asserted here is the part that is both true and worth
//! protecting: no crate Better OS wrote or chose reaches an HTTP client, a TLS
//! stack, or a resolver, and the toolkit is the only reason one is linked at
//! all. If someone adds an HTTP client to a launcher crate, these fail.
//!
//! The walk is the one `launcher-core/tests/dependencies.rs` uses, copied
//! rather than shared because a test binary cannot import another crate's test
//! module. It reads workspace manifests for members, so a development
//! dependency is never charged to the shipped crate, and the lockfile from
//! there down. It over-approximates, because the lockfile records a package's
//! optional dependencies whether or not the enabled features pull them in.
//! Over-approximating is the right direction for a rule about what must not be
//! reachable.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// Crates that would mean something in the graph can reach a remote service.
/// The D-Bus stack is deliberately absent; see the module note.
const FORBIDDEN_NETWORK: [&str; 13] = [
    "reqwest",
    "zed-reqwest",
    "hyper",
    "curl",
    "ureq",
    "isahc",
    "rustls",
    "native-tls",
    "openssl",
    "trust-dns-resolver",
    "hickory-resolver",
    "tungstenite",
    "quinn",
];

/// The toolkit boundary. Everything past it is a dependency of the shared GPUI
/// component library, which every Better OS desktop binary already links.
const TOOLKIT: [&str; 4] = [
    "gpui",
    "gpui_platform",
    "gpui-component",
    "gpui-component-assets",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

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
        members.contains_key("launcher-gui"),
        "launcher-gui is not a workspace member"
    );
    members
}

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

/// Everything reachable from `from`, optionally stopping at a set of packages
/// whose own dependencies are not followed.
fn closure_stopping_at(from: &str, boundary: &[&str]) -> BTreeSet<String> {
    let root = workspace_root();
    let members = workspace_members(&root);
    let lock = lock_graph(&root);

    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([from.to_string()]);
    while let Some(package) = queue.pop_front() {
        if !seen.insert(package.clone()) {
            continue;
        }
        if boundary.contains(&package.as_str()) {
            continue;
        }
        let next = match members.get(&package) {
            Some(directory) => manifest_dependencies(&directory.join("Cargo.toml")),
            None => lock.get(&package).cloned().unwrap_or_default(),
        };
        queue.extend(next);
    }
    seen
}

fn build_closure(from: &str) -> BTreeSet<String> {
    closure_stopping_at(from, &[])
}

#[test]
fn no_launcher_crate_reaches_a_network_client_of_its_own() {
    // Everything the launcher is, minus the toolkit: the overlay's own crates,
    // the platform seam, the index, the shared catalog, and the session bus.
    let closure = closure_stopping_at("launcher-gui", &TOOLKIT);
    for forbidden in FORBIDDEN_NETWORK {
        assert!(
            !closure.contains(forbidden),
            "a launcher crate reaches {forbidden}; the launcher asks nothing of a remote service"
        );
    }
    assert!(
        closure.contains("launcher-platform")
            && closure.contains("launcher-core")
            && closure.contains("app-catalog-core"),
        "the dependency walk lost real edges: {closure:?}"
    );
}

#[test]
fn the_platform_half_reaches_no_network_client_at_all() {
    let closure = build_closure("launcher-platform");
    for forbidden in FORBIDDEN_NETWORK {
        assert!(
            !closure.contains(forbidden),
            "launcher-platform reaches {forbidden}"
        );
    }
}

#[test]
fn the_http_client_in_the_binary_comes_only_from_the_shared_toolkit() {
    // Recorded as a test so the exception stays one thing with one cause. If
    // this stops being true, either the toolkit changed or somebody added a
    // client, and both are worth a failing test.
    let whole = build_closure("launcher-gui");
    let without_toolkit = closure_stopping_at("launcher-gui", &TOOLKIT);
    assert!(
        whole.contains("hyper"),
        "the toolkit no longer links an HTTP client; this exception can be deleted"
    );
    assert!(
        !without_toolkit.contains("hyper"),
        "an HTTP client is now reachable without going through the toolkit"
    );
}

#[test]
fn the_only_socket_capable_dependency_the_launcher_chose_is_the_session_bus() {
    let closure = closure_stopping_at("launcher-gui", &TOOLKIT);
    assert!(
        closure.contains("zbus"),
        "the single-instance activation path needs the session bus: {closure:?}"
    );
}

#[test]
fn the_ranking_seam_stays_out_of_the_window_system_and_off_the_bus() {
    // The property `launcher-core` asserts about itself, checked from the
    // other side: adding the platform crate must not have given it an edge.
    let closure = build_closure("launcher-core");
    assert!(
        !closure.iter().any(|package| package.starts_with("gpui")),
        "launcher-core reached a gpui crate: {closure:?}"
    );
    assert!(
        !closure.contains("zbus") && !closure.contains("tokio"),
        "launcher-core reached the bus stack: {closure:?}"
    );
}

#[test]
fn the_platform_crate_never_reaches_the_window_system() {
    // `launcher-platform` is where the host lives, and it is deliberately
    // testable with no display backend. A GPUI edge here would end that.
    let closure = build_closure("launcher-platform");
    assert!(
        !closure.iter().any(|package| package.starts_with("gpui")),
        "launcher-platform reached a gpui crate: {closure:?}"
    );
}
