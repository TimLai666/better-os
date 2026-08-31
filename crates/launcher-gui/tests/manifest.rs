//! The manifest's benchmark definitions against the harness that takes them.
//!
//! `launcher-platform/tests/manifest.rs` checks that the manifest is a valid
//! Better OS manifest and that Issue #2's four benchmark names are present.
//! This file checks the thing that was actually wrong before ticket 37: the
//! workloads and metrics described a measurement nobody took. `BENCHMARKS` is
//! what `cargo bench -p launcher-gui --bench launcher_suite` labels its rows
//! from, so asserting the two are the same list is what keeps the manifest from
//! promising a number the harness does not produce.

use better_core::ComponentManifest;
use launcher_gui::BENCHMARKS;

const MANIFEST: &str = include_str!("../../../components/manifests/better-launcher.yaml");

#[test]
fn the_manifest_declares_exactly_the_benchmarks_the_harness_measures() {
    let manifest = ComponentManifest::parse_yaml(MANIFEST).expect("the launcher manifest is valid");
    let declared: Vec<(&str, &str, &str)> = manifest
        .benchmarks
        .iter()
        .map(|benchmark| {
            (
                benchmark.name.as_str(),
                benchmark.workload.as_str(),
                benchmark.metric.as_str(),
            )
        })
        .collect();
    assert_eq!(
        declared,
        BENCHMARKS.to_vec(),
        "the manifest and the benchmark suite disagree about what is measured"
    );
}

#[test]
fn issue_2s_four_launcher_benchmarks_are_all_still_named() {
    // The extra row is `idle-memory`: the harness reads both CPU and resident
    // memory over the idle window, and one metric string cannot carry two
    // numbers.
    let names: Vec<&str> = BENCHMARKS.iter().map(|(name, _, _)| *name).collect();
    for required in [
        "warm-search-update",
        "warm-overlay-open",
        "application-list-update",
        "idle-overhead",
    ] {
        assert!(
            names.contains(&required),
            "{required} is no longer measured"
        );
    }
    assert_eq!(names.len(), 5);
}
