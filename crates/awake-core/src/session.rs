//! One session: what it prevents, why, and when it stops.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::policy::SessionPolicy;
use crate::rules::RuleId;

/// Battery percentage a session stops at unless the user chose otherwise.
/// Issue #13 fixes the tray wording at `低於 20% 電量時停止`, so this is the
/// number that wording promises.
pub const DEFAULT_BATTERY_STOP_PERCENT: u8 = 20;

/// A reason is shown in a panel menu and handed to logind, both of which are
/// happier with a short line than a paragraph.
pub const MAX_REASON_CHARS: usize = 120;

/// Thirty days. Long enough for any honest session, short enough that an
/// overflowing `until` time is caught as the mistake it is.
pub const MAX_SESSION_SECONDS: u64 = 30 * 24 * 60 * 60;

/// Identity of an active session, handed back to the tray so a later Extend or
/// End names the session it meant rather than "whatever is active now".
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub u64);

impl std::fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A validated human-readable reason.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Reason(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum ReasonError {
    #[error("awake.error.reason_empty")]
    Empty,
    #[error("awake.error.reason_too_long")]
    TooLong,
    #[error("awake.error.reason_control_character")]
    ControlCharacter,
}

impl Reason {
    pub fn new(value: impl Into<String>) -> Result<Self, ReasonError> {
        let value: String = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(ReasonError::Empty);
        }
        if trimmed.chars().count() > MAX_REASON_CHARS {
            return Err(ReasonError::TooLong);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(ReasonError::ControlCharacter);
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Reason {
    type Error = ReasonError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Reason::new(value)
    }
}

impl From<Reason> for String {
    fn from(reason: Reason) -> Self {
        reason.0
    }
}

impl std::fmt::Display for Reason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Who asked for the session. A manual session is one a person started; a
/// trigger session is one an automatic rule is holding. They may run together,
/// and ending either never ends the other.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrigin {
    Manual,
    Trigger,
}

/// When a session stops on its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EndCondition {
    /// Runs until someone ends it. The tray shows "until ended" rather than
    /// inventing a time.
    Indefinite,
    /// A fixed length measured from the session's start.
    Duration { seconds: u64 },
    /// A wall-clock instant the user picked.
    UntilUnixSeconds { unix_seconds: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum EndConditionError {
    #[error("awake.error.duration_zero")]
    ZeroDuration,
    #[error("awake.error.duration_too_long")]
    DurationTooLong,
    #[error("awake.error.end_time_in_the_past")]
    EndTimeInThePast,
    #[error("awake.error.cannot_extend_indefinite")]
    CannotExtendIndefinite,
}

impl EndCondition {
    /// Checks a condition against the moment the session would start.
    ///
    /// An `until` time in the past is refused rather than quietly producing a
    /// session that ends the instant it begins.
    pub fn validate(&self, start_unix_seconds: u64) -> Result<(), EndConditionError> {
        match *self {
            EndCondition::Indefinite => Ok(()),
            EndCondition::Duration { seconds } => {
                if seconds == 0 {
                    Err(EndConditionError::ZeroDuration)
                } else if seconds > MAX_SESSION_SECONDS {
                    Err(EndConditionError::DurationTooLong)
                } else {
                    Ok(())
                }
            }
            EndCondition::UntilUnixSeconds { unix_seconds } => {
                if unix_seconds <= start_unix_seconds {
                    Err(EndConditionError::EndTimeInThePast)
                } else if unix_seconds - start_unix_seconds > MAX_SESSION_SECONDS {
                    Err(EndConditionError::DurationTooLong)
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// What is left of a session, as a value rather than a formatted string. The
/// tray owns the wording; this owns the arithmetic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Remaining {
    /// No end condition: it runs until ended.
    UntilEnded,
    Seconds(u64),
    /// The end has passed and the session has not been reaped yet.
    Elapsed,
}

/// Everything a client must supply to start a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRequest {
    pub reason: Reason,
    pub origin: SessionOrigin,
    pub policy: SessionPolicy,
    /// `None` means the session never stops itself for battery level.
    pub battery_stop_percent: Option<u8>,
    pub end: EndCondition,
    /// The automatic rule holding this session, when one is. Always `None` for a
    /// manual session, which is what lets the service tell, after a restart,
    /// which sessions belong to rules it must now re-evaluate.
    pub rule: Option<RuleId>,
}

impl SessionRequest {
    /// The one-click session the tray's presets use: keeps the machine running
    /// and lets the screen turn off and lock by itself.
    pub fn quick(reason: Reason, end: EndCondition) -> Self {
        Self {
            reason,
            origin: SessionOrigin::Manual,
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(DEFAULT_BATTERY_STOP_PERCENT),
            end,
            rule: None,
        }
    }
}

/// The mutable part of a session, replaced wholesale by a Change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionChange {
    pub reason: Reason,
    pub policy: SessionPolicy,
    pub battery_stop_percent: Option<u8>,
    pub end: EndCondition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub id: SessionId,
    pub reason: Reason,
    pub origin: SessionOrigin,
    pub policy: SessionPolicy,
    pub battery_stop_percent: Option<u8>,
    pub end: EndCondition,
    pub started_at_unix_seconds: u64,
    /// The automatic rule holding this session, when one is.
    pub rule: Option<RuleId>,
}

impl Session {
    /// The instant this session stops on its own, or `None` when it does not.
    pub fn ends_at_unix_seconds(&self) -> Option<u64> {
        match self.end {
            EndCondition::Indefinite => None,
            EndCondition::Duration { seconds } => {
                Some(self.started_at_unix_seconds.saturating_add(seconds))
            }
            EndCondition::UntilUnixSeconds { unix_seconds } => Some(unix_seconds),
        }
    }

    pub fn remaining(&self, now_unix_seconds: u64) -> Remaining {
        match self.ends_at_unix_seconds() {
            None => Remaining::UntilEnded,
            Some(end) if end > now_unix_seconds => Remaining::Seconds(end - now_unix_seconds),
            Some(_) => Remaining::Elapsed,
        }
    }

    pub fn has_expired(&self, now_unix_seconds: u64) -> bool {
        matches!(self.remaining(now_unix_seconds), Remaining::Elapsed)
    }

    /// Pushes the end out by `by_seconds`.
    ///
    /// An indefinite session has no end to push, so extending one is refused
    /// rather than silently doing nothing. Extending a session whose end has
    /// already passed measures from now, not from the missed end, so "extend by
    /// 15 minutes" always buys fifteen more minutes.
    pub fn extend(
        &mut self,
        by_seconds: u64,
        now_unix_seconds: u64,
    ) -> Result<(), EndConditionError> {
        if by_seconds == 0 {
            return Err(EndConditionError::ZeroDuration);
        }
        let base = match self.ends_at_unix_seconds() {
            None => return Err(EndConditionError::CannotExtendIndefinite),
            Some(end) => end.max(now_unix_seconds),
        };
        let end = base.saturating_add(by_seconds);
        if end.saturating_sub(self.started_at_unix_seconds) > MAX_SESSION_SECONDS {
            return Err(EndConditionError::DurationTooLong);
        }
        self.end = EndCondition::UntilUnixSeconds { unix_seconds: end };
        Ok(())
    }

    /// Whether the battery has fallen to this session's stop threshold.
    pub fn should_stop_for_battery(&self, percent: u8) -> bool {
        self.battery_stop_percent
            .is_some_and(|threshold| percent < threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(end: EndCondition) -> Session {
        Session {
            id: SessionId(1),
            reason: Reason::new("Build is running").unwrap(),
            origin: SessionOrigin::Manual,
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(DEFAULT_BATTERY_STOP_PERCENT),
            end,
            started_at_unix_seconds: 1_000,
            rule: None,
        }
    }

    #[test]
    fn a_reason_is_trimmed_and_bounded() {
        assert_eq!(Reason::new("  Build  ").unwrap().as_str(), "Build");
        assert_eq!(Reason::new("   "), Err(ReasonError::Empty));
        assert_eq!(
            Reason::new("a".repeat(MAX_REASON_CHARS + 1)),
            Err(ReasonError::TooLong)
        );
        assert_eq!(
            Reason::new("two\nlines"),
            Err(ReasonError::ControlCharacter)
        );
    }

    #[test]
    fn a_reason_counts_characters_not_bytes() {
        // The zh-TW wording is the reason this is not a byte length.
        let reason = "保持清醒".repeat(MAX_REASON_CHARS / 4);
        assert!(Reason::new(reason).is_ok());
    }

    #[test]
    fn an_indefinite_session_has_no_end_and_runs_until_ended() {
        let session = session(EndCondition::Indefinite);
        assert_eq!(session.ends_at_unix_seconds(), None);
        assert_eq!(session.remaining(9_999), Remaining::UntilEnded);
        assert!(!session.has_expired(u64::MAX));
    }

    #[test]
    fn a_timed_session_counts_down_from_its_start() {
        let session = session(EndCondition::Duration { seconds: 900 });
        assert_eq!(session.ends_at_unix_seconds(), Some(1_900));
        assert_eq!(session.remaining(1_000), Remaining::Seconds(900));
        assert_eq!(session.remaining(1_899), Remaining::Seconds(1));
        assert_eq!(session.remaining(1_900), Remaining::Elapsed);
        assert!(session.has_expired(1_901));
    }

    #[test]
    fn an_until_time_session_ends_at_that_time_however_late_it_started() {
        let session = session(EndCondition::UntilUnixSeconds {
            unix_seconds: 5_000,
        });
        assert_eq!(session.remaining(4_000), Remaining::Seconds(1_000));
        assert_eq!(session.remaining(5_000), Remaining::Elapsed);
    }

    #[test]
    fn extending_a_timed_session_adds_to_its_end() {
        let mut session = session(EndCondition::Duration { seconds: 900 });
        session.extend(900, 1_500).unwrap();
        assert_eq!(session.ends_at_unix_seconds(), Some(2_800));
    }

    #[test]
    fn extending_a_session_whose_end_already_passed_measures_from_now() {
        let mut session = session(EndCondition::Duration { seconds: 900 });
        session.extend(600, 5_000).unwrap();
        assert_eq!(session.ends_at_unix_seconds(), Some(5_600));
    }

    #[test]
    fn an_indefinite_session_cannot_be_extended() {
        let mut session = session(EndCondition::Indefinite);
        assert_eq!(
            session.extend(900, 1_000),
            Err(EndConditionError::CannotExtendIndefinite)
        );
    }

    #[test]
    fn extending_past_the_maximum_is_refused_and_changes_nothing() {
        let mut session = session(EndCondition::Duration { seconds: 900 });
        assert_eq!(
            session.extend(MAX_SESSION_SECONDS, 1_000),
            Err(EndConditionError::DurationTooLong)
        );
        assert_eq!(session.ends_at_unix_seconds(), Some(1_900));
    }

    #[test]
    fn extending_by_nothing_is_refused() {
        let mut session = session(EndCondition::Duration { seconds: 900 });
        assert_eq!(
            session.extend(0, 1_000),
            Err(EndConditionError::ZeroDuration)
        );
    }

    #[test]
    fn an_end_time_in_the_past_is_refused() {
        assert_eq!(
            EndCondition::UntilUnixSeconds {
                unix_seconds: 1_000
            }
            .validate(1_000),
            Err(EndConditionError::EndTimeInThePast)
        );
        assert_eq!(
            EndCondition::Duration { seconds: 0 }.validate(1_000),
            Err(EndConditionError::ZeroDuration)
        );
        assert_eq!(
            EndCondition::Duration {
                seconds: MAX_SESSION_SECONDS + 1
            }
            .validate(1_000),
            Err(EndConditionError::DurationTooLong)
        );
        assert!(EndCondition::Indefinite.validate(1_000).is_ok());
    }

    #[test]
    fn a_battery_threshold_stops_below_but_not_at_the_threshold() {
        let session = session(EndCondition::Indefinite);
        assert!(session.should_stop_for_battery(19));
        assert!(!session.should_stop_for_battery(20));

        let mut opted_out = session.clone();
        opted_out.battery_stop_percent = None;
        assert!(!opted_out.should_stop_for_battery(1));
    }

    #[test]
    fn the_quick_preset_keeps_the_machine_up_and_lets_the_screen_rest() {
        let request = SessionRequest::quick(
            Reason::new("保持清醒").unwrap(),
            EndCondition::Duration { seconds: 7_200 },
        );
        assert!(request.policy.prevent_system_suspend);
        assert!(!request.policy.prevent_display_sleep);
        assert!(!request.policy.prevent_automatic_lock);
        assert_eq!(
            request.battery_stop_percent,
            Some(DEFAULT_BATTERY_STOP_PERCENT)
        );
    }
}
