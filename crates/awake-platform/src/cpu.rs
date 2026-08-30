//! CPU utilization, from the aggregate line of `/proc/stat`.
//!
//! Utilization is a rate, not a level, so it only exists between two samples.
//! The first sample after startup therefore reports nothing rather than
//! inventing a figure from one reading — a provider that answered "0%" on its
//! first call would make a "keep awake while the CPU is busy" rule miss the
//! first interval of every build.

use awake_core::{Observations, ProviderKind};

use crate::provider::{CPU_POLL_SECONDS, Cadence, TriggerProvider};
use crate::roots::{ReadError, Roots, read_text};

/// The counters the aggregate `cpu` line carries, in jiffies.
///
/// `guest` and `guest_nice` are deliberately not added on top: the kernel
/// already counts them inside `user` and `nice`, so adding them again would
/// inflate the total and depress every utilization figure on a machine running
/// virtual machines.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

impl CpuTimes {
    /// Parses the aggregate `cpu` line.
    ///
    /// `idle` is idle plus iowait, because a machine waiting on a disk is not
    /// doing work a keep-awake rule should fire on.
    pub fn parse(text: &str) -> Option<Self> {
        let line = text
            .lines()
            .find(|line| line.starts_with("cpu ") || line == &"cpu")?;
        let fields: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|field| field.parse::<u64>().ok())
            .collect();
        // user nice system idle iowait irq softirq steal, at minimum. A kernel
        // that reports fewer is one this parser does not understand, and saying
        // so beats computing a figure from half a line.
        if fields.len() < 4 {
            return None;
        }
        let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
        // Only the first eight fields are real time categories; `guest` and
        // `guest_nice` follow them and are already double-counted.
        let total: u64 = fields.iter().take(8).sum();
        Some(Self { idle, total })
    }

    /// The busy percentage between this sample and a later one.
    ///
    /// Returns `None` when the counters did not move, which happens on a sample
    /// taken twice within one jiffy, and when they went backwards, which happens
    /// after a suspend on some kernels. Neither is a zero.
    pub fn utilization_since(&self, earlier: &CpuTimes) -> Option<u8> {
        let total = self.total.checked_sub(earlier.total)?;
        let idle = self.idle.checked_sub(earlier.idle)?;
        if total == 0 || idle > total {
            return None;
        }
        let busy = total - idle;
        Some(((busy * 100) / total).min(100) as u8)
    }
}

/// Reads CPU utilization across the whole machine.
#[derive(Clone, Debug)]
pub struct CpuProvider {
    roots: Roots,
    previous: Option<CpuTimes>,
}

impl CpuProvider {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            previous: None,
        }
    }

    fn read(&self) -> Result<Option<CpuTimes>, ReadError> {
        let text = read_text(&self.roots.proc_path("stat"))?;
        Ok(CpuTimes::parse(&text))
    }
}

impl TriggerProvider for CpuProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::CpuUtilization
    }

    fn cadence(&self) -> Cadence {
        Cadence::Poll {
            seconds: CPU_POLL_SECONDS,
        }
    }

    fn sample(&mut self, _now_unix_seconds: u64, into: &mut Observations) {
        let current = match self.read() {
            Err(error) => {
                into.mark_unavailable(ProviderKind::CpuUtilization, error.explanation());
                return;
            }
            Ok(None) => {
                into.mark_unavailable(
                    ProviderKind::CpuUtilization,
                    "awake.provider.malformed_stat",
                );
                return;
            }
            Ok(Some(times)) => times,
        };

        match self.previous.replace(current) {
            None => into.mark_unavailable(
                ProviderKind::CpuUtilization,
                "awake.provider.awaiting_second_sample",
            ),
            Some(previous) => match current.utilization_since(&previous) {
                Some(percent) => {
                    into.cpu_utilization_percent = Some(percent);
                    into.mark_available(ProviderKind::CpuUtilization);
                }
                None => into.mark_unavailable(
                    ProviderKind::CpuUtilization,
                    "awake.provider.counters_did_not_advance",
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(stat: &str) -> (tempfile::TempDir, Roots) {
        let directory = tempfile::tempdir().unwrap();
        let proc = directory.path().join("proc");
        std::fs::create_dir_all(&proc).unwrap();
        std::fs::write(proc.join("stat"), stat).unwrap();
        let roots = Roots::at(directory.path());
        (directory, roots)
    }

    fn write_stat(roots: &Roots, stat: &str) {
        std::fs::write(roots.proc_path("stat"), stat).unwrap();
    }

    #[test]
    fn utilization_needs_two_samples_and_says_so_after_the_first() {
        let (_directory, roots) = fixture("cpu  100 0 100 800 0 0 0 0 0 0\n");
        let mut provider = CpuProvider::new(roots.clone());

        let mut first = Observations::at(1_000);
        provider.sample(1_000, &mut first);
        assert_eq!(first.cpu_utilization_percent, None);
        assert_eq!(
            first
                .availability_of(ProviderKind::CpuUtilization)
                .explanation(),
            Some("awake.provider.awaiting_second_sample"),
            "reporting 0% from one reading would make every rule miss its first interval"
        );

        write_stat(&roots, "cpu  200 0 200 1400 0 0 0 0 0 0\n");
        let mut second = Observations::at(1_005);
        provider.sample(1_005, &mut second);
        // 200 busy jiffies against 800 total between the samples.
        assert_eq!(second.cpu_utilization_percent, Some(25));
    }

    #[test]
    fn iowait_counts_as_idle_because_waiting_on_a_disk_is_not_work() {
        let earlier = CpuTimes::parse("cpu  0 0 0 0 0 0 0 0\n").unwrap();
        let later = CpuTimes::parse("cpu  0 0 0 0 100 0 0 0\n").unwrap();
        assert_eq!(later.utilization_since(&earlier), Some(0));
    }

    #[test]
    fn guest_time_is_not_added_twice() {
        // The kernel already counts guest inside user, so a line with 100 user
        // and 100 guest describes 100 jiffies of work, not 200.
        let times = CpuTimes::parse("cpu  100 0 0 900 0 0 0 0 100 0\n").unwrap();
        assert_eq!(times.total, 1_000);
    }

    #[test]
    fn a_truncated_stat_line_is_malformed_and_not_a_zero() {
        assert_eq!(CpuTimes::parse("cpu  1 2 3\n"), None);

        let (_directory, roots) = fixture("cpu  1 2 3\n");
        let mut provider = CpuProvider::new(roots);
        let mut observations = Observations::at(1_000);
        provider.sample(1_000, &mut observations);
        assert_eq!(
            observations
                .availability_of(ProviderKind::CpuUtilization)
                .explanation(),
            Some("awake.provider.malformed_stat")
        );
    }

    #[test]
    fn a_counter_that_went_backwards_after_a_suspend_is_unknown_not_zero() {
        let (_directory, roots) = fixture("cpu  500 0 500 5000 0 0 0 0\n");
        let mut provider = CpuProvider::new(roots.clone());
        provider.sample(1_000, &mut Observations::at(1_000));

        write_stat(&roots, "cpu  10 0 10 100 0 0 0 0\n");
        let mut observations = Observations::at(1_005);
        provider.sample(1_005, &mut observations);
        assert_eq!(observations.cpu_utilization_percent, None);
        assert_eq!(
            observations
                .availability_of(ProviderKind::CpuUtilization)
                .explanation(),
            Some("awake.provider.counters_did_not_advance")
        );
    }

    #[test]
    fn two_identical_samples_report_unknown_rather_than_a_fabricated_zero() {
        let (_directory, roots) = fixture("cpu  100 0 100 800 0 0 0 0\n");
        let mut provider = CpuProvider::new(roots);
        provider.sample(1_000, &mut Observations::at(1_000));

        let mut observations = Observations::at(1_000);
        provider.sample(1_000, &mut observations);
        assert_eq!(observations.cpu_utilization_percent, None);
    }

    #[test]
    fn a_fully_busy_machine_reports_a_hundred_and_never_more() {
        let earlier = CpuTimes::parse("cpu  0 0 0 0 0 0 0 0\n").unwrap();
        let later = CpuTimes::parse("cpu  1000 0 0 0 0 0 0 0\n").unwrap();
        assert_eq!(later.utilization_since(&earlier), Some(100));
    }

    #[test]
    fn a_missing_proc_stat_names_the_path_rather_than_reporting_an_idle_machine() {
        let directory = tempfile::tempdir().unwrap();
        let mut provider = CpuProvider::new(Roots::at(directory.path()));
        let mut observations = Observations::at(1_000);
        provider.sample(1_000, &mut observations);
        let explanation = observations
            .availability_of(ProviderKind::CpuUtilization)
            .explanation()
            .unwrap()
            .to_string();
        assert!(explanation.contains("proc/stat"), "{explanation}");
    }
}
