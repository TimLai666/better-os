//! What the Overview and Performance pages draw, decided without a window.
//!
//! A round from the collectors is a set of reports keyed by metric names.
//! [`Readings::from_reports`] turns one round into the handful of facts a
//! person reads (how busy the processor is, how much memory is in use, how
//! fast each disk and network adapter is moving data), and [`LiveHistory`]
//! keeps the last minute of them for the charts. Both run on the update the
//! window adopts, once per round, so drawing never repeats the work and never
//! waits on anything: the collectors run on the link thread and only finished
//! rounds reach the window.
//!
//! A reading that is missing stays missing here (`None`), and the chart
//! leaves a gap for it. It is never turned into a zero.

use std::collections::{BTreeMap, VecDeque};

use monitor_core::{CollectorReport, EntityKind, MetricId, MetricScalar, MetricSet};

/// How many readings a live chart holds: one a second for a minute, as Task
/// Manager shows.
pub(crate) const CAPACITY: usize = 60;

/// A series the history keeps.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum SeriesKey {
    /// Overall processor utilization, in percent.
    Cpu,
    /// One logical processor, in percent.
    Core(u32),
    /// The processor package temperature, in degrees Celsius.
    CpuTemperature,
    /// Memory in use, in bytes.
    Memory,
    /// The fraction of time a disk was busy, in percent.
    DiskActive(String),
    /// Bytes read per second from a disk.
    DiskRead(String),
    /// Bytes written per second to a disk.
    DiskWrite(String),
    /// Bytes received per second on an adapter.
    Receive(String),
    /// Bytes sent per second on an adapter.
    Send(String),
}

fn id(raw: &str) -> MetricId {
    MetricId::new(raw).expect("a chart metric id is well formed")
}

fn scalar<'a>(metrics: &'a MetricSet, raw: &str) -> Option<&'a MetricScalar> {
    metrics.get(&id(raw))?.value()
}

fn number(metrics: &MetricSet, raw: &str) -> Option<f64> {
    scalar(metrics, raw)?
        .as_f64()
        .filter(|value| value.is_finite())
}

fn unsigned(metrics: &MetricSet, raw: &str) -> Option<u64> {
    number(metrics, raw)
        .filter(|value| *value >= 0.0)
        .map(|value| value as u64)
}

fn boolean(metrics: &MetricSet, raw: &str) -> Option<bool> {
    match scalar(metrics, raw)? {
        MetricScalar::Bool(value) => Some(*value),
        MetricScalar::Unsigned(value) => Some(*value != 0),
        _ => None,
    }
}

fn text(metrics: &MetricSet, raw: &str) -> Option<String> {
    scalar(metrics, raw)?
        .as_text()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// The processor, now.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CpuReadings {
    /// A fraction of all logical processors' time, `0.0..=1.0`.
    pub(crate) utilization: Option<f64>,
    /// Each logical processor by its kernel number.
    pub(crate) cores: Vec<CoreReadings>,
    /// The mean current clock of the processors that report one, in hertz.
    pub(crate) speed_hz: Option<f64>,
    /// The highest clock any processor may run at, in hertz.
    pub(crate) maximum_hz: Option<f64>,
    pub(crate) logical: Option<u64>,
    pub(crate) threads: Option<u64>,
    pub(crate) load_1m: Option<f64>,
    /// Seconds since the machine started.
    pub(crate) uptime_seconds: Option<f64>,
    pub(crate) temperature_celsius: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CoreReadings {
    pub(crate) index: u32,
    pub(crate) utilization: Option<f64>,
    pub(crate) speed_hz: Option<f64>,
}

/// Memory, now. Every value is in bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MemoryReadings {
    pub(crate) total: Option<u64>,
    pub(crate) available: Option<u64>,
    pub(crate) free: Option<u64>,
    /// Page cache and buffers: memory holding files, given back on demand.
    pub(crate) cached: Option<u64>,
    /// Changed file data waiting to be written to disk.
    pub(crate) modified: Option<u64>,
    pub(crate) committed: Option<u64>,
    pub(crate) commit_limit: Option<u64>,
    pub(crate) swap_total: Option<u64>,
    pub(crate) swap_used: Option<u64>,
}

impl MemoryReadings {
    /// What programs are using: everything the kernel could not hand out
    /// right now without taking it from someone.
    pub(crate) fn in_use(&self) -> Option<u64> {
        Some(self.total?.saturating_sub(self.available?))
    }

    /// In use as a fraction of all memory.
    pub(crate) fn in_use_fraction(&self) -> Option<f64> {
        let total = self.total.filter(|total| *total > 0)?;
        Some(self.in_use()? as f64 / total as f64)
    }
}

/// One disk, now.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DiskReadings {
    /// The kernel's name for it (`nvme0n1`), which is what a person sees in
    /// every other Linux tool.
    pub(crate) name: String,
    pub(crate) model: Option<String>,
    /// The fraction of time it had work in flight, `0.0..=1.0`.
    pub(crate) active: Option<f64>,
    pub(crate) read_rate: Option<f64>,
    pub(crate) write_rate: Option<f64>,
    /// Mean time a request took, in milliseconds.
    pub(crate) read_latency_ms: Option<f64>,
    pub(crate) write_latency_ms: Option<f64>,
    pub(crate) capacity: Option<u64>,
    pub(crate) rotational: Option<bool>,
}

impl DiskReadings {
    /// The slower of the two directions, which is what a person waits on.
    pub(crate) fn response_ms(&self) -> Option<f64> {
        match (self.read_latency_ms, self.write_latency_ms) {
            (Some(read), Some(write)) => Some(read.max(write)),
            (one, other) => one.or(other),
        }
    }
}

/// What kind of connection an adapter is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdapterKind {
    Wired,
    Wireless,
    Other,
}

/// One network adapter, now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AdapterReadings {
    pub(crate) name: String,
    pub(crate) kind: AdapterKind,
    /// Whether it is connected: a link that is up and, where the kernel says,
    /// has a carrier.
    pub(crate) connected: Option<bool>,
    /// The negotiated link speed in bits per second, where there is one.
    pub(crate) link_speed_bps: Option<f64>,
    pub(crate) receive_rate: Option<f64>,
    pub(crate) send_rate: Option<f64>,
}

/// Everything the live pages draw from one round.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Readings {
    pub(crate) cpu: CpuReadings,
    pub(crate) memory: MemoryReadings,
    pub(crate) disks: Vec<DiskReadings>,
    pub(crate) adapters: Vec<AdapterReadings>,
    /// Whether the round carried each collector at all, so a page can tell
    /// "nothing reported" from "nothing to report".
    pub(crate) has_cpu: bool,
    pub(crate) has_memory: bool,
    pub(crate) has_storage: bool,
    pub(crate) has_network: bool,
}

fn report<'a>(reports: &'a [CollectorReport], collector: &str) -> Option<&'a CollectorReport> {
    reports
        .iter()
        .find(|report| report.collector.as_str() == collector)
}

impl Readings {
    /// Read one round. `now_unix` is the wall clock in seconds, for uptime.
    pub(crate) fn from_reports(reports: &[CollectorReport], now_unix: f64) -> Self {
        let mut readings = Readings::default();
        if let Some(cpu) = report(reports, "linux.cpu") {
            readings.has_cpu = true;
            let metrics = &cpu.metrics;
            let mut cores: Vec<CoreReadings> = cpu
                .entities_of(EntityKind::LogicalCpu)
                .filter_map(|entity| {
                    Some(CoreReadings {
                        index: entity.id.key.parse().ok()?,
                        utilization: number(&entity.metrics, "cpu.utilization.busy"),
                        speed_hz: number(&entity.metrics, "cpu.frequency.current"),
                    })
                })
                .collect();
            cores.sort_by_key(|core| core.index);
            let speeds: Vec<f64> = cores.iter().filter_map(|core| core.speed_hz).collect();
            let maximum = cpu
                .entities_of(EntityKind::LogicalCpu)
                .filter_map(|entity| number(&entity.metrics, "cpu.frequency.max"))
                .fold(None, |peak: Option<f64>, value| {
                    Some(peak.map_or(value, |peak| peak.max(value)))
                });
            readings.cpu = CpuReadings {
                utilization: number(metrics, "cpu.utilization.busy"),
                speed_hz: (!speeds.is_empty())
                    .then(|| speeds.iter().sum::<f64>() / speeds.len() as f64),
                maximum_hz: maximum,
                logical: unsigned(metrics, "cpu.logical.count"),
                threads: unsigned(metrics, "cpu.tasks.total"),
                load_1m: number(metrics, "cpu.load.average.1m"),
                uptime_seconds: number(metrics, "cpu.boot_time")
                    .map(|boot| (now_unix - boot).max(0.0)),
                temperature_celsius: number(metrics, "cpu.package.temperature"),
                cores,
            };
        }
        if let Some(memory) = report(reports, "linux.memory") {
            readings.has_memory = true;
            let metrics = &memory.metrics;
            let cached = match (
                unsigned(metrics, "memory.cached"),
                unsigned(metrics, "memory.buffers"),
            ) {
                (Some(cached), Some(buffers)) => Some(cached + buffers),
                (cached, _) => cached,
            };
            readings.memory = MemoryReadings {
                total: unsigned(metrics, "memory.total"),
                available: unsigned(metrics, "memory.available"),
                free: unsigned(metrics, "memory.free"),
                cached,
                modified: unsigned(metrics, "memory.dirty"),
                committed: unsigned(metrics, "memory.commit.committed"),
                commit_limit: unsigned(metrics, "memory.commit.limit"),
                swap_total: unsigned(metrics, "memory.swap.total"),
                swap_used: unsigned(metrics, "memory.swap.used"),
            };
        }
        if let Some(storage) = report(reports, "linux.storage") {
            readings.has_storage = true;
            readings.disks = storage
                .entities_of(EntityKind::BlockDevice)
                .map(|entity| {
                    let m = &entity.metrics;
                    DiskReadings {
                        name: entity.id.key.clone(),
                        model: text(m, "storage.model"),
                        active: number(m, "storage.utilization"),
                        read_rate: number(m, "storage.read.bytes.rate"),
                        write_rate: number(m, "storage.write.bytes.rate"),
                        read_latency_ms: number(m, "storage.read.latency.mean"),
                        write_latency_ms: number(m, "storage.write.latency.mean"),
                        capacity: unsigned(m, "storage.capacity"),
                        rotational: boolean(m, "storage.rotational"),
                    }
                })
                .collect();
            readings.disks.sort_by(|a, b| a.name.cmp(&b.name));
        }
        if let Some(network) = report(reports, "linux.network") {
            readings.has_network = true;
            readings.adapters = network
                .entities_of(EntityKind::NetworkInterface)
                .filter_map(|entity| {
                    let m = &entity.metrics;
                    let kind_text = text(m, "network.link.type");
                    if kind_text.as_deref() == Some("loopback") || entity.id.key == "lo" {
                        // The machine talking to itself is not a connection.
                        return None;
                    }
                    let kind = match kind_text.as_deref() {
                        Some("wifi" | "wireless") => AdapterKind::Wireless,
                        Some("ethernet") => AdapterKind::Wired,
                        _ => AdapterKind::Other,
                    };
                    let up = text(m, "network.link.state").map(|state| state == "up");
                    let carrier = boolean(m, "network.link.carrier");
                    let connected = match (up, carrier) {
                        (Some(false), _) | (_, Some(false)) => Some(false),
                        (Some(true), _) => Some(true),
                        (None, carrier) => carrier,
                    };
                    Some(AdapterReadings {
                        name: entity.id.key.clone(),
                        kind,
                        connected,
                        link_speed_bps: number(m, "network.link.speed").filter(|s| *s > 0.0),
                        receive_rate: number(m, "network.rx.bytes.rate"),
                        send_rate: number(m, "network.tx.bytes.rate"),
                    })
                })
                .collect();
            // Connected adapters first, then by name, so the one carrying the
            // traffic is the one a person sees first.
            readings.adapters.sort_by(|a, b| {
                (a.connected != Some(true))
                    .cmp(&(b.connected != Some(true)))
                    .then_with(|| a.name.cmp(&b.name))
            });
        }
        readings
    }

    /// The sum of every disk's reads and writes, or `None` when no disk
    /// reported either.
    pub(crate) fn disk_total(&self) -> (Option<f64>, Option<f64>) {
        sum(self.disks.iter().map(|d| (d.read_rate, d.write_rate)))
    }

    /// The sum of every adapter's receive and send rates.
    pub(crate) fn network_total(&self) -> (Option<f64>, Option<f64>) {
        sum(self.adapters.iter().map(|a| (a.receive_rate, a.send_rate)))
    }

    /// The busiest disk's active time, which is what the overview shows.
    pub(crate) fn busiest_disk_active(&self) -> Option<f64> {
        self.disks
            .iter()
            .filter_map(|disk| disk.active)
            .fold(None, |peak: Option<f64>, value| {
                Some(peak.map_or(value, |peak| peak.max(value)))
            })
    }
}

fn sum(pairs: impl Iterator<Item = (Option<f64>, Option<f64>)>) -> (Option<f64>, Option<f64>) {
    pairs.fold((None, None), |(a, b), (x, y)| {
        (
            match (a, x) {
                (Some(a), Some(x)) => Some(a + x),
                (a, x) => a.or(x),
            },
            match (b, y) {
                (Some(b), Some(y)) => Some(b + y),
                (b, y) => b.or(y),
            },
        )
    })
}

struct Track {
    values: VecDeque<Option<f64>>,
    /// The last round that carried a reading for it.
    last_seen: u64,
}

/// The last minute of every series, newest last.
#[derive(Default)]
pub(crate) struct LiveHistory {
    tracks: BTreeMap<SeriesKey, Track>,
    round: u64,
}

impl LiveHistory {
    /// Adopt one round. Every series the history already knows gets a value
    /// or a gap; a series nobody has reported for a whole chart's width is
    /// forgotten, so a disk that was unplugged stops being drawn.
    pub(crate) fn record(&mut self, readings: &Readings) {
        self.round += 1;
        let mut now: Vec<(SeriesKey, Option<f64>)> = Vec::new();
        if readings.has_cpu {
            now.push((SeriesKey::Cpu, readings.cpu.utilization.map(|v| v * 100.0)));
            for core in &readings.cpu.cores {
                now.push((
                    SeriesKey::Core(core.index),
                    core.utilization.map(|v| v * 100.0),
                ));
            }
            now.push((SeriesKey::CpuTemperature, readings.cpu.temperature_celsius));
        }
        if readings.has_memory {
            now.push((
                SeriesKey::Memory,
                readings.memory.in_use().map(|v| v as f64),
            ));
        }
        for disk in &readings.disks {
            now.push((
                SeriesKey::DiskActive(disk.name.clone()),
                disk.active.map(|v| v * 100.0),
            ));
            now.push((SeriesKey::DiskRead(disk.name.clone()), disk.read_rate));
            now.push((SeriesKey::DiskWrite(disk.name.clone()), disk.write_rate));
        }
        for adapter in &readings.adapters {
            now.push((
                SeriesKey::Receive(adapter.name.clone()),
                adapter.receive_rate,
            ));
            now.push((SeriesKey::Send(adapter.name.clone()), adapter.send_rate));
        }

        let round = self.round;
        for (key, value) in now {
            let track = self.tracks.entry(key).or_insert_with(|| Track {
                values: VecDeque::with_capacity(CAPACITY),
                last_seen: round,
            });
            track.last_seen = round;
            push(&mut track.values, value);
        }
        for track in self.tracks.values_mut() {
            if track.last_seen != round {
                push(&mut track.values, None);
            }
        }
        self.tracks
            .retain(|_, track| round - track.last_seen < CAPACITY as u64);
    }

    /// A series, oldest first. Empty when it was never reported.
    pub(crate) fn values(&self, key: &SeriesKey) -> Vec<Option<f64>> {
        self.tracks
            .get(key)
            .map(|track| track.values.iter().copied().collect())
            .unwrap_or_default()
    }

    /// How many rounds the history has seen.
    #[cfg(test)]
    pub(crate) fn rounds(&self) -> u64 {
        self.round
    }
}

fn push(values: &mut VecDeque<Option<f64>>, value: Option<f64>) {
    if values.len() == CAPACITY {
        values.pop_front();
    }
    values.push_back(value);
}

/// What the window's tables are showing, captured so a round can be turned
/// into tables away from the window's thread.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TableSettings {
    pub(crate) sort: (monitor_views::ProcessColumn, monitor_views::SortDirection),
    pub(crate) filter: String,
    pub(crate) tree: bool,
    pub(crate) command_lines: bool,
    pub(crate) precedence: monitor_views::grouping::GroupingPrecedence,
    /// The applications the person opened, by group key.
    pub(crate) expanded: Vec<String>,
}

impl TableSettings {
    /// Make a process table show these settings.
    pub(crate) fn apply_processes(&self, model: &mut monitor_views::ProcessTableModel) {
        if model.sort() != self.sort {
            model.set_sort(self.sort.0, self.sort.1);
        }
        if model.filter() != self.filter {
            model.set_filter(self.filter.clone());
        }
        if model.tree_mode() != self.tree {
            model.set_tree_mode(self.tree);
        }
        if model.show_command_line() != self.command_lines {
            model.set_show_command_line(self.command_lines);
        }
    }

    /// Make an applications model show these settings.
    pub(crate) fn apply_apps(&self, model: &mut monitor_views::AppsModel) {
        if model.filter() != self.filter {
            model.set_filter(self.filter.clone());
        }
        for key in &self.expanded {
            if !model.is_expanded(key) {
                model.toggle_expanded(key);
            }
        }
    }
}

/// One round, worked out completely: everything the window adopts in one
/// assignment. Built on a background thread.
pub(crate) struct BuiltRound {
    pub(crate) reports: Vec<CollectorReport>,
    pub(crate) overview: monitor_views::OverviewModel,
    pub(crate) readings: Readings,
    pub(crate) processes: monitor_views::ProcessTableModel,
    pub(crate) apps: monitor_views::AppsModel,
    /// The settings the tables were built with, so the window can tell
    /// whether the person changed one meanwhile.
    pub(crate) settings: TableSettings,
}

impl BuiltRound {
    pub(crate) fn build(
        reports: Vec<CollectorReport>,
        now_unix: f64,
        settings: TableSettings,
    ) -> Self {
        let overview = monitor_views::OverviewModel::from_reports(&reports);
        let readings = Readings::from_reports(&reports, now_unix);
        let facts = reports
            .iter()
            .find(|report| report.collector.as_str() == "linux.process")
            .map(monitor_views::ProcessFacts::from_report)
            .unwrap_or_default();
        let mut processes = monitor_views::ProcessTableModel::new(facts.clone());
        settings.apply_processes(&mut processes);
        let mut apps = monitor_views::AppsModel::new(facts, settings.precedence.clone());
        settings.apply_apps(&mut apps);
        Self {
            reports,
            overview,
            readings,
            processes,
            apps,
            settings,
        }
    }
}
