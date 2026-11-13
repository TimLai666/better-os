//! Neural processing units, from `/sys/class/accel`.
//!
//! An NPU is a compute accelerator the kernel exposes as `/dev/accel/accelN`
//! with its device under `/sys/class/accel/accelN/device`. What each driver
//! publishes differs, and this module reads only what is there:
//!
//! - **amdxdna** (AMD Ryzen AI) names the part in `vbnv` (`RyzenAI-npu4`) and
//!   its firmware in `fw_version`. It publishes no busy counter in sysfs; busy
//!   time and memory come from the DRM client keys in the fdinfo of an open
//!   `/dev/accel` file, which [`crate::netproc`] reads, and only exist while a
//!   program has the NPU open.
//! - **intel_vpu** (Intel NPU) publishes `npu_busy_time_us`, a busy-time
//!   counter, so its busy share is the counter's delta over the interval.
//!
//! Every NPU has a PCI power state (`power_state`, `D0` running, `D3hot`
//! asleep) and runtime power management (`power/runtime_status`, and
//! `power/runtime_active_time` in milliseconds). The share of time it was
//! awake is reported as exactly that: awake, not busy. An NPU that is awake
//! can still be idle.

use crate::catalog::{collector_id, gauge, identity, metric_id, sys_source, utilization};
use crate::fsread::{ReadError, list_dir, read_attribute};
use crate::gpu::{uevent, vendor_name};
use crate::netproc::{DeviceClass, UsageRound};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::HashMap;
use std::path::Path;

pub const NPU_COLLECTOR: &str = "linux.npu";

#[derive(Clone, Copy, Debug, Default)]
struct Counters {
    active_ms: Option<u64>,
    busy_us: Option<u64>,
    at: Option<Timestamp>,
}

/// The NPU collector.
pub struct NpuCollector {
    roots: Roots,
    previous: HashMap<String, Counters>,
}

fn number(path: &Path) -> Option<u64> {
    read_attribute(path).ok()?.trim().parse().ok()
}

impl NpuCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            previous: HashMap::new(),
        }
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        let source = || sys_source("class/accel/accelN/device");
        vec![
            identity("npu.device", source(), "the kernel's accel device name"),
            identity("npu.driver", source(), "the kernel driver bound to the NPU"),
            identity("npu.vendor", source(), "the vendor, from the PCI vendor id"),
            identity(
                "npu.model",
                source(),
                "vbnv: the part's name, where published",
            ),
            identity("npu.firmware", source(), "fw_version, where published"),
            identity(
                "npu.power_state",
                source(),
                "the PCI power state: D0 running, D3hot asleep",
            ),
            identity(
                "npu.runtime_status",
                source(),
                "runtime power management: active or suspended",
            ),
            utilization(
                "npu.awake",
                source(),
                "power/runtime_active_time delta: the share of time it was powered up",
            ),
            utilization(
                "npu.utilization",
                source(),
                "npu_busy_time_us delta, or the busiest engine of its open clients",
            ),
            identity(
                "npu.utilization.source",
                source(),
                "driver or clients: where the busy share was read",
            ),
            gauge(
                "npu.clients",
                Unit::Count,
                source(),
                "programs this user runs that have the NPU open",
            ),
        ]
    }

    pub fn sample_with(
        &mut self,
        roots: &Roots,
        at: Timestamp,
        usage: Option<&UsageRound>,
    ) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(NPU_COLLECTOR), at);
        let devices = match list_dir(&roots.sys("class/accel")) {
            Ok(devices) => devices,
            Err(ReadError::Missing { path }) => {
                report.health = CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
                    path: path.display().to_string(),
                });
                return report;
            }
            Err(other) => {
                report.health = CollectorHealth::Failed {
                    detail: other.path().display().to_string(),
                };
                return report;
            }
        };
        for dir in devices {
            let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                continue;
            };
            if !name.starts_with("accel") {
                continue;
            }
            let device = dir.join("device");
            let address = uevent(&device, "PCI_SLOT_NAME").unwrap_or_else(|| name.clone());
            let driver = std::fs::read_link(device.join("driver"))
                .ok()
                .and_then(|t| t.file_name()?.to_str().map(str::to_string))
                .or_else(|| uevent(&device, "DRIVER"));
            let mut m = MetricSet::new();
            m.insert(metric_id("npu.device"), Observation::text(name.clone()));
            if let Some(driver) = &driver {
                m.insert(metric_id("npu.driver"), Observation::text(driver.clone()));
            }
            if let Ok(vendor) = read_attribute(&device.join("vendor")) {
                m.insert(
                    metric_id("npu.vendor"),
                    Observation::text(vendor_name(&vendor).unwrap_or(&vendor)),
                );
            }
            for (id, file) in [
                ("npu.model", "vbnv"),
                ("npu.firmware", "fw_version"),
                ("npu.power_state", "power_state"),
                ("npu.runtime_status", "power/runtime_status"),
            ] {
                if let Ok(value) = read_attribute(&device.join(file))
                    && !value.is_empty()
                {
                    m.insert(metric_id(id), Observation::text(value));
                }
            }

            let now = Counters {
                active_ms: number(&device.join("power/runtime_active_time")),
                busy_us: number(&device.join("npu_busy_time_us")),
                at: Some(at),
            };
            let before = self
                .previous
                .insert(address.clone(), now)
                .unwrap_or_default();
            let seconds = before
                .at
                .and_then(|then| Timestamp::interval_seconds(then, at))
                .filter(|s| *s > 0.0);
            if now.active_ms.is_some() {
                m.insert(
                    metric_id("npu.awake"),
                    match (now.active_ms, before.active_ms, seconds) {
                        (Some(now), Some(then), Some(seconds)) => Observation::float(
                            (now.saturating_sub(then) as f64 / 1000.0 / seconds).clamp(0.0, 1.0),
                        ),
                        _ => Observation::Unknown(UnknownReason::NotYetSampled),
                    },
                );
            }

            let clients = usage
                .and_then(|u| u.devices.get(&address))
                .filter(|d| d.class == Some(DeviceClass::Accelerator));
            m.insert(
                metric_id("npu.clients"),
                Observation::unsigned(clients.map(|c| c.clients as u64).unwrap_or(0)),
            );
            let utilization = if now.busy_us.is_some() {
                m.insert(
                    metric_id("npu.utilization.source"),
                    Observation::text("driver"),
                );
                match (now.busy_us, before.busy_us, seconds) {
                    (Some(now), Some(then), Some(seconds)) => Observation::float(
                        (now.saturating_sub(then) as f64 / 1e6 / seconds).clamp(0.0, 1.0),
                    ),
                    _ => Observation::Unknown(UnknownReason::NotYetSampled),
                }
            } else {
                match (clients, usage) {
                    (Some(clients), _) => match clients.busy {
                        Some(busy) => {
                            m.insert(
                                metric_id("npu.utilization.source"),
                                Observation::text("clients"),
                            );
                            Observation::float(busy)
                        }
                        None => Observation::Unsupported(UnsupportedReason::NotReported {
                            detail: "the driver reports no busy time for the programs using it"
                                .into(),
                        }),
                    },
                    (None, Some(round))
                        if round.has_interval
                            && driver
                                .as_deref()
                                .is_some_and(crate::netproc::reports_engine_time) =>
                    {
                        // Nothing has it open, so nothing is making it busy.
                        m.insert(
                            metric_id("npu.utilization.source"),
                            Observation::text("clients"),
                        );
                        Observation::float(0.0)
                    }
                    (None, Some(round)) if !round.has_interval => {
                        Observation::Unknown(UnknownReason::NotYetSampled)
                    }
                    (None, Some(_)) => Observation::Unsupported(UnsupportedReason::NotReported {
                        detail: "the driver publishes no busy time".into(),
                    }),
                    (None, None) => Observation::Unsupported(UnsupportedReason::NotReported {
                        detail: "the driver publishes no busy counter".into(),
                    }),
                }
            };
            m.insert(metric_id("npu.utilization"), utilization);
            report
                .entities
                .push(Entity::new(EntityId::new(EntityKind::Npu, address), m));
        }
        report
    }
}

impl Collector for NpuCollector {
    fn id(&self) -> CollectorId {
        collector_id(NPU_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        NpuCollector::descriptors()
    }

    fn collect(&mut self, at: Timestamp) -> CollectorReport {
        let roots = self.roots.clone();
        self.sample_with(&roots, at, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = NpuCollector::descriptors();
        let mut ids: Vec<_> = descriptors.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), descriptors.len());
    }
}
