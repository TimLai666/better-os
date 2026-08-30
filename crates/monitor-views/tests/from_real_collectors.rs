//! End to end from a captured `/proc` tree to the screens.
//!
//! The unit tests build `ProcessFacts` by hand, which proves the view logic
//! but not that the views and the collectors agree on metric names. These
//! tests run the production collectors over ticket 22's captured fixture
//! trees and assert on what the Apps, Processes, and Overview models make of
//! the result — so a renamed metric fails here rather than blanking a column
//! on someone's machine.

use monitor_collectors_linux::{LinuxCollectors, ProcessPrivacy, Roots};
use monitor_core::{CollectorReport, Timestamp};
use monitor_views::apps::AppsModel;
use monitor_views::facts::ProcessFacts;
use monitor_views::format::{Cell, NonValue, cell};
use monitor_views::grouping::{AppKind, GroupingEvidence, GroupingPrecedence};
use monitor_views::overview::{OverviewModel, ResourceVerdict, ThrottlingState};
use monitor_views::process_table::{ProcessColumn, ProcessTableModel, SortDirection};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("monitor-collectors-linux")
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn at(milliseconds: u64) -> Timestamp {
    Timestamp {
        unix_ms: 1_700_000_000_000 + milliseconds,
        monotonic_ns: milliseconds * 1_000_000,
    }
}

/// Two rounds over the two captured snapshots, which is what every rate needs.
fn two_rounds(first: &str, second: &str) -> Vec<CollectorReport> {
    let a = Roots::at(fixture(first));
    let b = Roots::at(fixture(second));
    let mut collectors = LinuxCollectors::new(a.clone(), ProcessPrivacy::default());
    let _ = collectors.sample(&a, at(0));
    collectors.sample(&b, at(1_000))
}

fn processes(reports: &[CollectorReport]) -> Vec<ProcessFacts> {
    reports
        .iter()
        .find(|report| report.collector.as_str() == "linux.process")
        .map(ProcessFacts::from_report)
        .unwrap_or_default()
}

#[test]
fn the_process_table_reads_every_column_the_collectors_produce() {
    let reports = two_rounds("snapshot-a", "snapshot-b");
    let facts = processes(&reports);
    assert!(!facts.is_empty(), "the snapshot carries processes");

    let bash = facts
        .iter()
        .find(|process| process.pid == 45609)
        .expect("the captured shell");
    assert_eq!(bash.display_name(), "bash");
    assert_eq!(bash.user.value().map(String::as_str), Some("fixture"));
    assert!(bash.cpu_utilization.is_value(), "two rounds produce a rate");
    assert!(bash.memory_resident.is_value());
    assert!(bash.threads.is_value());
    assert!(bash.cgroup.is_value());
    assert!(bash.read_rate.is_value(), "two rounds produce throughput");

    // The command line is withheld by default, and the cell says so rather
    // than showing an empty string.
    assert_eq!(
        cell(&bash.command_line, |value| value.clone()),
        Cell::Missing(NonValue::PolicyWithheld {
            policy: "command lines are not collected unless explicitly enabled".into()
        })
    );
}

#[test]
fn the_captured_desktop_scope_becomes_a_named_application() {
    let reports = two_rounds("snapshot-a", "snapshot-b");
    let model = AppsModel::new(processes(&reports), GroupingPrecedence::default());

    let claude = model
        .applications()
        .iter()
        .find(|row| row.group.display_name == "Claude")
        .expect("the captured application scope");
    assert_eq!(claude.group.kind, AppKind::UserApplication);
    assert!(matches!(
        claude.group.evidence,
        GroupingEvidence::SystemdUnit { .. }
    ));
    assert!(claude.cpu_utilization.is_complete());

    // `init.scope` is not something a person launched.
    assert!(
        model
            .services()
            .iter()
            .any(|row| row.group.display_name == "init"),
        "pid 1 belongs with the background services"
    );
}

#[test]
fn sorting_and_filtering_work_over_a_real_round() {
    let reports = two_rounds("snapshot-a", "snapshot-b");
    let mut model = ProcessTableModel::new(processes(&reports));
    model.set_sort(ProcessColumn::Pid, SortDirection::Ascending);
    let first = model.process_at(0).expect("a first row").pid;
    model.set_sort(ProcessColumn::Pid, SortDirection::Descending);
    let last = model.process_at(0).expect("a first row").pid;
    assert!(first < last);

    model.set_filter("bash");
    assert!(model.visible_rows().iter().all(|row| {
        model
            .row(row.index)
            .unwrap()
            .display_name()
            .contains("bash")
    }));
}

#[test]
fn the_overview_reads_the_captured_machine_rather_than_a_mock() {
    let reports = two_rounds("snapshot-a", "snapshot-b");
    let model = OverviewModel::from_reports(&reports);

    assert!(model.cpu.utilization.is_value(), "{:?}", model.cpu);
    assert!(model.memory.total.is_value());
    assert!(model.memory.available.is_value());
    assert!(model.logical_cpus.is_value());
    assert!(model.process_count.is_value());
    assert!(model.cpu.pressure_some.is_value(), "the snapshot has PSI");
    assert!(model.storage.read.counted > 0);
    assert!(model.network.read.counted > 0);
    assert!(model.coverage.value > 0);
    assert!(
        model.coverage.observed_fraction().unwrap() > 0.5,
        "most of a captured machine is observable"
    );
    // Six collectors, all reporting.
    assert_eq!(model.collectors.len(), 6);
    assert!(model.unhealthy_collectors().is_empty());
}

#[test]
fn a_kernel_without_psi_is_visibly_unsupported_and_not_silently_calm() {
    let roots = Roots::at(fixture("no-psi"));
    let mut collectors = LinuxCollectors::new(roots.clone(), ProcessPrivacy::default());
    let reports = collectors.sample(&roots, at(0));
    let model = OverviewModel::from_reports(&reports);

    assert!(
        matches!(
            model.cpu.pressure_some,
            monitor_views::Field::Unsupported(_)
        ),
        "{:?}",
        model.cpu.pressure_some
    );
    let unhealthy = model.unhealthy_collectors();
    assert!(
        unhealthy
            .iter()
            .any(|status| status.collector == "linux.pressure"),
        "the pressure collector must appear in observation health"
    );
    assert!(unhealthy[0].detail().is_some());
}

#[test]
fn a_host_with_no_proc_at_all_explains_itself_rather_than_showing_zeros() {
    let roots = Roots::at(fixture("does-not-exist"));
    let mut collectors = LinuxCollectors::new(roots.clone(), ProcessPrivacy::default());
    let reports = collectors.sample(&roots, at(0));
    let model = OverviewModel::from_reports(&reports);

    assert!(processes(&reports).is_empty());
    for verdict in [
        &model.cpu.verdict,
        &model.memory.resource.verdict,
        &model.io.verdict,
    ] {
        assert!(
            matches!(
                verdict,
                ResourceVerdict::CollectorFailed { .. }
                    | ResourceVerdict::Unsupported { .. }
                    | ResourceVerdict::Unobserved { .. }
            ),
            "an unobservable host must not read as nominal: {verdict:?}"
        );
    }
    assert!(matches!(
        model.throttling,
        ThrottlingState::NotObservable { .. }
    ));
    assert_eq!(model.unhealthy_collectors().len(), 6);
}

#[test]
fn a_command_line_appears_only_when_collection_was_told_to_gather_it() {
    let roots = Roots::at(fixture("snapshot-a"));
    let mut collectors = LinuxCollectors::new(
        roots.clone(),
        ProcessPrivacy {
            include_command_line: true,
        },
    );
    let reports = collectors.sample(&roots, at(0));
    let facts = processes(&reports);
    let with_command = facts
        .iter()
        .find(|process| process.command_line.is_value())
        .expect("at least one process has a command line");

    let mut model = ProcessTableModel::new(facts.clone());
    assert!(!model.columns().contains(&ProcessColumn::CommandLine));
    model.set_show_command_line(true);
    assert!(model.columns().contains(&ProcessColumn::CommandLine));
    assert!(
        cell(&with_command.command_line, |value| value.clone())
            .text()
            .is_some()
    );
}
