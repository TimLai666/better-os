//! A measured smoke test for collection cost.
//!
//! The numbers are printed so `cargo test -- --nocapture` reproduces what the
//! ticket records. The assertions are deliberately loose bounds that a much
//! slower machine still passes: their job is to catch a collector that starts
//! costing seconds, not to pin a hardware-specific figure.

use monitor_collectors_linux::{ProcessPrivacy, Roots, measure};
use std::time::Duration;

/// A tree captured from a real desktop. The default tests measure against it,
/// so they never read the machine they run on.
fn recorded() -> Roots {
    Roots::at(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("snapshot-a"),
    )
}

const ROUNDS: u32 = 30;

/// A round that takes longer than this is not "low overhead" on any machine.
const ROUND_BUDGET: Duration = Duration::from_millis(500);

#[test]
fn sampling_every_collector_thirty_times_stays_within_a_sane_budget() {
    budget(&recorded());
}

/// The same budget against the machine running the test. Ignored by default
/// because it reads the real `/proc`, `/sys`, and socket table; run it with
/// `cargo test -p monitor-collectors-linux --test overhead -- --ignored
/// --nocapture` to reproduce the figures the tickets record.
#[test]
#[ignore = "reads the real machine; run explicitly to measure it"]
fn sampling_the_real_machine_thirty_times_stays_within_a_sane_budget() {
    budget(&Roots::system());
}

fn budget(roots: &Roots) {
    let report = measure(roots, ProcessPrivacy::default(), ROUNDS);

    println!("rounds:      {}", report.rounds);
    println!("total wall:  {:?}", report.total_wall);
    println!("mean round:  {:?}", report.mean_wall);
    println!("worst round: {:?}", report.worst_wall);
    match report.cpu_time {
        Some(cpu) => println!(
            "process cpu: {cpu:?} ({:.1}% of wall)",
            report.cpu_fraction().unwrap_or(0.0) * 100.0
        ),
        None => println!("process cpu: unavailable"),
    }
    for cost in &report.per_collector {
        println!(
            "  {:<16} mean {:?}, worst {:?}",
            cost.collector, cost.mean_wall, cost.worst_wall
        );
    }

    assert_eq!(report.rounds, ROUNDS);
    assert_eq!(
        report.per_collector.len(),
        monitor_collectors_linux::LinuxCollectors::collector_names().len()
    );
    assert!(
        report.mean_wall < ROUND_BUDGET,
        "a sampling round averaged {:?}, over the {ROUND_BUDGET:?} budget",
        report.mean_wall
    );
    assert!(
        report.total_wall > Duration::ZERO,
        "the measurement recorded no elapsed time at all"
    );
}

#[test]
fn every_collector_is_measured_separately() {
    // Attribution is the point of the per-collector breakdown: without it a
    // regression in one collector hides inside the total.
    let report = measure(&recorded(), ProcessPrivacy::default(), 5);
    let names: Vec<&str> = report
        .per_collector
        .iter()
        .map(|cost| cost.collector.as_str())
        .collect();
    assert_eq!(
        names,
        monitor_collectors_linux::LinuxCollectors::collector_names()
    );
}

#[test]
#[ignore = "reads the real machine; run explicitly to measure it"]
fn the_process_table_is_the_expensive_collector_on_the_real_machine() {
    let report = measure(&Roots::system(), ProcessPrivacy::default(), 5);
    let process = report
        .per_collector
        .iter()
        .find(|cost| cost.collector == "linux.process")
        .expect("the process collector");
    let pressure = report
        .per_collector
        .iter()
        .find(|cost| cost.collector == "linux.pressure")
        .expect("the pressure collector");
    assert!(
        process.total_wall >= pressure.total_wall,
        "scanning every process ({:?}) should not be cheaper than reading three PSI files ({:?})",
        process.total_wall,
        pressure.total_wall
    );
}

#[test]
fn measuring_against_a_fixture_costs_almost_nothing_and_still_reports_per_collector() {
    let roots = Roots::at(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("snapshot-a"),
    );
    let report = measure(&roots, ProcessPrivacy::default(), 3);
    assert_eq!(report.rounds, 3);
    assert!(
        report
            .per_collector
            .iter()
            .all(|cost| cost.total_wall > Duration::ZERO)
    );
}
