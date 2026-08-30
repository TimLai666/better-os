//! A reading, carried far enough to be drawn.
//!
//! `Observation` is the collector's answer. A view needs the same answer with
//! the value already narrowed to the type the column expects, because the
//! alternative is every render site calling `as_f64().unwrap_or(0.0)` and
//! quietly turning an unreadable descriptor count into "no open files".
//!
//! `Field` keeps the five observation states plus one more that only a
//! consumer can see: a metric the collector never wrote at all. That is not
//! the same as a metric it wrote as unknown, and the difference matters when
//! diagnosing which collector stopped reporting.

use monitor_core::{
    MetricId, MetricScalar, MetricSet, Observation, ObservationState, UnknownReason,
    UnsupportedReason,
};
use std::time::Duration;

/// One reading, typed for the column that shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Field<T> {
    Value(T),
    Stale {
        value: T,
        age: Duration,
    },
    Unknown(UnknownReason),
    Unsupported(UnsupportedReason),
    PermissionDenied {
        path: String,
    },
    /// The collector produced a report but said nothing about this metric.
    NotCollected,
}

impl<T> Field<T> {
    /// The current value. A stale one is deliberately excluded, matching
    /// `Observation::value`: a caller that wants it must ask for it by name.
    pub fn value(&self) -> Option<&T> {
        match self {
            Field::Value(value) => Some(value),
            _ => None,
        }
    }

    /// The value including a stale one, for a display that labels the age.
    pub fn any_value(&self) -> Option<&T> {
        match self {
            Field::Value(value) | Field::Stale { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn is_value(&self) -> bool {
        matches!(self, Field::Value(_))
    }

    /// The observation state, or `None` for a metric that was never reported.
    pub fn state(&self) -> Option<ObservationState> {
        match self {
            Field::Value(_) => Some(ObservationState::Value),
            Field::Stale { .. } => Some(ObservationState::Stale),
            Field::Unknown(_) => Some(ObservationState::Unknown),
            Field::Unsupported(_) => Some(ObservationState::Unsupported),
            Field::PermissionDenied { .. } => Some(ObservationState::PermissionDenied),
            Field::NotCollected => None,
        }
    }

    pub fn map<U>(self, transform: impl FnOnce(T) -> U) -> Field<U> {
        match self {
            Field::Value(value) => Field::Value(transform(value)),
            Field::Stale { value, age } => Field::Stale {
                value: transform(value),
                age,
            },
            Field::Unknown(reason) => Field::Unknown(reason),
            Field::Unsupported(reason) => Field::Unsupported(reason),
            Field::PermissionDenied { path } => Field::PermissionDenied { path },
            Field::NotCollected => Field::NotCollected,
        }
    }

    /// Build from an observation, given how to narrow the scalar.
    ///
    /// A scalar of the wrong shape — text where a number was expected —
    /// becomes `Unknown(Malformed)` rather than a default, because a collector
    /// disagreeing with the catalog is a fault to report, not to paper over.
    pub fn from_observation(
        observation: Option<&Observation>,
        narrow: impl Fn(&MetricScalar) -> Option<T>,
    ) -> Self {
        let Some(observation) = observation else {
            return Field::NotCollected;
        };
        match observation {
            Observation::Value(scalar) => match narrow(scalar) {
                Some(value) => Field::Value(value),
                None => Field::Unknown(UnknownReason::Malformed {
                    detail: "the reading is not of the type this column shows".into(),
                }),
            },
            Observation::Stale { value, age } => match narrow(value) {
                Some(value) => Field::Stale { value, age: *age },
                None => Field::Unknown(UnknownReason::Malformed {
                    detail: "the reading is not of the type this column shows".into(),
                }),
            },
            Observation::Unknown(reason) => Field::Unknown(reason.clone()),
            Observation::Unsupported(reason) => Field::Unsupported(reason.clone()),
            Observation::PermissionDenied { path } => {
                Field::PermissionDenied { path: path.clone() }
            }
        }
    }
}

impl<T: Copy> Field<T> {
    pub fn copied(&self) -> Option<T> {
        self.value().copied()
    }
}

fn scalar_f64(scalar: &MetricScalar) -> Option<f64> {
    scalar.as_f64()
}

/// A floating-point reading, such as a utilization or a rate.
pub fn number(metrics: &MetricSet, id: &MetricId) -> Field<f64> {
    Field::from_observation(metrics.get(id), scalar_f64)
}

/// An unsigned reading, such as a byte count. A negative float is rejected
/// rather than wrapped.
pub fn unsigned(metrics: &MetricSet, id: &MetricId) -> Field<u64> {
    Field::from_observation(metrics.get(id), |scalar| match scalar {
        MetricScalar::Unsigned(value) => Some(*value),
        MetricScalar::Signed(value) => u64::try_from(*value).ok(),
        MetricScalar::Float(value) if *value >= 0.0 && value.is_finite() => Some(*value as u64),
        _ => None,
    })
}

/// A signed reading, such as a nice value.
pub fn signed(metrics: &MetricSet, id: &MetricId) -> Field<i64> {
    Field::from_observation(metrics.get(id), |scalar| match scalar {
        MetricScalar::Signed(value) => Some(*value),
        MetricScalar::Unsigned(value) => i64::try_from(*value).ok(),
        MetricScalar::Float(value) if value.is_finite() => Some(*value as i64),
        _ => None,
    })
}

/// An identity reading, such as a process name.
pub fn text(metrics: &MetricSet, id: &MetricId) -> Field<String> {
    Field::from_observation(metrics.get(id), |scalar| {
        scalar.as_text().map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> MetricId {
        MetricId::new(raw).unwrap()
    }

    fn set(observation: Observation) -> MetricSet {
        let mut set = MetricSet::new();
        set.insert(id("process.threads"), observation);
        set
    }

    #[test]
    fn a_measured_zero_stays_a_value_and_a_missing_reading_never_becomes_one() {
        let zero = unsigned(&set(Observation::unsigned(0)), &id("process.threads"));
        assert_eq!(zero, Field::Value(0));
        assert_eq!(zero.state(), Some(ObservationState::Value));

        let denied = unsigned(
            &set(Observation::PermissionDenied {
                path: "/proc/9/fd".into(),
            }),
            &id("process.threads"),
        );
        assert_eq!(denied.value(), None);
        assert_eq!(denied.state(), Some(ObservationState::PermissionDenied));
        assert_ne!(denied, zero);
    }

    #[test]
    fn a_metric_the_collector_never_wrote_is_not_the_same_as_one_it_could_not_read() {
        let never = unsigned(&MetricSet::new(), &id("process.threads"));
        assert_eq!(never, Field::NotCollected);
        assert_eq!(never.state(), None);

        let unreadable = unsigned(
            &set(Observation::Unknown(UnknownReason::ReadFailed {
                detail: "eof".into(),
            })),
            &id("process.threads"),
        );
        assert_eq!(unreadable.state(), Some(ObservationState::Unknown));
        assert_ne!(never, unreadable);
    }

    #[test]
    fn a_stale_value_is_readable_only_by_a_caller_that_asked_for_the_age() {
        let stale = number(
            &set(Observation::Stale {
                value: MetricScalar::Float(0.4),
                age: Duration::from_secs(20),
            }),
            &id("process.threads"),
        );
        assert_eq!(stale.value(), None);
        assert_eq!(stale.any_value(), Some(&0.4));
        assert_eq!(stale.state(), Some(ObservationState::Stale));
    }

    #[test]
    fn a_reading_of_the_wrong_shape_is_reported_rather_than_defaulted() {
        let wrong = unsigned(&set(Observation::text("systemd")), &id("process.threads"));
        assert!(matches!(
            wrong,
            Field::Unknown(UnknownReason::Malformed { .. })
        ));
        assert_eq!(wrong.value(), None);
    }

    #[test]
    fn a_negative_signed_reading_is_not_wrapped_into_a_huge_unsigned_one() {
        let negative = unsigned(
            &set(Observation::Value(MetricScalar::Signed(-5))),
            &id("process.threads"),
        );
        assert!(matches!(negative, Field::Unknown(_)));

        let signed_value = signed(
            &set(Observation::Value(MetricScalar::Signed(-5))),
            &id("process.threads"),
        );
        assert_eq!(signed_value, Field::Value(-5));
    }

    #[test]
    fn an_unsupported_reason_survives_the_narrowing() {
        let withheld = text(
            &set(Observation::Unsupported(
                UnsupportedReason::PolicyWithheld {
                    policy: "command lines".into(),
                },
            )),
            &id("process.threads"),
        );
        assert!(matches!(
            withheld,
            Field::Unsupported(UnsupportedReason::PolicyWithheld { .. })
        ));
    }
}
