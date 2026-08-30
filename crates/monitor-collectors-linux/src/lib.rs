//! Linux collectors for Better Monitor.
//!
//! Every reading here comes from `/proc` or `/sys` directly. No collector runs
//! a command or parses a human-formatted tool's output, because the kernel
//! already publishes a stable structured interface for everything in this
//! crate and a formatted line is a moving target.
//!
//! Nothing opens a hardcoded path. Every read goes through [`Roots`], so a
//! test runs the production code path against a captured snapshot of a real
//! machine's `/proc` and `/sys`.
//!
//! ## On the `sysinfo` crate
//!
//! The specification names `sysinfo` as the first dependency candidate for
//! baseline portable data, and it was evaluated for this crate. It is not
//! adopted here, for reasons specific to what this ticket collects rather than
//! any judgement about the crate. Everything in scope — the eight kernel CPU
//! time categories, PSI, `/proc/vmstat` paging counters, `/proc/diskstats`
//! queue and service times, per-process cgroup paths and descriptor counts —
//! is either absent from a portable abstraction or flattened by it, and the
//! five-way observation state this crate is built around has no equivalent
//! there: a portable API reports zero where the interface is missing. Running
//! both would also mean two sampling cadences over the same counters, which is
//! exactly how two parts of a UI come to disagree.
//!
//! That evaluation covers this ticket's scope only. Battery, component
//! temperature naming, and disk identity are plausible places to revisit it,
//! and adopting it would need a dependency decision recorded in the usual way.

pub mod catalog;
pub mod cpu;
pub mod fsread;
pub mod memory;
pub mod network;
pub mod overhead;
pub mod pressure;
pub mod process;
pub mod roots;
pub mod storage;
#[cfg(test)]
mod test_support;

pub use cpu::CpuCollector;
pub use memory::MemoryCollector;
pub use network::NetworkCollector;
pub use overhead::{OverheadReport, measure};
pub use pressure::PressureCollector;
pub use process::{ProcessCollector, ProcessPrivacy};
pub use roots::Roots;
pub use storage::StorageCollector;

use monitor_core::{Collector, CollectorReport, MetricDescriptor, Timestamp};

/// Every Linux collector, sampled together.
///
/// The order is fixed so a round's reports are comparable across runs and so
/// the overhead measurement can attribute cost to a stable index.
pub struct LinuxCollectors {
    cpu: CpuCollector,
    memory: MemoryCollector,
    pressure: PressureCollector,
    process: ProcessCollector,
    storage: StorageCollector,
    network: NetworkCollector,
}

impl LinuxCollectors {
    pub fn new(roots: Roots, privacy: ProcessPrivacy) -> Self {
        Self {
            cpu: CpuCollector::new(roots.clone()),
            memory: MemoryCollector::new(roots.clone()),
            pressure: PressureCollector::new(roots.clone()),
            process: ProcessCollector::new(roots.clone(), privacy),
            storage: StorageCollector::new(roots.clone()),
            network: NetworkCollector::new(roots),
        }
    }

    /// The collector names, in sampling order.
    pub fn collector_names() -> [&'static str; 6] {
        [
            "linux.cpu",
            "linux.memory",
            "linux.pressure",
            "linux.process",
            "linux.storage",
            "linux.network",
        ]
    }

    /// Every metric any Linux collector can emit.
    pub fn descriptors() -> Vec<MetricDescriptor> {
        let mut descriptors = CpuCollector::descriptors();
        descriptors.extend(MemoryCollector::descriptors());
        descriptors.extend(PressureCollector::descriptors());
        descriptors.extend(ProcessCollector::descriptors());
        descriptors.extend(StorageCollector::descriptors());
        descriptors.extend(NetworkCollector::descriptors());
        descriptors
    }

    /// Sample everything against the given roots at one timestamp.
    ///
    /// All six reports share a timestamp so a chart can line them up without
    /// interpolating, which the specification requires of irregular sampling.
    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> Vec<CollectorReport> {
        (0..Self::collector_names().len())
            .map(|index| self.sample_one(index, roots, at))
            .collect()
    }

    /// Sample one collector by its index in [`LinuxCollectors::collector_names`].
    pub fn sample_one(&mut self, index: usize, roots: &Roots, at: Timestamp) -> CollectorReport {
        match index {
            0 => self.cpu.sample(roots, at),
            1 => self.memory.sample(roots, at),
            2 => self.pressure.sample(roots, at),
            3 => self.process.sample(roots, at),
            4 => self.storage.sample(roots, at),
            5 => self.network.sample(roots, at),
            other => panic!("no collector at index {other}"),
        }
    }

    /// The collectors as trait objects, for a caller that drives them through
    /// the shared contract and does not care that they are Linux-specific.
    pub fn as_collectors(&mut self) -> Vec<&mut dyn Collector> {
        vec![
            &mut self.cpu,
            &mut self.memory,
            &mut self.pressure,
            &mut self.process,
            &mut self.storage,
            &mut self.network,
        ]
    }
}
