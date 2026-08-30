//! A monotonic session clock.
//!
//! Safety decisions compare "when was the flush verified" against "when did the
//! last write arrive", and a wall clock that jumps backwards over NTP would make
//! a stale proof look fresh. The service converts one `Instant` origin into
//! these values, and tests hand in exact ones.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Time since the coordinating service started observing.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(Duration);

impl Timestamp {
    pub const START: Timestamp = Timestamp(Duration::ZERO);

    pub fn from_millis(millis: u64) -> Self {
        Timestamp(Duration::from_millis(millis))
    }

    pub fn from_duration(elapsed: Duration) -> Self {
        Timestamp(elapsed)
    }

    pub fn as_duration(self) -> Duration {
        self.0
    }

    /// Elapsed time since an earlier point. A later point compared against an
    /// earlier one saturates at zero rather than wrapping, so an out-of-order
    /// event cannot manufacture a negative age.
    pub fn duration_since(self, earlier: Timestamp) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_out_of_order_pair_reports_zero_age_rather_than_wrapping() {
        let early = Timestamp::from_millis(10);
        let late = Timestamp::from_millis(50);
        assert_eq!(late.duration_since(early), Duration::from_millis(40));
        assert_eq!(early.duration_since(late), Duration::ZERO);
    }
}
