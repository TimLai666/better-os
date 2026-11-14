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

pub mod battery;
pub mod catalog;
pub mod cpu;
pub mod energy;
pub mod fsread;
pub mod gpu;
pub mod hwmon;
pub mod memory;
pub mod netproc;
pub mod network;
pub mod npu;
pub mod overhead;
pub mod pressure;
pub mod process;
pub mod roots;
pub mod storage;
#[cfg(test)]
mod test_support;

pub use battery::BatteryCollector;
pub use cpu::CpuCollector;
pub use gpu::GpuCollector;
pub use hwmon::SensorsCollector;
pub use memory::MemoryCollector;
pub use netproc::ProcessUsageCollector;
pub use network::NetworkCollector;
pub use npu::NpuCollector;
pub use overhead::{OverheadReport, measure};
pub use pressure::PressureCollector;
pub use process::{ProcessCollector, ProcessPrivacy};
pub use roots::{Roots, SocketSource};
pub use storage::StorageCollector;

use monitor_core::{Collector, CollectorReport, MetricDescriptor, Timestamp};

/// Every Linux collector, sampled together.
///
/// The order is fixed so a round's reports are comparable across runs and so
/// the overhead measurement can attribute cost to a stable index. The process
/// usage scan runs after the process table, because it reads the processes
/// that table found, and before the graphics and NPU collectors, because an
/// Intel card's busy share is the sum of its clients.
pub struct LinuxCollectors {
    cpu: CpuCollector,
    memory: MemoryCollector,
    pressure: PressureCollector,
    process: ProcessCollector,
    storage: StorageCollector,
    network: NetworkCollector,
    usage: ProcessUsageCollector,
    gpu: GpuCollector,
    npu: NpuCollector,
    battery: BatteryCollector,
    sensors: SensorsCollector,
    /// The process table of the last round, which the usage scan reads.
    last_pids: Vec<u32>,
}

impl LinuxCollectors {
    pub fn new(roots: Roots, privacy: ProcessPrivacy) -> Self {
        Self {
            cpu: CpuCollector::new(roots.clone()),
            memory: MemoryCollector::new(roots.clone()),
            pressure: PressureCollector::new(roots.clone()),
            process: ProcessCollector::new(roots.clone(), privacy),
            storage: StorageCollector::new(roots.clone()),
            network: NetworkCollector::new(roots.clone()),
            usage: ProcessUsageCollector::new(roots.clone()),
            gpu: GpuCollector::new(roots.clone()),
            npu: NpuCollector::new(roots.clone()),
            battery: BatteryCollector::new(roots.clone()),
            sensors: SensorsCollector::new(roots),
            last_pids: Vec::new(),
        }
    }

    /// The collector names, in sampling order.
    pub fn collector_names() -> [&'static str; 11] {
        [
            "linux.cpu",
            "linux.memory",
            "linux.pressure",
            "linux.process",
            "linux.storage",
            "linux.network",
            netproc::PROCESS_USAGE_COLLECTOR,
            gpu::GPU_COLLECTOR,
            npu::NPU_COLLECTOR,
            battery::BATTERY_COLLECTOR,
            hwmon::SENSORS_COLLECTOR,
        ]
    }

    /// Every metric any Linux collector can emit.
    pub fn descriptors() -> Vec<MetricDescriptor> {
        let mut descriptors = CpuCollector::descriptors();
        descriptors.extend(MemoryCollector::descriptors());
        descriptors.extend(PressureCollector::descriptors());
        descriptors.extend(ProcessCollector::descriptors());
        descriptors.extend(energy::descriptors());
        descriptors.extend(StorageCollector::descriptors());
        descriptors.extend(NetworkCollector::descriptors());
        descriptors.extend(ProcessUsageCollector::descriptors());
        descriptors.extend(GpuCollector::descriptors());
        descriptors.extend(NpuCollector::descriptors());
        descriptors.extend(BatteryCollector::descriptors());
        descriptors.extend(SensorsCollector::descriptors());
        descriptors
    }

    /// The metrics one collector declares, by its name.
    pub fn descriptors_of(collector: &str) -> Vec<MetricDescriptor> {
        match collector {
            "linux.cpu" => CpuCollector::descriptors(),
            "linux.memory" => MemoryCollector::descriptors(),
            "linux.pressure" => PressureCollector::descriptors(),
            "linux.process" => {
                let mut process = ProcessCollector::descriptors();
                // The process table carries what the usage scan and the energy
                // estimate add to its entities.
                process.extend(
                    ProcessUsageCollector::descriptors()
                        .into_iter()
                        .filter(|d| {
                            d.id.as_str().starts_with("process.")
                                && !d.id.as_str().starts_with("process.usage.")
                        }),
                );
                process.extend(energy::descriptors());
                process
            }
            "linux.storage" => StorageCollector::descriptors(),
            "linux.network" => NetworkCollector::descriptors(),
            netproc::PROCESS_USAGE_COLLECTOR => ProcessUsageCollector::descriptors()
                .into_iter()
                .filter(|d| d.id.as_str().starts_with("process.usage."))
                .collect(),
            gpu::GPU_COLLECTOR => GpuCollector::descriptors(),
            npu::NPU_COLLECTOR => NpuCollector::descriptors(),
            battery::BATTERY_COLLECTOR => BatteryCollector::descriptors(),
            hwmon::SENSORS_COLLECTOR => SensorsCollector::descriptors(),
            _ => Vec::new(),
        }
    }

    /// Sample everything against the given roots at one timestamp.
    ///
    /// All reports share a timestamp so a chart can line them up without
    /// interpolating, which the specification requires of irregular sampling.
    /// What each process does with the network, the graphics card, the NPU,
    /// and the battery is written into the process table's own entities, so
    /// a process is one entity however many collectors described it.
    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> Vec<CollectorReport> {
        let mut reports: Vec<CollectorReport> = (0..Self::collector_names().len())
            .map(|index| self.sample_one(index, roots, at))
            .collect();
        let battery = reports
            .iter()
            .position(|r| r.collector.as_str() == battery::BATTERY_COLLECTOR)
            .map(|index| reports[index].clone());
        if let Some(process) = reports
            .iter_mut()
            .find(|r| r.collector.as_str() == "linux.process")
        {
            self.usage.enrich(process);
            energy::estimate(process, battery.as_ref());
        }
        reports
    }

    /// Sample one collector by its index in [`LinuxCollectors::collector_names`].
    pub fn sample_one(&mut self, index: usize, roots: &Roots, at: Timestamp) -> CollectorReport {
        match index {
            0 => self.cpu.sample(roots, at),
            1 => self.memory.sample(roots, at),
            2 => self.pressure.sample(roots, at),
            3 => {
                let report = self.process.sample(roots, at);
                self.last_pids = netproc::pids_of(&report);
                report
            }
            4 => self.storage.sample(roots, at),
            5 => self.network.sample(roots, at),
            6 => {
                let pids = std::mem::take(&mut self.last_pids);
                let report = self.usage.sample_for(roots, at, &pids);
                self.last_pids = pids;
                report
            }
            7 => self.gpu.sample_with(roots, at, Some(self.usage.latest())),
            8 => self.npu.sample_with(roots, at, Some(self.usage.latest())),
            9 => self.battery.sample(roots, at),
            10 => self.sensors.sample(roots, at),
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
            &mut self.usage,
            &mut self.gpu,
            &mut self.npu,
            &mut self.battery,
            &mut self.sensors,
        ]
    }
}
