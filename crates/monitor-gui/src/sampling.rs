//! Collection, off the interface thread.
//!
//! Reading a thousand processes out of `/proc` takes milliseconds, and doing
//! it inside `render` would drop a frame every tick. So a background task owns
//! the collectors, samples on a steady interval, and hands finished rounds
//! back to the window through a channel. There is no busy loop: the task is
//! parked on a timer between rounds.
//!
//! Pausing the display does not pause this. The specification is explicit that
//! a user who stops the numbers moving must not thereby stop collection, and
//! keeping the two separate here is what makes that true rather than intended.

use monitor_collectors_linux::{LinuxCollectors, ProcessPrivacy, Roots};
use monitor_core::{CollectorReport, Timestamp};
use monitor_views::ProcessFacts;

/// How often a round is taken.
///
/// One second is above `MINIMUM_DELTA_INTERVAL`, so every counter-delta metric
/// can produce a value rather than reporting the interval as too short.
pub(crate) const SAMPLE_INTERVAL_MILLIS: u64 = 1_000;

/// One finished round.
///
/// The processes are extracted here rather than in the window, because that
/// work scales with the process count and belongs on the same side of the
/// channel as the reading that produced it.
pub(crate) struct Round {
    pub(crate) reports: Vec<CollectorReport>,
    pub(crate) processes: Vec<ProcessFacts>,
}

/// Owns the collectors for the lifetime of the window.
pub(crate) struct Sampler {
    roots: Roots,
    collectors: LinuxCollectors,
}

impl Sampler {
    pub(crate) fn new(privacy: ProcessPrivacy) -> Self {
        let roots = Roots::system();
        Self {
            collectors: LinuxCollectors::new(roots.clone(), privacy),
            roots,
        }
    }

    /// Whether command lines are being collected. Changing it rebuilds the
    /// collectors, which resets the counter deltas — the honest cost of
    /// changing what is collected mid-session.
    pub(crate) fn set_privacy(&mut self, privacy: ProcessPrivacy) {
        self.collectors = LinuxCollectors::new(self.roots.clone(), privacy);
    }

    pub(crate) fn sample(&mut self) -> Round {
        let reports = self.collectors.sample(&self.roots, Timestamp::now());
        let processes = reports
            .iter()
            .find(|report| report.collector.as_str() == "linux.process")
            .map(ProcessFacts::from_report)
            .unwrap_or_default();
        Round { reports, processes }
    }
}
