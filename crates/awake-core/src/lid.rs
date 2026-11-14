//! How long a session may keep the machine awake with the lid closed on
//! battery (ticket 109).
//!
//! "Also when the lid is closed" stops logind suspending the machine when the
//! lid shuts. A laptop closed into a bag on battery would then run until it is
//! flat, so the service ends every session holding the lid once the lid has
//! been closed on battery for this long, and the machine may sleep. The battery
//! stop threshold still applies on top of it.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Two hours, the owner's default.
pub const DEFAULT_LID_BATTERY_LIMIT_MINUTES: u32 = 120;

/// The shortest limit worth offering. Anything shorter ends a session before a
/// person has carried the laptop to the next room.
pub const MIN_LID_BATTERY_LIMIT_MINUTES: u32 = 15;

/// A day. A longer limit is "never" in all but name, and "never" is offered.
pub const MAX_LID_BATTERY_LIMIT_MINUTES: u32 = 24 * 60;

/// When a closed lid on battery ends the sessions holding the lid.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum LidBatteryLimit {
    /// Only the battery stop threshold ends such a session.
    Never,
    /// After the lid has been closed on battery for this many minutes.
    Minutes { minutes: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum LidBatteryLimitError {
    #[error("awake.error.lid_limit_out_of_range:{minutes}")]
    OutOfRange { minutes: u32 },
}

impl Default for LidBatteryLimit {
    fn default() -> Self {
        LidBatteryLimit::Minutes {
            minutes: DEFAULT_LID_BATTERY_LIMIT_MINUTES,
        }
    }
}

impl LidBatteryLimit {
    pub fn validate(self) -> Result<Self, LidBatteryLimitError> {
        match self {
            LidBatteryLimit::Never => Ok(self),
            LidBatteryLimit::Minutes { minutes }
                if (MIN_LID_BATTERY_LIMIT_MINUTES..=MAX_LID_BATTERY_LIMIT_MINUTES)
                    .contains(&minutes) =>
            {
                Ok(self)
            }
            LidBatteryLimit::Minutes { minutes } => {
                Err(LidBatteryLimitError::OutOfRange { minutes })
            }
        }
    }

    pub fn minutes(self) -> Option<u32> {
        match self {
            LidBatteryLimit::Never => None,
            LidBatteryLimit::Minutes { minutes } => Some(minutes),
        }
    }

    /// Whether a lid closed on battery since `since` has reached the limit by
    /// `now`. A clock that stepped backwards has not reached anything.
    pub fn reached(self, since_unix_seconds: u64, now_unix_seconds: u64) -> bool {
        self.minutes().is_some_and(|minutes| {
            now_unix_seconds.saturating_sub(since_unix_seconds) >= u64::from(minutes) * 60
        })
    }

    /// When a lid closed on battery since `since` reaches the limit.
    pub fn ends_at(self, since_unix_seconds: u64) -> Option<u64> {
        self.minutes()
            .map(|minutes| since_unix_seconds.saturating_add(u64::from(minutes) * 60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_two_hours() {
        assert_eq!(LidBatteryLimit::default().minutes(), Some(120));
    }

    #[test]
    fn the_limit_is_reached_at_exactly_its_length_and_never_is_never_reached() {
        let limit = LidBatteryLimit::Minutes { minutes: 30 };
        assert!(!limit.reached(1_000, 1_000 + 30 * 60 - 1));
        assert!(limit.reached(1_000, 1_000 + 30 * 60));
        assert!(!limit.reached(5_000, 1_000), "a clock stepping back");
        assert_eq!(limit.ends_at(1_000), Some(1_000 + 1_800));
        assert!(!LidBatteryLimit::Never.reached(0, u64::MAX));
        assert_eq!(LidBatteryLimit::Never.ends_at(0), None);
    }

    #[test]
    fn a_limit_outside_the_offered_range_is_refused() {
        for minutes in [0, 14, 24 * 60 + 1] {
            assert_eq!(
                LidBatteryLimit::Minutes { minutes }.validate(),
                Err(LidBatteryLimitError::OutOfRange { minutes })
            );
        }
        for minutes in [15, 120, 24 * 60] {
            assert!(LidBatteryLimit::Minutes { minutes }.validate().is_ok());
        }
        assert!(LidBatteryLimit::Never.validate().is_ok());
    }

    #[test]
    fn the_limit_has_a_closed_shape_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&LidBatteryLimit::Never).unwrap(),
            r#"{"kind":"never"}"#
        );
        assert_eq!(
            serde_json::from_str::<LidBatteryLimit>(r#"{"kind":"minutes","minutes":60}"#).unwrap(),
            LidBatteryLimit::Minutes { minutes: 60 }
        );
        assert!(serde_json::from_str::<LidBatteryLimit>(r#"{}"#).is_err());
    }
}
