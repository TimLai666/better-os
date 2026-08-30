//! Memory, swap, and paging.
//!
//! Upstream interface: `/proc/meminfo` and `/proc/vmstat`, documented in the
//! kernel's `Documentation/filesystems/proc.rst`.
//!
//! Three unit traps live in these files and are converted here so no consumer
//! has to know about them. `/proc/meminfo` writes `kB` but means kibibytes, so
//! every size is multiplied by 1024. `/proc/vmstat`'s `pgpgin` and `pgpgout`
//! also count kibibytes despite the `pg` prefix, so they become bytes per
//! second. `pswpin` and `pswpout` really do count pages, so they stay counts
//! per second rather than being silently multiplied by a page size this crate
//! does not read.
//!
//! `memory.used` is `MemTotal - MemAvailable`, not `MemTotal - MemFree`. Free
//! memory is not the same as available memory, and the difference is exactly
//! the reclaimable cache the user should not be told is "used".

use crate::catalog::{
    MINIMUM_DELTA_INTERVAL, collector_id, derived_source, gauge, metric_id, proc_source, rate,
};
use crate::fsread::{MalformedInput, ReadError, read_text};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, MetricDescriptor, Observation,
    Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::BTreeMap;

const PROC_MEMINFO: &str = "/proc/meminfo";
const PROC_VMSTAT: &str = "/proc/vmstat";

/// A kibibyte, which is what `/proc/meminfo` means when it writes `kB`.
const MEMINFO_UNIT_BYTES: u64 = 1024;

/// Parse `/proc/meminfo` into its raw kibibyte values.
///
/// Keys are kept verbatim, parentheses included, because `Active(anon)` is a
/// key and not a function call.
pub fn parse_meminfo(input: &str) -> Result<BTreeMap<String, u64>, MalformedInput> {
    let mut values = BTreeMap::new();
    for line in input.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let mut fields = rest.split_whitespace();
        let Some(raw) = fields.next() else {
            return Err(MalformedInput::new(
                PROC_MEMINFO,
                format!("{key} has no value"),
            ));
        };
        let value = raw.parse::<u64>().map_err(|_| {
            MalformedInput::new(PROC_MEMINFO, format!("{key} is not a number: {raw:?}"))
        })?;
        // A line without a unit, such as HugePages_Total, is a count and is
        // not a size; those are not in the catalog below, so they are simply
        // recorded as-is.
        values.insert(key.trim().to_string(), value);
    }
    if values.is_empty() {
        return Err(MalformedInput::new(PROC_MEMINFO, "no recognisable lines"));
    }
    if !values.contains_key("MemTotal") {
        return Err(MalformedInput::new(PROC_MEMINFO, "no MemTotal line"));
    }
    Ok(values)
}

/// Parse `/proc/vmstat`, whose every line is `name value`.
pub fn parse_vmstat(input: &str) -> Result<BTreeMap<String, u64>, MalformedInput> {
    let mut values = BTreeMap::new();
    for line in input.lines() {
        let mut fields = line.split_whitespace();
        let Some(key) = fields.next() else {
            continue;
        };
        let Some(raw) = fields.next() else {
            return Err(MalformedInput::new(
                PROC_VMSTAT,
                format!("{key} has no value"),
            ));
        };
        let value = raw.parse::<u64>().map_err(|_| {
            MalformedInput::new(PROC_VMSTAT, format!("{key} is not a number: {raw:?}"))
        })?;
        values.insert(key.to_string(), value);
    }
    if values.is_empty() {
        return Err(MalformedInput::new(PROC_VMSTAT, "no recognisable lines"));
    }
    Ok(values)
}

/// `(metric, meminfo key)` for every size Better Monitor reports directly.
const MEMINFO_SIZES: [(&str, &str); 30] = [
    ("memory.total", "MemTotal"),
    ("memory.free", "MemFree"),
    ("memory.available", "MemAvailable"),
    ("memory.buffers", "Buffers"),
    ("memory.cached", "Cached"),
    ("memory.active", "Active"),
    ("memory.inactive", "Inactive"),
    ("memory.active.anonymous", "Active(anon)"),
    ("memory.inactive.anonymous", "Inactive(anon)"),
    ("memory.active.file", "Active(file)"),
    ("memory.inactive.file", "Inactive(file)"),
    ("memory.unevictable", "Unevictable"),
    ("memory.locked", "Mlocked"),
    ("memory.anonymous", "AnonPages"),
    ("memory.mapped", "Mapped"),
    ("memory.shared", "Shmem"),
    ("memory.dirty", "Dirty"),
    ("memory.writeback", "Writeback"),
    ("memory.slab", "Slab"),
    ("memory.slab.reclaimable", "SReclaimable"),
    ("memory.slab.unreclaimable", "SUnreclaim"),
    ("memory.kernel.reclaimable", "KReclaimable"),
    ("memory.page_tables", "PageTables"),
    ("memory.per_cpu", "Percpu"),
    ("memory.commit.limit", "CommitLimit"),
    ("memory.commit.committed", "Committed_AS"),
    ("memory.swap.total", "SwapTotal"),
    ("memory.swap.free", "SwapFree"),
    ("memory.swap.cached", "SwapCached"),
    ("memory.zswap.compressed", "Zswap"),
];

/// `(metric, vmstat counter, unit)` for the paging rates.
///
/// The unit column is where the `pgpgin` kibibyte trap is handled.
const VMSTAT_RATES: [(&str, &str, Unit); 9] = [
    ("memory.page_in.rate", "pgpgin", Unit::BytesPerSecond),
    ("memory.page_out.rate", "pgpgout", Unit::BytesPerSecond),
    ("memory.swap_in.rate", "pswpin", Unit::CountPerSecond),
    ("memory.swap_out.rate", "pswpout", Unit::CountPerSecond),
    (
        "memory.fault.major.rate",
        "pgmajfault",
        Unit::CountPerSecond,
    ),
    ("memory.fault.total.rate", "pgfault", Unit::CountPerSecond),
    (
        "memory.reclaim.background.rate",
        "pgscan_kswapd",
        Unit::CountPerSecond,
    ),
    (
        "memory.reclaim.direct.rate",
        "pgscan_direct",
        Unit::CountPerSecond,
    ),
    ("memory.oom.kill.rate", "oom_kill", Unit::CountPerSecond),
];

const MEMORY_COLLECTOR: &str = "linux.memory";

struct MemorySnapshot {
    at: Timestamp,
    vmstat: BTreeMap<String, u64>,
}

/// Memory breakdown, swap, and paging rates.
pub struct MemoryCollector {
    roots: Roots,
    previous: Option<MemorySnapshot>,
}

impl MemoryCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            previous: None,
        }
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        let mut descriptors: Vec<MetricDescriptor> = MEMINFO_SIZES
            .iter()
            .map(|(metric, key)| {
                gauge(
                    metric,
                    Unit::Bytes,
                    proc_source("meminfo"),
                    format!("{key} converted from kibibytes to bytes"),
                )
            })
            .collect();
        descriptors.extend(VMSTAT_RATES.iter().map(|(metric, key, unit)| {
            rate(
                metric,
                *unit,
                proc_source("vmstat"),
                format!("delta of the {key} counter per second"),
            )
        }));
        descriptors.extend([
            gauge(
                "memory.used",
                Unit::Bytes,
                derived_source("MemTotal - MemAvailable"),
                "memory that is neither free nor reclaimable without cost",
            ),
            gauge(
                "memory.utilization",
                Unit::Ratio,
                derived_source("memory.used / memory.total"),
                "fraction of memory that is not available",
            ),
            gauge(
                "memory.swap.used",
                Unit::Bytes,
                derived_source("SwapTotal - SwapFree"),
                "swap in use",
            ),
            gauge(
                "memory.swap.utilization",
                Unit::Ratio,
                derived_source("memory.swap.used / memory.swap.total"),
                "fraction of swap in use, unknown when no swap is configured",
            ),
            rate(
                "memory.fault.minor.rate",
                Unit::CountPerSecond,
                derived_source("pgfault - pgmajfault"),
                "page faults per second that did not need a disk read",
            ),
        ]);
        descriptors
    }

    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(MEMORY_COLLECTOR), at);
        match read_text(&roots.proc("meminfo")).map(|raw| parse_meminfo(&raw)) {
            Ok(Ok(meminfo)) => self.report_meminfo(&meminfo, &mut report),
            Ok(Err(error)) => {
                report.health = CollectorHealth::Failed {
                    detail: format!("{}: {}", error.context, error.detail),
                };
                return report;
            }
            Err(error) => {
                report.health = failed_or_unsupported(&error);
                return report;
            }
        }
        self.report_vmstat(roots, at, &mut report);
        report
    }

    fn report_meminfo(&self, meminfo: &BTreeMap<String, u64>, report: &mut CollectorReport) {
        for (metric, key) in MEMINFO_SIZES {
            let observation = match meminfo.get(key) {
                Some(kibibytes) => {
                    Observation::unsigned(kibibytes.saturating_mul(MEMINFO_UNIT_BYTES))
                }
                None => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: format!("{key} absent from /proc/meminfo on this kernel"),
                }),
            };
            report.metrics.insert(metric_id(metric), observation);
        }

        let total = meminfo.get("MemTotal").copied();
        let available = meminfo.get("MemAvailable").copied();
        let used = match (total, available) {
            (Some(total), Some(available)) => Some(total.saturating_sub(available)),
            _ => None,
        };
        report.metrics.insert(
            metric_id("memory.used"),
            match used {
                Some(kibibytes) => {
                    Observation::unsigned(kibibytes.saturating_mul(MEMINFO_UNIT_BYTES))
                }
                None => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "MemAvailable absent; used memory cannot be derived honestly".into(),
                }),
            },
        );
        report.metrics.insert(
            metric_id("memory.utilization"),
            ratio(used, total, "MemTotal is zero"),
        );

        let swap_total = meminfo.get("SwapTotal").copied();
        let swap_free = meminfo.get("SwapFree").copied();
        let swap_used = match (swap_total, swap_free) {
            (Some(total), Some(free)) => Some(total.saturating_sub(free)),
            _ => None,
        };
        report.metrics.insert(
            metric_id("memory.swap.used"),
            match swap_used {
                Some(kibibytes) => {
                    Observation::unsigned(kibibytes.saturating_mul(MEMINFO_UNIT_BYTES))
                }
                None => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "SwapTotal or SwapFree absent from /proc/meminfo".into(),
                }),
            },
        );
        report.metrics.insert(
            metric_id("memory.swap.utilization"),
            ratio(swap_used, swap_total, "no swap is configured"),
        );
    }

    fn report_vmstat(&mut self, roots: &Roots, at: Timestamp, report: &mut CollectorReport) {
        let vmstat = match read_text(&roots.proc("vmstat")).map(|raw| parse_vmstat(&raw)) {
            Ok(Ok(vmstat)) => vmstat,
            Ok(Err(error)) => {
                for (metric, _, _) in VMSTAT_RATES {
                    report
                        .metrics
                        .insert(metric_id(metric), error.clone().into_observation());
                }
                report.health = CollectorHealth::Degraded {
                    detail: format!("{}: {}", error.context, error.detail),
                };
                return;
            }
            Err(error) => {
                for (metric, _, _) in VMSTAT_RATES {
                    report
                        .metrics
                        .insert(metric_id(metric), error.clone().into_observation());
                }
                report.health = CollectorHealth::Degraded {
                    detail: format!("{} unreadable", error.path().display()),
                };
                return;
            }
        };

        let seconds = self
            .previous
            .as_ref()
            .and_then(|previous| Timestamp::interval_seconds(previous.at, at))
            .filter(|seconds| *seconds >= MINIMUM_DELTA_INTERVAL.as_secs_f64());

        for (metric, key, unit) in VMSTAT_RATES {
            let scale = if unit == Unit::BytesPerSecond {
                // pgpgin and pgpgout count kibibytes, not pages.
                MEMINFO_UNIT_BYTES as f64
            } else {
                1.0
            };
            let observation = self.counter_rate(&vmstat, key, seconds, scale);
            report.metrics.insert(metric_id(metric), observation);
        }

        let minor = match (
            self.previous.as_ref(),
            vmstat.get("pgfault"),
            vmstat.get("pgmajfault"),
            seconds,
        ) {
            (Some(previous), Some(faults), Some(major), Some(seconds)) => {
                match (
                    previous.vmstat.get("pgfault"),
                    previous.vmstat.get("pgmajfault"),
                ) {
                    (Some(earlier_faults), Some(earlier_major)) => {
                        let all = faults.saturating_sub(*earlier_faults);
                        let heavy = major.saturating_sub(*earlier_major);
                        Observation::float(all.saturating_sub(heavy) as f64 / seconds)
                    }
                    _ => Observation::Unknown(UnknownReason::NotYetSampled),
                }
            }
            (_, None, _, _) | (_, _, None, _) => {
                Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "pgfault or pgmajfault absent from /proc/vmstat".into(),
                })
            }
            (None, _, _, _) => Observation::Unknown(UnknownReason::NotYetSampled),
            (_, _, _, None) => Observation::Unknown(UnknownReason::IntervalTooShort),
        };
        report
            .metrics
            .insert(metric_id("memory.fault.minor.rate"), minor);

        self.previous = Some(MemorySnapshot { at, vmstat });
    }

    fn counter_rate(
        &self,
        vmstat: &BTreeMap<String, u64>,
        key: &str,
        seconds: Option<f64>,
        scale: f64,
    ) -> Observation {
        let Some(later) = vmstat.get(key) else {
            return Observation::Unsupported(UnsupportedReason::NotReported {
                detail: format!("{key} absent from /proc/vmstat on this kernel"),
            });
        };
        let Some(previous) = self.previous.as_ref() else {
            return Observation::Unknown(UnknownReason::NotYetSampled);
        };
        let Some(earlier) = previous.vmstat.get(key) else {
            return Observation::Unknown(UnknownReason::NotYetSampled);
        };
        let Some(seconds) = seconds else {
            return Observation::Unknown(UnknownReason::IntervalTooShort);
        };
        Observation::float(later.saturating_sub(*earlier) as f64 * scale / seconds)
    }
}

fn ratio(part: Option<u64>, whole: Option<u64>, absent_detail: &str) -> Observation {
    match (part, whole) {
        (Some(part), Some(whole)) if whole > 0 => Observation::float(part as f64 / whole as f64),
        _ => Observation::Unsupported(UnsupportedReason::NotReported {
            detail: absent_detail.to_string(),
        }),
    }
}

fn failed_or_unsupported(error: &ReadError) -> CollectorHealth {
    match error {
        ReadError::Missing { path } => {
            CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
                path: path.display().to_string(),
            })
        }
        other => CollectorHealth::Failed {
            detail: other.path().display().to_string(),
        },
    }
}

impl Collector for MemoryCollector {
    fn id(&self) -> CollectorId {
        collector_id(MEMORY_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        MemoryCollector::descriptors()
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

    fn snapshot(name: &str) -> Roots {
        Roots::at(fixture(name))
    }

    #[test]
    fn parses_the_captured_meminfo_including_parenthesised_keys() {
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("proc/meminfo")).unwrap();
        let meminfo = parse_meminfo(&raw).unwrap();
        assert_eq!(meminfo["MemTotal"], 31_942_664);
        assert_eq!(meminfo["MemAvailable"], 26_615_420);
        assert_eq!(meminfo["Active(anon)"], 2_703_072);
        assert_eq!(meminfo["Inactive(anon)"], 0);
    }

    #[test]
    fn a_kibibyte_line_becomes_bytes_not_kilobytes() {
        let roots = snapshot("synthetic-a");
        let mut collector = MemoryCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        // MemTotal is 1000000 kB in the fixture.
        assert_eq!(
            report
                .metrics
                .get(&metric_id("memory.total"))
                .unwrap()
                .as_f64(),
            Some(1_000_000.0 * 1024.0)
        );
    }

    #[test]
    fn used_memory_is_total_minus_available_not_total_minus_free() {
        let roots = snapshot("synthetic-a");
        let mut collector = MemoryCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        // total 1000000, free 200000, available 600000.
        let used = report
            .metrics
            .get(&metric_id("memory.used"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert_eq!(used, 400_000.0 * 1024.0);
        assert_ne!(used, 800_000.0 * 1024.0);
        let utilization = report
            .metrics
            .get(&metric_id("memory.utilization"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((utilization - 0.4).abs() < 1e-9);
    }

    #[test]
    fn swap_use_is_derived_from_the_two_swap_lines() {
        let roots = snapshot("synthetic-a");
        let mut collector = MemoryCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert_eq!(
            report
                .metrics
                .get(&metric_id("memory.swap.used"))
                .unwrap()
                .as_f64(),
            Some(100_000.0 * 1024.0)
        );
        let utilization = report
            .metrics
            .get(&metric_id("memory.swap.utilization"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((utilization - 0.25).abs() < 1e-9);
    }

    #[test]
    fn a_machine_with_no_swap_reports_unsupported_rather_than_zero_utilization() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::write(
            temporary.roots().proc("meminfo"),
            "MemTotal: 1000000 kB\nMemFree: 200000 kB\nMemAvailable: 600000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        let mut collector = MemoryCollector::new(temporary.roots());
        let report = collector.sample(&temporary.roots(), at(0));
        assert_eq!(
            report
                .metrics
                .state_of(&metric_id("memory.swap.utilization")),
            ObservationState::Unsupported
        );
        // The zero-sized swap itself is still a real measurement.
        assert_eq!(
            report
                .metrics
                .get(&metric_id("memory.swap.total"))
                .unwrap()
                .as_f64(),
            Some(0.0)
        );
    }

    #[test]
    fn page_in_counts_kibibytes_and_swap_in_counts_pages() {
        // pgpgin goes 1000 -> 3000 (2000 KiB) and pswpin 5 -> 15 (10 pages)
        // over one second.
        let a = snapshot("synthetic-a");
        let b = snapshot("synthetic-b");
        let mut collector = MemoryCollector::new(a.clone());
        collector.sample(&a, at(0));
        let report = collector.sample(&b, at(1_000));

        let page_in = report
            .metrics
            .get(&metric_id("memory.page_in.rate"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((page_in - 2000.0 * 1024.0).abs() < 1e-6);

        let swap_in = report
            .metrics
            .get(&metric_id("memory.swap_in.rate"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((swap_in - 10.0).abs() < 1e-9);
    }

    #[test]
    fn minor_faults_are_the_faults_that_were_not_major() {
        // pgfault 10000 -> 14000, pgmajfault 100 -> 300, over one second.
        let a = snapshot("synthetic-a");
        let b = snapshot("synthetic-b");
        let mut collector = MemoryCollector::new(a.clone());
        collector.sample(&a, at(0));
        let report = collector.sample(&b, at(1_000));
        let minor = report
            .metrics
            .get(&metric_id("memory.fault.minor.rate"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((minor - 3800.0).abs() < 1e-6);
        let major = report
            .metrics
            .get(&metric_id("memory.fault.major.rate"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((major - 200.0).abs() < 1e-6);
    }

    #[test]
    fn a_counter_that_did_not_move_reports_a_real_zero() {
        // pgpgout is 2000 in both synthetic samples.
        let a = snapshot("synthetic-a");
        let b = snapshot("synthetic-b");
        let mut collector = MemoryCollector::new(a.clone());
        collector.sample(&a, at(0));
        let report = collector.sample(&b, at(1_000));
        let out = report
            .metrics
            .get(&metric_id("memory.page_out.rate"))
            .unwrap();
        assert_eq!(out.state(), ObservationState::Value);
        assert_eq!(out.as_f64(), Some(0.0));
    }

    #[test]
    fn the_first_round_reports_unknown_paging_rates() {
        let roots = snapshot("synthetic-a");
        let mut collector = MemoryCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert_eq!(
            report.metrics.state_of(&metric_id("memory.page_in.rate")),
            ObservationState::Unknown
        );
    }

    #[test]
    fn a_vmstat_counter_this_kernel_lacks_is_unsupported_not_zero() {
        let roots = snapshot("synthetic-a");
        let mut collector = MemoryCollector::new(roots.clone());
        collector.sample(&roots, at(0));
        let report = collector.sample(&roots, at(1_000));
        // The synthetic vmstat has no pgscan_kswapd line.
        assert_eq!(
            report
                .metrics
                .state_of(&metric_id("memory.reclaim.background.rate")),
            ObservationState::Unsupported
        );
    }

    #[test]
    fn a_truncated_meminfo_fails_the_collector() {
        let roots = snapshot("truncated");
        let mut collector = MemoryCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert!(matches!(report.health, CollectorHealth::Failed { .. }));
    }

    #[test]
    fn a_malformed_meminfo_value_fails_the_collector() {
        let roots = snapshot("malformed");
        let mut collector = MemoryCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert!(matches!(report.health, CollectorHealth::Failed { .. }));
    }

    #[test]
    fn a_meminfo_without_memtotal_is_malformed() {
        let error = parse_meminfo("MemFree: 100 kB\n").unwrap_err();
        assert!(error.detail.contains("no MemTotal"));
    }

    #[test]
    fn a_malformed_vmstat_degrades_the_collector_without_losing_meminfo() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::write(temporary.roots().proc("vmstat"), "pgpgin many\n").unwrap();
        let mut collector = MemoryCollector::new(temporary.roots());
        let report = collector.sample(&temporary.roots(), at(0));
        assert!(matches!(report.health, CollectorHealth::Degraded { .. }));
        assert_eq!(
            report.metrics.state_of(&metric_id("memory.total")),
            ObservationState::Value
        );
        assert_eq!(
            report.metrics.state_of(&metric_id("memory.page_in.rate")),
            ObservationState::Unknown
        );
    }

    #[test]
    fn parses_the_captured_vmstat() {
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("proc/vmstat")).unwrap();
        let vmstat = parse_vmstat(&raw).unwrap();
        assert_eq!(vmstat["pgmajfault"], 24_228);
        assert_eq!(vmstat["pswpin"], 0);
    }

    #[test]
    fn a_truncated_vmstat_line_is_malformed() {
        let error = parse_vmstat("pgpgin 1\npgpgout\n").unwrap_err();
        assert!(error.detail.contains("has no value"));
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = MemoryCollector::descriptors();
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
