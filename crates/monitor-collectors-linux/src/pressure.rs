//! Pressure stall information.
//!
//! Upstream interface: `/proc/pressure/cpu`, `/proc/pressure/memory`, and
//! `/proc/pressure/io`, documented in the kernel's `Documentation/accounting/
//! psi.rst` (docs.kernel.org build 7.2.0).
//!
//! This is the metric that separates a busy machine from a stalled one, and it
//! is also the one most likely to be absent: a kernel built without
//! `CONFIG_PSI`, or booted with `psi=0`, has no `/proc/pressure` directory at
//! all. That is reported as `Unsupported` for the whole collector, once, with
//! the path that was missing — never as zero pressure, which would read as a
//! perfectly healthy system.
//!
//! Two semantics from the documentation are preserved rather than smoothed
//! over. `avg10`, `avg60`, and `avg300` are percentages the kernel has already
//! averaged over its own windows, so this collector republishes them and does
//! not average them again. And `full` on the CPU file is undefined at system
//! level; the kernel has reported a zero line since 5.13 for backward
//! compatibility, so it is reported as `NotReported` rather than as a real
//! zero.

use crate::catalog::{collector_id, counter, kernel_averaged, metric_id, proc_source, utilization};
use crate::fsread::{MalformedInput, read_text};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::BTreeMap;

/// The resources the kernel publishes pressure for.
pub const PRESSURE_RESOURCES: [&str; 3] = ["cpu", "memory", "io"];

/// One `some` or `full` line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PressureSeries {
    /// Percentage of the last ten seconds in which the stall condition held.
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    /// Total stall time since boot, in microseconds.
    pub total_microseconds: u64,
}

/// One `/proc/pressure/*` file.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PressureFile {
    /// At least one task stalled.
    pub some: Option<PressureSeries>,
    /// Every runnable task stalled. Undefined for CPU at system level.
    pub full: Option<PressureSeries>,
}

const PROC_PRESSURE: &str = "/proc/pressure";

pub fn parse_pressure(input: &str) -> Result<PressureFile, MalformedInput> {
    let mut parsed = PressureFile::default();
    for line in input.lines() {
        let mut fields = line.split_whitespace();
        let Some(kind) = fields.next() else {
            continue;
        };
        let mut series = PressureSeries::default();
        let mut seen = 0;
        for field in fields {
            let Some((name, raw)) = field.split_once('=') else {
                return Err(MalformedInput::new(
                    PROC_PRESSURE,
                    format!("field {field:?} is not name=value"),
                ));
            };
            match name {
                "avg10" | "avg60" | "avg300" => {
                    let value = raw.parse::<f64>().map_err(|_| {
                        MalformedInput::new(
                            PROC_PRESSURE,
                            format!("{name} is not a number: {raw:?}"),
                        )
                    })?;
                    match name {
                        "avg10" => series.avg10 = value,
                        "avg60" => series.avg60 = value,
                        _ => series.avg300 = value,
                    }
                    seen += 1;
                }
                "total" => {
                    series.total_microseconds = raw.parse::<u64>().map_err(|_| {
                        MalformedInput::new(
                            PROC_PRESSURE,
                            format!("total is not a number: {raw:?}"),
                        )
                    })?;
                    seen += 1;
                }
                _ => {}
            }
        }
        if seen < 4 {
            return Err(MalformedInput::new(
                PROC_PRESSURE,
                format!("{kind} line has {seen} of the four expected fields"),
            ));
        }
        match kind {
            "some" => parsed.some = Some(series),
            "full" => parsed.full = Some(series),
            other => {
                return Err(MalformedInput::new(
                    PROC_PRESSURE,
                    format!("unknown line kind {other:?}"),
                ));
            }
        }
    }
    if parsed.some.is_none() && parsed.full.is_none() {
        return Err(MalformedInput::new(PROC_PRESSURE, "no some or full line"));
    }
    Ok(parsed)
}

const PRESSURE_COLLECTOR: &str = "linux.pressure";

/// `(metric suffix, extractor)` for the four values of one series.
type SeriesExtractor = fn(&PressureSeries) -> f64;
const SERIES_FIELDS: [(&str, SeriesExtractor); 3] = [
    ("avg10", |series| series.avg10),
    ("avg60", |series| series.avg60),
    ("avg300", |series| series.avg300),
];

fn series_metrics(scope: &str, series: Option<PressureSeries>, metrics: &mut MetricSet) {
    for (field, extract) in SERIES_FIELDS {
        let observation = match series {
            Some(series) => Observation::float(extract(&series)),
            None => Observation::Unsupported(UnsupportedReason::NotReported {
                detail: format!("no {scope} line in this pressure file"),
            }),
        };
        metrics.insert(metric_id(&format!("pressure.{scope}.{field}")), observation);
    }
    metrics.insert(
        metric_id(&format!("pressure.{scope}.total")),
        match series {
            Some(series) => Observation::unsigned(series.total_microseconds),
            None => Observation::Unsupported(UnsupportedReason::NotReported {
                detail: format!("no {scope} line in this pressure file"),
            }),
        },
    );
}

struct PressureSnapshot {
    at: Timestamp,
    totals: BTreeMap<String, (Option<u64>, Option<u64>)>,
}

/// PSI for CPU, memory, and I/O.
pub struct PressureCollector {
    roots: Roots,
    previous: Option<PressureSnapshot>,
}

impl PressureCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            previous: None,
        }
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        let mut descriptors = Vec::new();
        for scope in ["some", "full"] {
            for window in ["avg10", "avg60", "avg300"] {
                descriptors.push(kernel_averaged(
                    &format!("pressure.{scope}.{window}"),
                    Unit::Percent,
                    proc_source("pressure/{resource}"),
                    format!("percentage of the last {window} window in which {scope} tasks stalled, as the kernel averaged it"),
                ));
            }
            descriptors.push(counter(
                &format!("pressure.{scope}.total"),
                Unit::Microseconds,
                proc_source("pressure/{resource}"),
                format!("total {scope} stall time since boot"),
            ));
            descriptors.push(utilization(
                &format!("pressure.{scope}.stall_ratio"),
                crate::catalog::derived_source("pressure total delta / elapsed wall time"),
                format!("fraction of the sampling interval spent in a {scope} stall"),
            ));
        }
        descriptors
    }

    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(PRESSURE_COLLECTOR), at);
        let directory = roots.proc("pressure");
        if !directory.exists() {
            // A kernel without CONFIG_PSI. Say so once, for the whole
            // subsystem, rather than reporting three files of zeroes.
            report.health = CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
                path: directory.display().to_string(),
            });
            for resource in PRESSURE_RESOURCES {
                let mut metrics = MetricSet::new();
                for scope in ["some", "full"] {
                    series_metrics(scope, None, &mut metrics);
                    metrics.insert(
                        metric_id(&format!("pressure.{scope}.stall_ratio")),
                        Observation::Unsupported(UnsupportedReason::InterfaceMissing {
                            path: directory.display().to_string(),
                        }),
                    );
                }
                report.entities.push(Entity::new(
                    EntityId::new(EntityKind::PressureResource, resource),
                    metrics,
                ));
            }
            return report;
        }

        let seconds = self
            .previous
            .as_ref()
            .and_then(|previous| Timestamp::interval_seconds(previous.at, at));
        let mut totals = BTreeMap::new();
        let mut degraded = Vec::new();

        for resource in PRESSURE_RESOURCES {
            let path = roots.proc(&format!("pressure/{resource}"));
            let mut metrics = MetricSet::new();
            let parsed = match read_text(&path) {
                Ok(raw) => match parse_pressure(&raw) {
                    Ok(parsed) => Ok(parsed),
                    Err(error) => {
                        degraded.push(format!("{resource}: {}", error.detail));
                        Err(error.into_observation())
                    }
                },
                Err(error) => {
                    degraded.push(format!("{resource}: {} unreadable", error.path().display()));
                    Err(error.into_observation())
                }
            };

            match parsed {
                Ok(file) => {
                    // The CPU file's `full` line is a compatibility zero, not
                    // a measurement, so it is not offered as one.
                    let full = if resource == "cpu" { None } else { file.full };
                    series_metrics("some", file.some, &mut metrics);
                    series_metrics("full", full, &mut metrics);
                    let previous_totals = self
                        .previous
                        .as_ref()
                        .and_then(|previous| previous.totals.get(resource))
                        .copied()
                        .unwrap_or((None, None));
                    metrics.insert(
                        metric_id("pressure.some.stall_ratio"),
                        stall_ratio(previous_totals.0, file.some, seconds),
                    );
                    metrics.insert(
                        metric_id("pressure.full.stall_ratio"),
                        stall_ratio(previous_totals.1, full, seconds),
                    );
                    totals.insert(
                        resource.to_string(),
                        (
                            file.some.map(|series| series.total_microseconds),
                            full.map(|series| series.total_microseconds),
                        ),
                    );
                }
                Err(observation) => {
                    for scope in ["some", "full"] {
                        for field in ["avg10", "avg60", "avg300", "total", "stall_ratio"] {
                            metrics.insert(
                                metric_id(&format!("pressure.{scope}.{field}")),
                                observation.clone(),
                            );
                        }
                    }
                }
            }

            report.entities.push(Entity::new(
                EntityId::new(EntityKind::PressureResource, resource),
                metrics,
            ));
        }

        if !degraded.is_empty() {
            report.health = CollectorHealth::Degraded {
                detail: degraded.join("; "),
            };
        }
        self.previous = Some(PressureSnapshot { at, totals });
        report
    }
}

/// The fraction of wall time spent stalled, from the difference between two
/// `total=` reads.
fn stall_ratio(
    earlier: Option<u64>,
    later: Option<PressureSeries>,
    seconds: Option<f64>,
) -> Observation {
    let Some(later) = later else {
        return Observation::Unsupported(UnsupportedReason::NotReported {
            detail: "no such line in this pressure file".into(),
        });
    };
    let (Some(earlier), Some(seconds)) = (earlier, seconds) else {
        return Observation::Unknown(UnknownReason::NotYetSampled);
    };
    let stalled_microseconds = later.total_microseconds.saturating_sub(earlier) as f64;
    Observation::float(stalled_microseconds / (seconds * 1_000_000.0))
}

impl Collector for PressureCollector {
    fn id(&self) -> CollectorId {
        collector_id(PRESSURE_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        PressureCollector::descriptors()
    }

    fn collect(&mut self, at: Timestamp) -> CollectorReport {
        let roots = self.roots.clone();
        self.sample(&roots, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempTree, at, fixture};
    use monitor_core::ObservationState;

    fn resource<'a>(report: &'a CollectorReport, name: &str) -> &'a Entity {
        report
            .entities
            .iter()
            .find(|entity| entity.id.key == name)
            .expect("a pressure resource")
    }

    #[test]
    fn parses_both_lines_of_a_captured_pressure_file() {
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("proc/pressure/io")).unwrap();
        let parsed = parse_pressure(&raw).unwrap();
        let some = parsed.some.expect("a some line");
        assert_eq!(some.total_microseconds, 2_746_439);
        let full = parsed.full.expect("a full line");
        assert_eq!(full.total_microseconds, 2_631_951);
    }

    #[test]
    fn a_truncated_pressure_line_is_malformed_rather_than_zero_pressure() {
        let error = parse_pressure("some avg10=0.00 avg60=0.00\n").unwrap_err();
        assert!(error.detail.contains("of the four expected fields"));
    }

    #[test]
    fn a_field_that_is_not_name_equals_value_is_malformed() {
        let error = parse_pressure("some avg10 avg60=0.00 avg300=0.00 total=1\n").unwrap_err();
        assert!(error.detail.contains("not name=value"));
    }

    #[test]
    fn an_unknown_line_kind_is_malformed() {
        let error =
            parse_pressure("sideways avg10=0.00 avg60=0.00 avg300=0.00 total=1\n").unwrap_err();
        assert!(error.detail.contains("unknown line kind"));
    }

    #[test]
    fn a_kernel_without_config_psi_reports_unsupported_for_the_whole_subsystem() {
        let roots = Roots::at(fixture("no-psi"));
        let mut collector = PressureCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert!(matches!(report.health, CollectorHealth::Unsupported(_)));
        assert_eq!(report.entities.len(), 3);
        for entity in &report.entities {
            assert_eq!(
                entity.metrics.state_of(&metric_id("pressure.some.avg10")),
                ObservationState::Unsupported
            );
            // The distinction that matters: this is not a measurement of no
            // pressure.
            assert_eq!(
                entity
                    .metrics
                    .get(&metric_id("pressure.some.avg10"))
                    .unwrap()
                    .as_f64(),
                None
            );
        }
    }

    #[test]
    fn the_kernels_averages_are_republished_rather_than_averaged_again() {
        let roots = Roots::at(fixture("synthetic-a"));
        let mut collector = PressureCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        let cpu = resource(&report, "cpu");
        assert_eq!(
            cpu.metrics
                .get(&metric_id("pressure.some.avg10"))
                .unwrap()
                .as_f64(),
            Some(1.25)
        );
        assert_eq!(
            cpu.metrics
                .get(&metric_id("pressure.some.total"))
                .unwrap()
                .as_f64(),
            Some(1_000_000.0)
        );
    }

    #[test]
    fn the_cpu_full_line_is_not_offered_as_a_measurement() {
        // The kernel emits a zero `full` line for CPU only for backward
        // compatibility; reporting it as a real zero would claim no task ever
        // stalled completely, which the file does not say.
        let roots = Roots::at(fixture("synthetic-a"));
        let mut collector = PressureCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert_eq!(
            resource(&report, "cpu")
                .metrics
                .state_of(&metric_id("pressure.full.avg10")),
            ObservationState::Unsupported
        );
        // The io file's full line is real and is reported.
        assert_eq!(
            resource(&report, "io")
                .metrics
                .get(&metric_id("pressure.full.avg10"))
                .unwrap()
                .as_f64(),
            Some(0.10)
        );
    }

    #[test]
    fn the_stall_ratio_needs_two_samples() {
        let a = Roots::at(fixture("synthetic-a"));
        let b = Roots::at(fixture("synthetic-b"));
        let mut collector = PressureCollector::new(a.clone());
        let first = collector.sample(&a, at(0));
        assert_eq!(
            resource(&first, "cpu")
                .metrics
                .state_of(&metric_id("pressure.some.stall_ratio")),
            ObservationState::Unknown
        );

        // CPU some total goes 1_000_000 -> 3_000_000 microseconds, so two
        // seconds of stall inside four seconds of wall time: 0.5.
        let second = collector.sample(&b, at(4_000));
        let ratio = resource(&second, "cpu")
            .metrics
            .get(&metric_id("pressure.some.stall_ratio"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn one_malformed_pressure_file_degrades_the_collector_without_hiding_the_others() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::write(
            temporary.roots().proc("pressure/memory"),
            "some avg10=nought avg60=0.00 avg300=0.00 total=1\n",
        )
        .unwrap();
        let mut collector = PressureCollector::new(temporary.roots());
        let report = collector.sample(&temporary.roots(), at(0));
        assert!(matches!(report.health, CollectorHealth::Degraded { .. }));
        assert_eq!(
            resource(&report, "memory")
                .metrics
                .state_of(&metric_id("pressure.some.avg10")),
            ObservationState::Unknown
        );
        assert_eq!(
            resource(&report, "cpu")
                .metrics
                .state_of(&metric_id("pressure.some.avg10")),
            ObservationState::Value
        );
    }

    #[test]
    fn a_truncated_pressure_tree_degrades_every_resource() {
        let roots = Roots::at(fixture("truncated"));
        let mut collector = PressureCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert!(matches!(report.health, CollectorHealth::Degraded { .. }));
        for entity in &report.entities {
            assert_eq!(
                entity.metrics.state_of(&metric_id("pressure.some.avg10")),
                ObservationState::Unknown
            );
        }
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = PressureCollector::descriptors();
        let mut seen = std::collections::BTreeSet::new();
        for descriptor in &descriptors {
            assert!(
                seen.insert(descriptor.id.clone()),
                "duplicate metric {}",
                descriptor.id
            );
        }
        assert_eq!(seen.len(), descriptors.len());
    }
}
