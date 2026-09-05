//! `better-manager catalog status`, end to end, against a disposable cache.
//!
//! No test here reaches the network. Refresh behaviour is decided in
//! `manager-core` and tested there against an injected fetcher; what these
//! tests prove is that the shipped binary reads the same cache the refresh
//! writes, plans from it, and never presents a stale catalog as a current one.

use std::path::Path;
use std::process::Command;

/// A manifest at a version far above anything the built-in catalog carries, so
/// a cache built from it is unmistakably not the compiled-in one.
fn manifest(id: &str, version: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 2,
        "id": id,
        "display_name": id,
        "component_type": "enhancement",
        "version": version,
        "targets": {
            "distributions": ["ubuntu"],
            "releases": ["24.04"],
            "architectures": ["amd64"],
        },
        "artifacts": [{
            "release": "24.04",
            "architecture": "amd64",
            "url": format!("https://example.com/{id}_{version}_ubuntu-24.04_amd64.deb"),
            "sha256": "a".repeat(64),
            "release_asset": format!("{id}_{version}_ubuntu-24.04_amd64.deb"),
        }],
        "lifecycle": {
            "install": "plan-install",
            "enable": "plan-enable",
            "disable": "plan-disable",
            "remove": "plan-remove",
            "rollback": "plan-rollback",
        },
    })
}

fn write_cache(path: &Path, fetched_at: u64) {
    let cache = serde_json::json!({
        "schema_version": 1,
        "source_url": "https://fixture.invalid/manifests",
        "fetched_at_unix_seconds": fetched_at,
        "manifests": [manifest("better-monitor", "42.0.0")],
    });
    std::fs::write(path, serde_json::to_vec_pretty(&cache).unwrap()).unwrap();
}

fn run(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .args(["--execution", "mock"])
        .args([
            "--state-path",
            &directory.join("state.json").display().to_string(),
        ])
        .args([
            "--catalog-path",
            &directory.join("catalog.json").display().to_string(),
        ])
        .args(arguments)
        .output()
        .expect("the manager binary runs");
    assert!(
        output.status.success(),
        "{:?} failed: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn temporary(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("better-os-catalog-cli-{name}-"))
        .tempdir()
        .unwrap()
}

#[test]
fn with_no_cache_the_catalog_reports_the_built_in_one_as_never_refreshed() {
    let directory = temporary("built-in");
    let output = run(directory.path(), &["catalog", "status"]);

    assert!(output.contains("catalog source: built-in"), "{output}");
    assert!(output.contains("catalog origin: compiled in"), "{output}");
    assert!(output.contains("catalog fetched: never"), "{output}");
    assert!(
        output.contains("may be outdated (catalog.degraded.never_refreshed)"),
        "{output}"
    );
}

#[test]
fn a_cached_catalog_is_reported_with_its_source_and_fetch_time() {
    let directory = temporary("cached");
    write_cache(&directory.path().join("catalog.json"), 1_700_000_000);
    let output = run(directory.path(), &["catalog", "status"]);

    assert!(output.contains("catalog source: cache"), "{output}");
    assert!(
        output.contains("catalog origin: https://fixture.invalid/manifests"),
        "{output}"
    );
    assert!(output.contains("catalog fetched: 1700000000"), "{output}");
}

#[test]
fn the_cached_catalog_is_what_the_manager_lists_and_plans_from() {
    let directory = temporary("plans");
    write_cache(&directory.path().join("catalog.json"), 1_700_000_000);

    // The cache holds one component at 42.0.0 and nothing else. If the binary
    // were still planning from the compiled-in catalog, six other components
    // would be listed and this version would not appear anywhere.
    let listed = run(directory.path(), &["list"]);
    assert_eq!(listed.lines().count(), 1, "{listed}");
    assert!(listed.contains("better-monitor"), "{listed}");

    let plan = run(directory.path(), &["plan", "better-monitor", "install"]);
    assert!(plan.contains("42.0.0"), "{plan}");
}

#[test]
fn an_unusable_cache_falls_back_to_the_built_in_catalog_instead_of_failing() {
    let directory = temporary("unusable");
    let path = directory.path().join("catalog.json");
    std::fs::write(&path, b"{not json").unwrap();

    let output = run(directory.path(), &["catalog", "status"]);
    assert!(output.contains("catalog source: built-in"), "{output}");
    assert!(
        output.contains("may be outdated (catalog.degraded.never_refreshed)"),
        "{output}"
    );
    // The file a person might want to look at is still there.
    assert!(path.exists());
}
