//! Block device throughput.
//!
//! Upstream interface: `/proc/diskstats`, documented in the kernel's
//! `Documentation/admin-guide/iostats.rst` (docs.kernel.org build 7.2.0),
//! which defines the 17 counters that follow the major, minor, and device
//! name.
//!
//! Three interpretations are recorded here because the file does not state
//! them. Sector counts are always 512-byte units in this interface regardless
//! of the device's logical block size, so the byte rates multiply by 512 and
//! not by `queue/logical_block_size`. The `io_ticks` counter is milliseconds
//! during which the queue was non-empty, so dividing it by the elapsed wall
//! time gives the device utilization `iostat` calls `%util`. And the weighted
//! time counter divided by elapsed time gives the mean number of requests in
//! flight, which is queue depth rather than a duration.
//!
//! Partitions are excluded because they are not devices: `nvme0n1p6`'s traffic
//! is already counted in `nvme0n1`, and adding both would double the machine's
//! apparent I/O. The filter is membership in `/sys/block`, which partitions
//! are not part of, plus an exclusion list for virtual devices that back files
//! rather than hardware.

use crate::catalog::{
    MINIMUM_DELTA_INTERVAL, collector_id, derived_source, gauge, identity, latency, metric_id,
    proc_source, rate, saturation, sys_source,
};
use crate::fsread::{MalformedInput, field_u64, read_attribute, read_text, read_u64_attribute};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::BTreeMap;

/// The `/proc/diskstats` and `/sys/block/*/size` sector unit. Fixed at 512
/// bytes by the interface, not by the hardware.
pub const DISKSTATS_SECTOR_BYTES: u64 = 512;

/// Name prefixes for devices that are not hardware. Device-mapper (`dm-`) is
/// deliberately not here: an encrypted or LVM volume is where a user's real
/// I/O goes.
const VIRTUAL_DEVICE_PREFIXES: [&str; 4] = ["loop", "ram", "zram", "fd"];

/// One `/proc/diskstats` line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiskStats {
    pub major: u64,
    pub minor: u64,
    pub name: String,
    pub reads_completed: u64,
    pub reads_merged: u64,
    pub sectors_read: u64,
    pub read_milliseconds: u64,
    pub writes_completed: u64,
    pub writes_merged: u64,
    pub sectors_written: u64,
    pub write_milliseconds: u64,
    pub in_flight: u64,
    pub io_milliseconds: u64,
    pub weighted_io_milliseconds: u64,
    /// Present since 4.18. `None` on an older kernel, which is not the same as
    /// a device that never discards.
    pub discards_completed: Option<u64>,
    pub sectors_discarded: Option<u64>,
    pub discard_milliseconds: Option<u64>,
    /// Present since 5.5, and never tracked for partitions.
    pub flushes_completed: Option<u64>,
    pub flush_milliseconds: Option<u64>,
}

const PROC_DISKSTATS: &str = "/proc/diskstats";

/// Parse `/proc/diskstats`.
///
/// A line must carry the eleven counters every kernel since 2.6 has had. The
/// discard and flush groups are optional because kernels before 4.18 and 5.5
/// respectively do not emit them, and a missing counter must read as unknown
/// rather than as zero discards.
pub fn parse_diskstats(input: &str) -> Result<Vec<DiskStats>, MalformedInput> {
    let mut devices = Vec::new();
    for line in input.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 14 {
            return Err(MalformedInput::new(
                PROC_DISKSTATS,
                format!("line has {} of at least 14 fields: {line:?}", fields.len()),
            ));
        }
        let at = |index: usize| field_u64(PROC_DISKSTATS, &fields, index);
        let optional = |index: usize| -> Result<Option<u64>, MalformedInput> {
            if index < fields.len() {
                at(index).map(Some)
            } else {
                Ok(None)
            }
        };
        devices.push(DiskStats {
            major: at(0)?,
            minor: at(1)?,
            name: fields[2].to_string(),
            reads_completed: at(3)?,
            reads_merged: at(4)?,
            sectors_read: at(5)?,
            read_milliseconds: at(6)?,
            writes_completed: at(7)?,
            writes_merged: at(8)?,
            sectors_written: at(9)?,
            write_milliseconds: at(10)?,
            in_flight: at(11)?,
            io_milliseconds: at(12)?,
            weighted_io_milliseconds: at(13)?,
            discards_completed: optional(14)?,
            sectors_discarded: optional(16)?,
            discard_milliseconds: optional(17)?,
            flushes_completed: optional(18)?,
            flush_milliseconds: optional(19)?,
        });
    }
    if devices.is_empty() {
        return Err(MalformedInput::new(PROC_DISKSTATS, "no device lines"));
    }
    Ok(devices)
}

/// Whether a `/proc/diskstats` entry names a whole hardware block device.
pub fn is_real_block_device(roots: &Roots, name: &str) -> bool {
    if VIRTUAL_DEVICE_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return false;
    }
    // Partitions live under their parent in /sys/class/block, never directly
    // under /sys/block.
    roots.sys(&format!("block/{name}")).is_dir()
}

const STORAGE_COLLECTOR: &str = "linux.storage";

struct StorageSnapshot {
    at: Timestamp,
    devices: BTreeMap<String, DiskStats>,
}

/// Per-device read, write, discard, and flush activity.
pub struct StorageCollector {
    roots: Roots,
    previous: Option<StorageSnapshot>,
}

impl StorageCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            previous: None,
        }
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        vec![
            rate(
                "storage.read.bytes.rate",
                Unit::BytesPerSecond,
                proc_source("diskstats"),
                "sectors read per second times the 512-byte interface sector",
            ),
            rate(
                "storage.write.bytes.rate",
                Unit::BytesPerSecond,
                proc_source("diskstats"),
                "sectors written per second times the 512-byte interface sector",
            ),
            rate(
                "storage.discard.bytes.rate",
                Unit::BytesPerSecond,
                proc_source("diskstats"),
                "sectors discarded per second, unsupported before kernel 4.18",
            ),
            rate(
                "storage.read.ops.rate",
                Unit::CountPerSecond,
                proc_source("diskstats"),
                "reads completed per second",
            ),
            rate(
                "storage.write.ops.rate",
                Unit::CountPerSecond,
                proc_source("diskstats"),
                "writes completed per second",
            ),
            rate(
                "storage.discard.ops.rate",
                Unit::CountPerSecond,
                proc_source("diskstats"),
                "discards completed per second, unsupported before kernel 4.18",
            ),
            rate(
                "storage.flush.ops.rate",
                Unit::CountPerSecond,
                proc_source("diskstats"),
                "flushes completed per second, unsupported before kernel 5.5",
            ),
            latency(
                "storage.read.latency.mean",
                derived_source("read time delta / reads completed delta"),
                "mean time a read spent in the block layer over the interval",
            ),
            latency(
                "storage.write.latency.mean",
                derived_source("write time delta / writes completed delta"),
                "mean time a write spent in the block layer over the interval",
            ),
            saturation(
                "storage.io.in_flight",
                Unit::Count,
                proc_source("diskstats"),
                "requests issued to the device but not yet completed",
            ),
            saturation(
                "storage.queue.depth.mean",
                Unit::Count,
                derived_source("weighted io time delta / elapsed time"),
                "mean number of requests in flight over the interval",
            ),
            crate::catalog::utilization(
                "storage.utilization",
                derived_source("io time delta / elapsed time"),
                "fraction of the interval the device had at least one request in flight",
            ),
            gauge(
                "storage.capacity",
                Unit::Bytes,
                sys_source("block/{device}/size"),
                "device size, from the 512-byte sector count sysfs reports",
            ),
            gauge(
                "storage.block.size.logical",
                Unit::Bytes,
                sys_source("block/{device}/queue/logical_block_size"),
                "logical block size the device advertises",
            ),
            gauge(
                "storage.rotational",
                Unit::None,
                sys_source("block/{device}/queue/rotational"),
                "whether the kernel treats the device as spinning media",
            ),
            identity(
                "storage.model",
                sys_source("block/{device}/device/model"),
                "device model string, where the driver publishes one",
            ),
        ]
    }

    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(STORAGE_COLLECTOR), at);
        let devices = match read_text(&roots.proc("diskstats")).map(|raw| parse_diskstats(&raw)) {
            Ok(Ok(devices)) => devices,
            Ok(Err(error)) => {
                report.health = CollectorHealth::Failed {
                    detail: format!("{}: {}", error.context, error.detail),
                };
                return report;
            }
            Err(error) => {
                report.health = CollectorHealth::Failed {
                    detail: format!("{} unreadable", error.path().display()),
                };
                return report;
            }
        };

        let seconds = self
            .previous
            .as_ref()
            .and_then(|previous| Timestamp::interval_seconds(previous.at, at))
            .filter(|seconds| *seconds >= MINIMUM_DELTA_INTERVAL.as_secs_f64());

        let mut kept = BTreeMap::new();
        for device in devices {
            if !is_real_block_device(roots, &device.name) {
                continue;
            }
            let earlier = self
                .previous
                .as_ref()
                .and_then(|previous| previous.devices.get(&device.name));
            let mut metrics = MetricSet::new();
            self.report_rates(earlier, &device, seconds, &mut metrics);
            read_device_sysfs(roots, &device.name, &mut metrics);
            report.entities.push(Entity::new(
                EntityId::new(EntityKind::BlockDevice, device.name.clone()),
                metrics,
            ));
            kept.insert(device.name.clone(), device);
        }

        if kept.is_empty() {
            report.health = CollectorHealth::Degraded {
                detail: "no hardware block device in /proc/diskstats".into(),
            };
        }
        self.previous = Some(StorageSnapshot { at, devices: kept });
        report
    }

    fn report_rates(
        &self,
        earlier: Option<&DiskStats>,
        later: &DiskStats,
        seconds: Option<f64>,
        metrics: &mut MetricSet,
    ) {
        metrics.insert(
            metric_id("storage.io.in_flight"),
            Observation::unsigned(later.in_flight),
        );

        let Some((earlier, seconds)) = earlier.zip(seconds) else {
            let reason = if earlier.is_none() {
                UnknownReason::NotYetSampled
            } else {
                UnknownReason::IntervalTooShort
            };
            for id in [
                "storage.read.bytes.rate",
                "storage.write.bytes.rate",
                "storage.discard.bytes.rate",
                "storage.read.ops.rate",
                "storage.write.ops.rate",
                "storage.discard.ops.rate",
                "storage.flush.ops.rate",
                "storage.read.latency.mean",
                "storage.write.latency.mean",
                "storage.queue.depth.mean",
                "storage.utilization",
            ] {
                metrics.insert(metric_id(id), Observation::Unknown(reason.clone()));
            }
            return;
        };

        let per_second = |later: u64, earlier: u64| later.saturating_sub(earlier) as f64 / seconds;

        for (id, later_value, earlier_value, scale) in [
            (
                "storage.read.bytes.rate",
                later.sectors_read,
                earlier.sectors_read,
                DISKSTATS_SECTOR_BYTES as f64,
            ),
            (
                "storage.write.bytes.rate",
                later.sectors_written,
                earlier.sectors_written,
                DISKSTATS_SECTOR_BYTES as f64,
            ),
            (
                "storage.read.ops.rate",
                later.reads_completed,
                earlier.reads_completed,
                1.0,
            ),
            (
                "storage.write.ops.rate",
                later.writes_completed,
                earlier.writes_completed,
                1.0,
            ),
        ] {
            metrics.insert(
                metric_id(id),
                Observation::float(per_second(later_value, earlier_value) * scale),
            );
        }

        for (id, later_value, earlier_value, scale) in [
            (
                "storage.discard.bytes.rate",
                later.sectors_discarded,
                earlier.sectors_discarded,
                DISKSTATS_SECTOR_BYTES as f64,
            ),
            (
                "storage.discard.ops.rate",
                later.discards_completed,
                earlier.discards_completed,
                1.0,
            ),
            (
                "storage.flush.ops.rate",
                later.flushes_completed,
                earlier.flushes_completed,
                1.0,
            ),
        ] {
            let observation = match (later_value, earlier_value) {
                (Some(later_value), Some(earlier_value)) => {
                    Observation::float(per_second(later_value, earlier_value) * scale)
                }
                _ => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "counter absent from /proc/diskstats on this kernel".into(),
                }),
            };
            metrics.insert(metric_id(id), observation);
        }

        // Mean service time: total time attributed to completed requests
        // divided by how many completed. With no completions the mean is
        // undefined, which is not the same as a latency of zero.
        for (id, time_later, time_earlier, ops_later, ops_earlier) in [
            (
                "storage.read.latency.mean",
                later.read_milliseconds,
                earlier.read_milliseconds,
                later.reads_completed,
                earlier.reads_completed,
            ),
            (
                "storage.write.latency.mean",
                later.write_milliseconds,
                earlier.write_milliseconds,
                later.writes_completed,
                earlier.writes_completed,
            ),
        ] {
            let operations = ops_later.saturating_sub(ops_earlier);
            let observation = if operations == 0 {
                Observation::Unknown(UnknownReason::IntervalTooShort)
            } else {
                let milliseconds = time_later.saturating_sub(time_earlier) as f64;
                Observation::float(milliseconds / operations as f64)
            };
            metrics.insert(metric_id(id), observation);
        }

        let elapsed_milliseconds = seconds * 1000.0;
        metrics.insert(
            metric_id("storage.utilization"),
            Observation::float(
                later
                    .io_milliseconds
                    .saturating_sub(earlier.io_milliseconds) as f64
                    / elapsed_milliseconds,
            ),
        );
        metrics.insert(
            metric_id("storage.queue.depth.mean"),
            Observation::float(
                later
                    .weighted_io_milliseconds
                    .saturating_sub(earlier.weighted_io_milliseconds) as f64
                    / elapsed_milliseconds,
            ),
        );
    }
}

fn read_device_sysfs(roots: &Roots, name: &str, metrics: &mut MetricSet) {
    let size = roots.sys(&format!("block/{name}/size"));
    metrics.insert(
        metric_id("storage.capacity"),
        match read_u64_attribute(&size) {
            Ok(sectors) => Observation::unsigned(sectors.saturating_mul(DISKSTATS_SECTOR_BYTES)),
            Err(error) => error.into_observation(),
        },
    );
    let block_size = roots.sys(&format!("block/{name}/queue/logical_block_size"));
    metrics.insert(
        metric_id("storage.block.size.logical"),
        match read_u64_attribute(&block_size) {
            Ok(bytes) => Observation::unsigned(bytes),
            Err(error) => error.into_observation(),
        },
    );
    let rotational = roots.sys(&format!("block/{name}/queue/rotational"));
    metrics.insert(
        metric_id("storage.rotational"),
        match read_u64_attribute(&rotational) {
            Ok(value) => Observation::boolean(value != 0),
            Err(error) => error.into_observation(),
        },
    );
    let model = roots.sys(&format!("block/{name}/device/model"));
    metrics.insert(
        metric_id("storage.model"),
        match read_attribute(&model) {
            Ok(value) => Observation::text(value),
            Err(error) => error.into_observation(),
        },
    );
}

impl Collector for StorageCollector {
    fn id(&self) -> CollectorId {
        collector_id(STORAGE_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        StorageCollector::descriptors()
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

    fn device<'a>(report: &'a CollectorReport, name: &str) -> &'a Entity {
        report
            .entities
            .iter()
            .find(|entity| entity.id.key == name)
            .unwrap_or_else(|| panic!("no device {name} in the report"))
    }

    #[test]
    fn parses_all_seventeen_counters_of_a_captured_diskstats_line() {
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("proc/diskstats")).unwrap();
        let devices = parse_diskstats(&raw).unwrap();
        let nvme = devices
            .iter()
            .find(|device| device.name == "nvme0n1")
            .unwrap();
        assert_eq!(nvme.major, 259);
        assert_eq!(nvme.reads_completed, 61_746);
        assert_eq!(nvme.sectors_read, 6_199_131);
        assert_eq!(nvme.writes_completed, 157_140);
        assert_eq!(nvme.io_milliseconds, 26_068);
        assert_eq!(nvme.weighted_io_milliseconds, 784_731);
        assert_eq!(nvme.flushes_completed, Some(1319));
        assert_eq!(nvme.flush_milliseconds, Some(1283));
    }

    #[test]
    fn a_kernel_without_the_discard_counters_reports_them_as_absent() {
        let devices = parse_diskstats(" 8 0 sda 1 2 3 4 5 6 7 8 9 10 11\n").unwrap();
        assert_eq!(devices[0].weighted_io_milliseconds, 11);
        assert_eq!(devices[0].discards_completed, None);
        assert_eq!(devices[0].flushes_completed, None);
    }

    #[test]
    fn a_truncated_diskstats_line_is_malformed() {
        let error = parse_diskstats(" 259 0 nvme0n1 61746 17990 6199131\n").unwrap_err();
        assert!(error.detail.contains("of at least 14 fields"));
    }

    #[test]
    fn a_malformed_diskstats_counter_is_rejected() {
        let error = parse_diskstats(" 259 0 nvme0n1 many 1 2 3 4 5 6 7 8 9 10\n").unwrap_err();
        assert!(error.detail.contains("not a number"));
    }

    #[test]
    fn partitions_and_loop_devices_are_left_out_of_the_report() {
        let roots = Roots::at(fixture("snapshot-a"));
        let mut collector = StorageCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        let names: Vec<&str> = report
            .entities
            .iter()
            .map(|entity| entity.id.key.as_str())
            .collect();
        // The captured host has eight loop devices, one NVMe disk, and six
        // partitions of it.
        assert_eq!(names, vec!["nvme0n1"]);
    }

    #[test]
    fn a_device_mapper_volume_is_treated_as_real_storage() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::create_dir_all(temporary.path().join("sys/block/dm-0/queue")).unwrap();
        std::fs::write(
            temporary.path().join("sys/block/dm-0/queue/rotational"),
            "0\n",
        )
        .unwrap();
        assert!(is_real_block_device(&temporary.roots(), "dm-0"));
        assert!(!is_real_block_device(&temporary.roots(), "loop0"));
        assert!(!is_real_block_device(&temporary.roots(), "sda1"));
    }

    #[test]
    fn sectors_are_five_hundred_and_twelve_bytes_regardless_of_the_block_size() {
        // sda advertises a 512-byte logical block, and gains 2048 read sectors
        // over one second: exactly 1 MiB per second.
        let a = Roots::at(fixture("synthetic-a"));
        let b = Roots::at(fixture("synthetic-b"));
        let mut collector = StorageCollector::new(a.clone());
        collector.sample(&a, at(0));
        let report = collector.sample(&b, at(1_000));
        let sda = device(&report, "sda");
        let read = sda
            .metrics
            .get(&metric_id("storage.read.bytes.rate"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((read - 1024.0 * 1024.0).abs() < 1e-6);
        let written = sda
            .metrics
            .get(&metric_id("storage.write.bytes.rate"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((written - 512.0 * 1024.0).abs() < 1e-6);
    }

    #[test]
    fn latency_is_service_time_per_completed_request() {
        // 200 ms of read time over 100 completed reads.
        let a = Roots::at(fixture("synthetic-a"));
        let b = Roots::at(fixture("synthetic-b"));
        let mut collector = StorageCollector::new(a.clone());
        collector.sample(&a, at(0));
        let report = collector.sample(&b, at(1_000));
        let sda = device(&report, "sda");
        let latency = sda
            .metrics
            .get(&metric_id("storage.read.latency.mean"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((latency - 2.0).abs() < 1e-9);
    }

    #[test]
    fn utilization_and_queue_depth_come_from_the_two_time_counters() {
        // io_milliseconds 2000 -> 2500 over one second is 50% utilization;
        // weighted 3000 -> 4000 is a mean depth of one.
        let a = Roots::at(fixture("synthetic-a"));
        let b = Roots::at(fixture("synthetic-b"));
        let mut collector = StorageCollector::new(a.clone());
        collector.sample(&a, at(0));
        let report = collector.sample(&b, at(1_000));
        let sda = device(&report, "sda");
        let utilization = sda
            .metrics
            .get(&metric_id("storage.utilization"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((utilization - 0.5).abs() < 1e-9);
        let depth = sda
            .metrics
            .get(&metric_id("storage.queue.depth.mean"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((depth - 1.0).abs() < 1e-9);
        assert_eq!(
            sda.metrics
                .get(&metric_id("storage.io.in_flight"))
                .unwrap()
                .as_f64(),
            Some(2.0)
        );
    }

    #[test]
    fn an_interval_with_no_completed_reads_has_no_latency_rather_than_zero() {
        let a = Roots::at(fixture("synthetic-a"));
        let mut collector = StorageCollector::new(a.clone());
        collector.sample(&a, at(0));
        // Sampling the same tree twice means no counter moved.
        let report = collector.sample(&a, at(1_000));
        let sda = device(&report, "sda");
        assert_eq!(
            sda.metrics
                .state_of(&metric_id("storage.read.latency.mean")),
            ObservationState::Unknown
        );
        // Throughput really is zero over that interval, and says so.
        assert_eq!(
            sda.metrics
                .get(&metric_id("storage.read.bytes.rate"))
                .unwrap()
                .as_f64(),
            Some(0.0)
        );
    }

    #[test]
    fn the_first_round_reports_unknown_throughput() {
        let a = Roots::at(fixture("synthetic-a"));
        let mut collector = StorageCollector::new(a.clone());
        let report = collector.sample(&a, at(0));
        assert_eq!(
            device(&report, "sda")
                .metrics
                .state_of(&metric_id("storage.read.bytes.rate")),
            ObservationState::Unknown
        );
    }

    #[test]
    fn device_attributes_come_from_sysfs_and_are_unsupported_when_absent() {
        let a = Roots::at(fixture("synthetic-a"));
        let mut collector = StorageCollector::new(a.clone());
        let report = collector.sample(&a, at(0));
        let sda = device(&report, "sda");
        // 2048 sectors of 512 bytes.
        assert_eq!(
            sda.metrics
                .get(&metric_id("storage.capacity"))
                .unwrap()
                .as_f64(),
            Some(1_048_576.0)
        );
        assert_eq!(
            sda.metrics
                .get(&metric_id("storage.rotational"))
                .unwrap()
                .as_f64(),
            Some(0.0)
        );
        // The synthetic tree has no device/model file.
        assert_eq!(
            sda.metrics.state_of(&metric_id("storage.model")),
            ObservationState::Unsupported
        );
    }

    #[test]
    fn a_real_nvme_reports_its_model_string() {
        let roots = Roots::at(fixture("snapshot-a"));
        let mut collector = StorageCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert_eq!(
            device(&report, "nvme0n1")
                .metrics
                .state_of(&metric_id("storage.model")),
            ObservationState::Value
        );
    }

    #[test]
    fn a_malformed_diskstats_fails_the_collector() {
        let roots = Roots::at(fixture("malformed"));
        let mut collector = StorageCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert!(matches!(report.health, CollectorHealth::Failed { .. }));
    }

    #[test]
    fn a_truncated_diskstats_fails_the_collector() {
        let roots = Roots::at(fixture("truncated"));
        let mut collector = StorageCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert!(matches!(report.health, CollectorHealth::Failed { .. }));
    }

    #[test]
    fn a_host_with_only_virtual_devices_is_degraded_rather_than_silent() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::write(
            temporary.roots().proc("diskstats"),
            "   7       0 loop0 5 0 40 0 0 0 0 0 0 1 0 0 0 0 0 0 0\n",
        )
        .unwrap();
        let mut collector = StorageCollector::new(temporary.roots());
        let report = collector.sample(&temporary.roots(), at(0));
        assert!(matches!(report.health, CollectorHealth::Degraded { .. }));
        assert!(report.entities.is_empty());
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = StorageCollector::descriptors();
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
