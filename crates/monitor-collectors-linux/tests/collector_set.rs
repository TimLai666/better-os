//! The whole collector set, driven through the shared `monitor-core` contract.

use monitor_collectors_linux::{LinuxCollectors, ProcessPrivacy, Roots};
use monitor_core::{
    CollectorHealth, MetricId, MetricSource, ObservationState, SupportState, Timestamp,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn metric(raw: &str) -> MetricId {
    MetricId::new(raw).unwrap()
}

#[test]
fn every_declared_metric_has_a_valid_name_a_source_and_a_summary() {
    let descriptors = LinuxCollectors::descriptors();
    assert!(descriptors.len() > 80, "catalog is suspiciously small");
    for descriptor in &descriptors {
        // MetricId::new already rejects a bad shape; re-parsing proves the
        // catalog constant would survive round-tripping through a store.
        assert!(MetricId::new(descriptor.id.as_str()).is_ok());
        assert!(
            !descriptor.summary.trim().is_empty(),
            "{} has no summary",
            descriptor.id
        );
        let source = match &descriptor.source {
            MetricSource::Proc(path)
            | MetricSource::Sys(path)
            | MetricSource::Derived(path)
            | MetricSource::Crate(path) => path,
        };
        assert!(
            !source.trim().is_empty(),
            "{} does not name where it comes from",
            descriptor.id
        );
    }
}

#[test]
fn no_two_collectors_claim_the_same_metric_name() {
    let descriptors = LinuxCollectors::descriptors();
    let mut seen = BTreeSet::new();
    for descriptor in &descriptors {
        assert!(
            seen.insert(descriptor.id.clone()),
            "{} is declared twice across the collector set",
            descriptor.id
        );
    }
}

#[test]
fn one_round_produces_one_report_per_collector_at_a_shared_timestamp() {
    let roots = Roots::at(fixture("snapshot-a"));
    let mut collectors = LinuxCollectors::new(roots.clone(), ProcessPrivacy::default());
    let when = at(0);
    let reports = collectors.sample(&roots, when);

    assert_eq!(reports.len(), LinuxCollectors::collector_names().len());
    let names: Vec<String> = reports
        .iter()
        .map(|report| report.collector.to_string())
        .collect();
    assert_eq!(names, LinuxCollectors::collector_names());
    for report in &reports {
        assert_eq!(report.observed_at, when);
    }
}

#[test]
fn the_captured_snapshot_produces_a_healthy_report_from_every_collector() {
    let roots = Roots::at(fixture("snapshot-a"));
    let mut collectors = LinuxCollectors::new(roots.clone(), ProcessPrivacy::default());
    for report in collectors.sample(&roots, at(0)) {
        assert!(
            matches!(report.health, CollectorHealth::Healthy),
            "{} reported {:?}",
            report.collector,
            report.health
        );
    }
}

#[test]
fn two_rounds_over_the_two_captured_snapshots_produce_real_values_everywhere() {
    let a = Roots::at(fixture("snapshot-a"));
    let b = Roots::at(fixture("snapshot-b"));
    let mut collectors = LinuxCollectors::new(a.clone(), ProcessPrivacy::default());
    collectors.sample(&a, at(0));
    let reports = collectors.sample(&b, at(2_860));

    let cpu = &reports[0];
    assert_eq!(
        cpu.metrics.state_of(&metric("cpu.utilization.busy")),
        ObservationState::Value
    );
    let busy = cpu
        .metrics
        .get(&metric("cpu.utilization.busy"))
        .unwrap()
        .as_f64()
        .unwrap();
    assert!((0.0..=1.0).contains(&busy), "busy ratio was {busy}");

    let memory = &reports[1];
    assert_eq!(
        memory.metrics.state_of(&metric("memory.page_in.rate")),
        ObservationState::Value
    );

    let storage = &reports[4];
    let nvme = storage
        .entities
        .iter()
        .find(|entity| entity.id.key == "nvme0n1")
        .expect("the captured nvme device");
    assert_eq!(
        nvme.metrics.state_of(&metric("storage.write.bytes.rate")),
        ObservationState::Value
    );

    let network = &reports[5];
    let wifi = network
        .entities
        .iter()
        .find(|entity| entity.id.key == "wlp98s0")
        .expect("the captured wireless interface");
    assert_eq!(
        wifi.metrics.state_of(&metric("network.rx.bytes.rate")),
        ObservationState::Value
    );
}

#[test]
fn capabilities_report_psi_unsupported_on_a_kernel_that_lacks_it_without_touching_the_rest() {
    let roots = Roots::at(fixture("no-psi"));
    let mut collectors = LinuxCollectors::new(roots.clone(), ProcessPrivacy::default());
    let reports = collectors.sample(&roots, at(0));

    let pressure = &reports[2];
    let capabilities =
        pressure.capabilities(&monitor_collectors_linux::PressureCollector::descriptors());
    assert!(!capabilities.is_empty());
    for capability in &capabilities {
        assert!(
            matches!(capability.support, SupportState::Unsupported(_)),
            "{} reported {:?}",
            capability.descriptor.id,
            capability.support
        );
    }

    let cpu = &reports[0];
    assert_eq!(
        cpu.support_of(&metric("cpu.load.average.1m")),
        SupportState::Supported
    );
}

#[test]
fn the_trait_objects_name_the_same_collectors_in_the_same_order() {
    let roots = Roots::at(fixture("snapshot-a"));
    let mut collectors = LinuxCollectors::new(roots, ProcessPrivacy::default());
    let names: Vec<String> = collectors
        .as_collectors()
        .iter()
        .map(|collector| collector.id().to_string())
        .collect();
    assert_eq!(names, LinuxCollectors::collector_names());
}

#[test]
fn driving_a_collector_through_the_trait_uses_the_roots_it_was_built_with() {
    let roots = Roots::at(fixture("snapshot-a"));
    let mut collectors = LinuxCollectors::new(roots, ProcessPrivacy::default());
    let mut objects = collectors.as_collectors();
    let cpu = &mut objects[0];
    assert!(!cpu.descriptors().is_empty());
    let report = cpu.collect(at(0));
    assert!(matches!(report.health, CollectorHealth::Healthy));
    assert!(!report.entities.is_empty());
}

#[test]
fn a_completely_absent_proc_leaves_every_collector_saying_so_rather_than_panicking() {
    let roots = Roots::at(fixture("does-not-exist"));
    let mut collectors = LinuxCollectors::new(roots.clone(), ProcessPrivacy::default());
    for report in collectors.sample(&roots, at(0)) {
        assert!(
            !matches!(report.health, CollectorHealth::Healthy),
            "{} claimed to be healthy without a /proc",
            report.collector
        );
    }
}

#[test]
fn the_real_machine_is_readable_and_reports_no_collector_failure() {
    // The one test that touches the live host. It asserts only what is true of
    // any Linux machine, so it cannot become flaky on different hardware.
    let roots = Roots::system();
    let mut collectors = LinuxCollectors::new(roots.clone(), ProcessPrivacy::default());
    collectors.sample(&roots, Timestamp::now());
    std::thread::sleep(std::time::Duration::from_millis(300));
    let reports = collectors.sample(&roots, Timestamp::now());

    for report in &reports {
        assert!(
            !matches!(report.health, CollectorHealth::Failed { .. }),
            "{} failed on the live host: {:?}",
            report.collector,
            report.health
        );
    }

    let cpu = &reports[0];
    let busy = cpu
        .metrics
        .get(&metric("cpu.utilization.busy"))
        .and_then(|observation| observation.as_f64())
        .expect("a busy ratio from the live host");
    assert!((0.0..=1.0).contains(&busy), "busy ratio was {busy}");

    let memory = &reports[1];
    let total = memory
        .metrics
        .get(&metric("memory.total"))
        .and_then(|observation| observation.as_f64())
        .expect("a memory total from the live host");
    assert!(total > 0.0);

    let processes = &reports[3];
    assert!(!processes.entities.is_empty());
}

#[test]
fn no_command_line_reaches_a_report_under_the_default_privacy_setting() {
    // The live host is used deliberately: a fixture cannot prove that real
    // command lines stay out.
    let roots = Roots::system();
    let mut collectors = LinuxCollectors::new(roots.clone(), ProcessPrivacy::default());
    let reports = collectors.sample(&roots, Timestamp::now());
    for report in &reports {
        for (_, id, observation) in report.observations() {
            if id.as_str() == "process.command_line" {
                assert_eq!(observation.state(), ObservationState::Unsupported);
                assert_eq!(observation.as_text(), None);
            }
        }
    }
}
