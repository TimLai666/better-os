//! Progress, throughput, and a remaining-time estimate that says how much it
//! trusts itself.
//!
//! Issue #6 asks for "current throughput and realistic remaining-time
//! confidence". The second half is the interesting one. A remaining time
//! computed from four hundred milliseconds of a copy that has not yet left the
//! page cache is not an estimate, it is a number that will embarrass itself
//! ten seconds later. [`RemainingTime`] therefore carries its own
//! [`Confidence`], and the low-confidence cases are reported as low confidence
//! rather than hidden or smoothed until they look plausible.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How much of a job is done, counted both ways.
///
/// Items and bytes are both needed. A job copying one 40 GB file is 0 of 1
/// items for most of its life, and a job copying 200,000 empty files has no
/// bytes to speak of.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub items_total: u64,
    pub items_done: u64,
    pub items_failed: u64,
    pub items_skipped: u64,
    pub bytes_total: u64,
    pub bytes_done: u64,
}

impl Progress {
    /// Fraction of items settled, in `0.0..=1.0`. `None` when the job has no
    /// items at all, which is not the same as zero progress.
    pub fn item_fraction(&self) -> Option<f64> {
        if self.items_total == 0 {
            return None;
        }
        Some((self.settled_items() as f64 / self.items_total as f64).clamp(0.0, 1.0))
    }

    /// Fraction of bytes copied. `None` for an operation that moves no bytes,
    /// such as a rename or a same-filesystem move, where a byte bar would be a
    /// permanently empty bar.
    pub fn byte_fraction(&self) -> Option<f64> {
        if self.bytes_total == 0 {
            return None;
        }
        Some((self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0))
    }

    /// Items that will not be worked on again: done, failed, or skipped.
    pub fn settled_items(&self) -> u64 {
        self.items_done + self.items_failed + self.items_skipped
    }

    pub fn bytes_remaining(&self) -> u64 {
        self.bytes_total.saturating_sub(self.bytes_done)
    }

    pub fn is_finished(&self) -> bool {
        self.items_total > 0 && self.settled_items() >= self.items_total
    }
}

/// Progress for the item currently being worked on.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ItemProgress {
    pub bytes_total: u64,
    pub bytes_done: u64,
}

impl ItemProgress {
    pub fn fraction(&self) -> Option<f64> {
        if self.bytes_total == 0 {
            return None;
        }
        Some((self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0))
    }
}

/// How much an estimate should be trusted.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// There is no basis for an estimate yet: too little elapsed time, no
    /// bytes moved, or a total that is not known.
    None,
    /// A number exists but the sample is short or the rate is swinging.
    Low,
    /// A steady rate over a useful window.
    Medium,
    /// A steady rate over a long window with most of the work still ahead
    /// measured the same way it was measured so far.
    High,
}

impl Confidence {
    pub fn key(self) -> &'static str {
        match self {
            Confidence::None => "files.job.confidence.none",
            Confidence::Low => "files.job.confidence.low",
            Confidence::Medium => "files.job.confidence.medium",
            Confidence::High => "files.job.confidence.high",
        }
    }
}

/// An estimate and how much it is worth.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemainingTime {
    pub estimate: Option<Duration>,
    pub confidence: Confidence,
}

impl RemainingTime {
    pub const UNKNOWN: RemainingTime = RemainingTime {
        estimate: None,
        confidence: Confidence::None,
    };
}

/// Bytes per second, or nothing when no rate has been observed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Throughput {
    pub bytes_per_second: Option<f64>,
    pub items_per_second: Option<f64>,
}

/// A sliding window of samples, from which throughput and the estimate come.
///
/// The window is deliberately short — a few seconds of samples — so a copy
/// that slows down when it leaves the page cache reports the slower rate
/// rather than an average dragged up by the fast start.
#[derive(Clone, Debug)]
pub struct RateEstimator {
    samples: Vec<Sample>,
    window: Duration,
    capacity: usize,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    elapsed: Duration,
    bytes: u64,
    items: u64,
}

impl Default for RateEstimator {
    fn default() -> Self {
        Self::new(Duration::from_secs(5), 64)
    }
}

impl RateEstimator {
    pub fn new(window: Duration, capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity.max(2)),
            window,
            capacity: capacity.max(2),
        }
    }

    /// Records cumulative counters at a point in the job's life.
    pub fn observe(&mut self, elapsed: Duration, bytes: u64, items: u64) {
        self.samples.push(Sample {
            elapsed,
            bytes,
            items,
        });
        let cutoff = elapsed.saturating_sub(self.window);
        // Always keep at least two samples so a slow first chunk still gives a
        // rate rather than resetting to "unknown" on every observation.
        while self.samples.len() > 2 && self.samples[0].elapsed < cutoff {
            self.samples.remove(0);
        }
        while self.samples.len() > self.capacity {
            self.samples.remove(0);
        }
    }

    pub fn throughput(&self) -> Throughput {
        let Some((first, last)) = self.span() else {
            return Throughput::default();
        };
        let seconds = (last.elapsed - first.elapsed).as_secs_f64();
        if seconds <= 0.0 {
            return Throughput::default();
        }
        Throughput {
            bytes_per_second: Some((last.bytes.saturating_sub(first.bytes)) as f64 / seconds),
            items_per_second: Some((last.items.saturating_sub(first.items)) as f64 / seconds),
        }
    }

    /// The remaining-time estimate, with the confidence it has earned.
    ///
    /// The rules, in order:
    ///
    /// - No total, or nothing moved yet: no estimate at all.
    /// - Under a second of observation: low confidence, because the first
    ///   chunk of a copy is the page cache answering, not the disk.
    /// - A rate that swung by more than half between the window's two halves:
    ///   low confidence, whatever the elapsed time.
    /// - Otherwise medium, rising to high once a quarter of the bytes are
    ///   through and the rate is steady.
    pub fn remaining(&self, progress: &Progress) -> RemainingTime {
        let bytes_remaining = progress.bytes_remaining();
        if progress.bytes_total == 0 {
            return self.remaining_by_items(progress);
        }
        let throughput = self.throughput();
        let Some(rate) = throughput.bytes_per_second.filter(|rate| *rate > 0.0) else {
            return RemainingTime::UNKNOWN;
        };
        let estimate =
            Duration::from_secs_f64((bytes_remaining as f64 / rate).min(86_400.0 * 30.0));
        RemainingTime {
            estimate: Some(estimate),
            confidence: self.confidence(progress.byte_fraction().unwrap_or(0.0)),
        }
    }

    /// The item-count fallback, used when there are no bytes to count: a
    /// directory tree of empty files, a trash of many small items.
    fn remaining_by_items(&self, progress: &Progress) -> RemainingTime {
        if progress.items_total == 0 {
            return RemainingTime::UNKNOWN;
        }
        let throughput = self.throughput();
        let Some(rate) = throughput.items_per_second.filter(|rate| *rate > 0.0) else {
            return RemainingTime::UNKNOWN;
        };
        let remaining = progress
            .items_total
            .saturating_sub(progress.settled_items());
        RemainingTime {
            estimate: Some(Duration::from_secs_f64(
                (remaining as f64 / rate).min(86_400.0 * 30.0),
            )),
            confidence: self.confidence(progress.item_fraction().unwrap_or(0.0)),
        }
    }

    fn confidence(&self, fraction: f64) -> Confidence {
        let Some((first, last)) = self.span() else {
            return Confidence::None;
        };
        let observed = last.elapsed - first.elapsed;
        if observed < Duration::from_millis(1000) {
            return Confidence::Low;
        }
        if self.rate_is_swinging() {
            return Confidence::Low;
        }
        if observed >= Duration::from_secs(3) && fraction >= 0.25 {
            Confidence::High
        } else {
            Confidence::Medium
        }
    }

    /// Whether the second half of the window moved at a wildly different rate
    /// from the first half.
    fn rate_is_swinging(&self) -> bool {
        if self.samples.len() < 4 {
            return true;
        }
        let middle = self.samples.len() / 2;
        let first_rate = rate_between(&self.samples[0], &self.samples[middle]);
        let second_rate =
            rate_between(&self.samples[middle], &self.samples[self.samples.len() - 1]);
        match (first_rate, second_rate) {
            (Some(first), Some(second)) if first > 0.0 && second > 0.0 => {
                let ratio = if first > second {
                    first / second
                } else {
                    second / first
                };
                ratio > 1.5
            }
            _ => true,
        }
    }

    fn span(&self) -> Option<(Sample, Sample)> {
        if self.samples.len() < 2 {
            return None;
        }
        Some((self.samples[0], self.samples[self.samples.len() - 1]))
    }
}

fn rate_between(first: &Sample, last: &Sample) -> Option<f64> {
    let seconds = (last.elapsed.checked_sub(first.elapsed)?).as_secs_f64();
    if seconds <= 0.0 {
        return None;
    }
    Some((last.bytes.saturating_sub(first.bytes)) as f64 / seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steady(rate: u64, seconds: u64) -> RateEstimator {
        let mut estimator = RateEstimator::default();
        for step in 0..=seconds * 4 {
            estimator.observe(Duration::from_millis(step * 250), rate * step / 4, step / 4);
        }
        estimator
    }

    #[test]
    fn an_estimate_with_nothing_observed_says_so_instead_of_guessing_zero() {
        let estimator = RateEstimator::default();
        let progress = Progress {
            items_total: 3,
            bytes_total: 1000,
            ..Progress::default()
        };
        let remaining = estimator.remaining(&progress);
        assert_eq!(remaining.estimate, None);
        assert_eq!(remaining.confidence, Confidence::None);
    }

    #[test]
    fn half_a_second_of_samples_is_reported_as_low_confidence() {
        let mut estimator = RateEstimator::default();
        for step in 0..=5u64 {
            estimator.observe(Duration::from_millis(step * 100), step * 1_000_000, step);
        }
        let progress = Progress {
            items_total: 1,
            bytes_total: 100_000_000,
            bytes_done: 5_000_000,
            ..Progress::default()
        };
        let remaining = estimator.remaining(&progress);
        assert!(remaining.estimate.is_some());
        assert_eq!(remaining.confidence, Confidence::Low);
    }

    #[test]
    fn a_steady_long_run_well_into_the_job_earns_high_confidence() {
        let estimator = steady(10_000_000, 6);
        let progress = Progress {
            items_total: 1,
            bytes_total: 100_000_000,
            bytes_done: 60_000_000,
            ..Progress::default()
        };
        let remaining = estimator.remaining(&progress);
        assert_eq!(remaining.confidence, Confidence::High);
        // 40 MB left at 10 MB/s is about four seconds.
        let estimate = remaining.estimate.unwrap();
        assert!(
            estimate >= Duration::from_secs(3) && estimate <= Duration::from_secs(5),
            "estimate was {estimate:?}"
        );
    }

    #[test]
    fn a_rate_that_collapses_mid_window_drops_back_to_low_confidence() {
        let mut estimator = RateEstimator::default();
        let mut bytes = 0u64;
        for step in 0..8u64 {
            bytes += 10_000_000;
            estimator.observe(Duration::from_millis(step * 300), bytes, step);
        }
        for step in 8..16u64 {
            bytes += 200_000;
            estimator.observe(Duration::from_millis(step * 300), bytes, step);
        }
        let progress = Progress {
            items_total: 1,
            bytes_total: 500_000_000,
            bytes_done: bytes,
            ..Progress::default()
        };
        assert_eq!(estimator.remaining(&progress).confidence, Confidence::Low);
    }

    #[test]
    fn a_job_with_no_bytes_estimates_from_items_instead() {
        let mut estimator = RateEstimator::default();
        for step in 0..=20u64 {
            estimator.observe(Duration::from_millis(step * 250), 0, step * 10);
        }
        let progress = Progress {
            items_total: 1000,
            items_done: 200,
            ..Progress::default()
        };
        let remaining = estimator.remaining(&progress);
        assert!(remaining.estimate.is_some());
        assert!(remaining.confidence >= Confidence::Low);
    }

    #[test]
    fn a_byte_bar_is_absent_rather_than_empty_for_an_operation_that_moves_none() {
        let progress = Progress {
            items_total: 2,
            items_done: 1,
            ..Progress::default()
        };
        assert_eq!(progress.byte_fraction(), None);
        assert_eq!(progress.item_fraction(), Some(0.5));
    }
}
