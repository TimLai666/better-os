//! What collection costs.
//!
//! The specification forbids claiming low overhead without published
//! measurements, so the cost of a sampling round is measured by the same code
//! that runs in production rather than estimated. Wall time comes from the
//! monotonic clock and CPU time from this process's own `/proc/self/stat`, so
//! a round that mostly waits on disk is distinguishable from one that burns
//! CPU.

use crate::cpu::USER_HZ;
use crate::fsread::read_text;
use crate::roots::Roots;
use crate::{LinuxCollectors, ProcessPrivacy};
use monitor_core::Timestamp;
use std::path::Path;
use std::time::{Duration, Instant};

/// The cost of one collector over the measured rounds.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectorCost {
    pub collector: String,
    pub total_wall: Duration,
    pub mean_wall: Duration,
    pub worst_wall: Duration,
}

/// The cost of sampling everything, N times.
#[derive(Clone, Debug, PartialEq)]
pub struct OverheadReport {
    pub rounds: u32,
    pub total_wall: Duration,
    pub mean_wall: Duration,
    pub worst_wall: Duration,
    /// CPU time this process consumed across the measured rounds, where
    /// `/proc/self/stat` was readable.
    pub cpu_time: Option<Duration>,
    pub per_collector: Vec<CollectorCost>,
}

impl OverheadReport {
    /// CPU time as a fraction of wall time. Above 1.0 would mean the work ran
    /// on more than one core, which this collector set never does.
    pub fn cpu_fraction(&self) -> Option<f64> {
        let cpu = self.cpu_time?;
        if self.total_wall.is_zero() {
            return None;
        }
        Some(cpu.as_secs_f64() / self.total_wall.as_secs_f64())
    }
}

/// This process's own user plus system CPU time, from `/proc/self/stat`.
fn self_cpu_time(proc_dir: &Path) -> Option<Duration> {
    let raw = read_text(&proc_dir.join("self/stat")).ok()?;
    let close = raw.rfind(')')?;
    let fields: Vec<&str> = raw[close + 1..].split_whitespace().collect();
    let user = fields.get(11)?.parse::<u64>().ok()?;
    let system = fields.get(12)?.parse::<u64>().ok()?;
    Some(Duration::from_secs_f64(
        (user + system) as f64 / USER_HZ as f64,
    ))
}

/// Sample every collector `rounds` times and report what it cost.
///
/// The first round is included: a cold round is part of what a user pays, and
/// hiding it would flatter the number.
pub fn measure(roots: &Roots, privacy: ProcessPrivacy, rounds: u32) -> OverheadReport {
    let mut collectors = LinuxCollectors::new(roots.clone(), privacy);
    let names = LinuxCollectors::collector_names();
    let mut per_collector: Vec<(Duration, Duration)> =
        vec![(Duration::ZERO, Duration::ZERO); names.len()];

    let cpu_before = self_cpu_time(roots.proc_dir());
    let started = Instant::now();
    let mut worst = Duration::ZERO;
    for _ in 0..rounds {
        let round_started = Instant::now();
        let at = Timestamp::now();
        for (index, cost) in per_collector.iter_mut().enumerate() {
            let each = Instant::now();
            collectors.sample_one(index, roots, at);
            let elapsed = each.elapsed();
            cost.0 += elapsed;
            cost.1 = cost.1.max(elapsed);
        }
        worst = worst.max(round_started.elapsed());
    }
    let total_wall = started.elapsed();
    let cpu_after = self_cpu_time(roots.proc_dir());

    let divisor = rounds.max(1);
    OverheadReport {
        rounds,
        total_wall,
        mean_wall: total_wall / divisor,
        worst_wall: worst,
        cpu_time: cpu_before
            .zip(cpu_after)
            .map(|(before, after)| after.saturating_sub(before)),
        per_collector: names
            .iter()
            .zip(per_collector)
            .map(|(name, (total, worst))| CollectorCost {
                collector: (*name).to_string(),
                total_wall: total,
                mean_wall: total / divisor,
                worst_wall: worst,
            })
            .collect(),
    }
}
