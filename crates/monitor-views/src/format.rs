//! Turning a reading into the string a cell shows.
//!
//! The formatting lives here rather than in the GUI for one reason: the rule
//! that a non-value is never drawn as a number is only worth anything if it is
//! tested, and testing it must not need a window. Every cell in both tables
//! goes through [`NonValue`], so there is exactly one place where a missing
//! reading could be turned into a zero, and it does not.

use crate::field::Field;
use monitor_core::{UnknownReason, UnsupportedReason};

/// Why a cell has no number, in the form the presentation layer localizes.
///
/// The variants are the states, not the sentences. The GUI owns the words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NonValue {
    /// The collector has not sampled this twice yet.
    NotYetSampled,
    /// Two reads arrived too close together to divide.
    IntervalTooShort,
    /// The read failed this round.
    ReadFailed { detail: String },
    /// The interface produced something the parser did not recognize.
    Malformed { detail: String },
    /// The entity went away mid-scan. A process exiting is the normal case.
    EntityDisappeared,
    /// This host does not have the interface.
    InterfaceMissing { path: String },
    /// The interface exists but says nothing about this entity.
    NotReported { detail: String },
    /// Collection policy withholds it. Command lines are the example.
    PolicyWithheld { policy: String },
    /// The process may not read it.
    PermissionDenied { path: String },
    /// The collector never mentioned this metric at all.
    NotCollected,
}

impl NonValue {
    /// A stable key for locale lookup and tests.
    pub fn key(&self) -> &'static str {
        match self {
            NonValue::NotYetSampled => "not-yet-sampled",
            NonValue::IntervalTooShort => "interval-too-short",
            NonValue::ReadFailed { .. } => "read-failed",
            NonValue::Malformed { .. } => "malformed",
            NonValue::EntityDisappeared => "entity-disappeared",
            NonValue::InterfaceMissing { .. } => "interface-missing",
            NonValue::NotReported { .. } => "not-reported",
            NonValue::PolicyWithheld { .. } => "policy-withheld",
            NonValue::PermissionDenied { .. } => "permission-denied",
            NonValue::NotCollected => "not-collected",
        }
    }

    /// The extra detail worth putting in a tooltip, where there is any.
    pub fn detail(&self) -> Option<&str> {
        match self {
            NonValue::ReadFailed { detail } | NonValue::Malformed { detail } => Some(detail),
            NonValue::NotReported { detail } => Some(detail),
            NonValue::InterfaceMissing { path } | NonValue::PermissionDenied { path } => Some(path),
            NonValue::PolicyWithheld { policy } => Some(policy),
            _ => None,
        }
    }
}

/// What a cell should draw: either the formatted value, or the reason there
/// is none. There is no third case, and no `0` anywhere in this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Cell {
    Value(String),
    /// A value old enough that the collector labelled it stale. It is drawn
    /// differently from a current one.
    Stale {
        text: String,
        age_seconds: u64,
    },
    Missing(NonValue),
}

impl Cell {
    pub fn text(&self) -> Option<&str> {
        match self {
            Cell::Value(text) | Cell::Stale { text, .. } => Some(text),
            Cell::Missing(_) => None,
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, Cell::Missing(_))
    }
}

/// Render a field, given how to format a present value.
pub fn cell<T>(field: &Field<T>, render: impl Fn(&T) -> String) -> Cell {
    match field {
        Field::Value(value) => Cell::Value(render(value)),
        Field::Stale { value, age } => Cell::Stale {
            text: render(value),
            age_seconds: age.as_secs(),
        },
        Field::Unknown(reason) => Cell::Missing(match reason {
            UnknownReason::NotYetSampled => NonValue::NotYetSampled,
            UnknownReason::IntervalTooShort => NonValue::IntervalTooShort,
            UnknownReason::ReadFailed { detail } => NonValue::ReadFailed {
                detail: detail.clone(),
            },
            UnknownReason::Malformed { detail } => NonValue::Malformed {
                detail: detail.clone(),
            },
            UnknownReason::EntityDisappeared => NonValue::EntityDisappeared,
        }),
        Field::Unsupported(reason) => Cell::Missing(match reason {
            UnsupportedReason::InterfaceMissing { path } => {
                NonValue::InterfaceMissing { path: path.clone() }
            }
            UnsupportedReason::NotReported { detail } => NonValue::NotReported {
                detail: detail.clone(),
            },
            UnsupportedReason::PolicyWithheld { policy } => NonValue::PolicyWithheld {
                policy: policy.clone(),
            },
        }),
        Field::PermissionDenied { path } => {
            Cell::Missing(NonValue::PermissionDenied { path: path.clone() })
        }
        Field::NotCollected => Cell::Missing(NonValue::NotCollected),
    }
}

/// A byte count in binary units, which is what every other Linux tool shows
/// for memory.
pub fn bytes(value: f64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut scaled = value.abs();
    let mut unit = 0;
    while scaled >= 1024.0 && unit + 1 < UNITS.len() {
        scaled /= 1024.0;
        unit += 1;
    }
    let sign = if value < 0.0 { "-" } else { "" };
    if unit == 0 {
        format!("{sign}{scaled:.0} {}", UNITS[unit])
    } else if scaled < 10.0 {
        format!("{sign}{scaled:.2} {}", UNITS[unit])
    } else {
        format!("{sign}{scaled:.1} {}", UNITS[unit])
    }
}

pub fn bytes_per_second(value: f64) -> String {
    format!("{}/s", bytes(value))
}

/// A ratio in `0.0..=1.0` as a percentage.
pub fn ratio_percent(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

/// A percentage the kernel already reports as one, such as PSI.
pub fn percent(value: f64) -> String {
    format!("{value:.1}%")
}

/// A duration in the largest unit that keeps it readable.
pub fn duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let whole = seconds as u64;
    let (hours, minutes, remainder) = (whole / 3600, (whole % 3600) / 60, whole % 60);
    if hours >= 24 {
        format!("{}d {}h", hours / 24, hours % 24)
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {remainder:02}s")
    } else if seconds < 10.0 {
        format!("{seconds:.2}s")
    } else {
        format!("{remainder}s")
    }
}

pub fn count(value: u64) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_missing_reading_never_renders_as_a_number() {
        for field in [
            Field::<u64>::Unknown(UnknownReason::NotYetSampled),
            Field::Unsupported(UnsupportedReason::NotReported {
                detail: "kernel thread".into(),
            }),
            Field::PermissionDenied {
                path: "/proc/9/fd".into(),
            },
            Field::NotCollected,
        ] {
            let rendered = cell(&field, |value| count(*value));
            assert!(rendered.is_missing(), "{field:?}");
            assert_eq!(rendered.text(), None);
        }
        // And a real zero still renders as one.
        assert_eq!(
            cell(&Field::Value(0u64), |value| count(*value)),
            Cell::Value("0".into())
        );
    }

    #[test]
    fn every_reason_a_cell_can_be_empty_has_its_own_key() {
        let reasons = [
            NonValue::NotYetSampled,
            NonValue::IntervalTooShort,
            NonValue::ReadFailed {
                detail: String::new(),
            },
            NonValue::Malformed {
                detail: String::new(),
            },
            NonValue::EntityDisappeared,
            NonValue::InterfaceMissing {
                path: String::new(),
            },
            NonValue::NotReported {
                detail: String::new(),
            },
            NonValue::PolicyWithheld {
                policy: String::new(),
            },
            NonValue::PermissionDenied {
                path: String::new(),
            },
            NonValue::NotCollected,
        ];
        let keys: std::collections::BTreeSet<&str> = reasons.iter().map(NonValue::key).collect();
        assert_eq!(keys.len(), reasons.len());
    }

    #[test]
    fn a_withheld_command_line_is_distinguishable_from_a_missing_one() {
        let withheld = cell(
            &Field::<String>::Unsupported(UnsupportedReason::PolicyWithheld {
                policy: "command lines".into(),
            }),
            |value| value.clone(),
        );
        let missing = cell(&Field::<String>::NotCollected, |value| value.clone());
        assert_ne!(withheld, missing);
        assert_eq!(
            withheld,
            Cell::Missing(NonValue::PolicyWithheld {
                policy: "command lines".into()
            })
        );
    }

    #[test]
    fn a_stale_value_is_drawn_with_its_age_rather_than_as_current() {
        let stale = cell(
            &Field::Stale {
                value: 42u64,
                age: Duration::from_secs(90),
            },
            |value| count(*value),
        );
        assert_eq!(
            stale,
            Cell::Stale {
                text: "42".into(),
                age_seconds: 90
            }
        );
        assert!(!stale.is_missing());
    }

    #[test]
    fn byte_counts_use_binary_units_at_a_readable_precision() {
        assert_eq!(bytes(0.0), "0 B");
        assert_eq!(bytes(512.0), "512 B");
        assert_eq!(bytes(1536.0), "1.50 KiB");
        assert_eq!(bytes(20.0 * 1024.0), "20.0 KiB");
        assert_eq!(bytes(1024.0 * 1024.0 * 1024.0), "1.00 GiB");
        assert_eq!(bytes_per_second(2048.0), "2.00 KiB/s");
    }

    #[test]
    fn ratios_and_kernel_percentages_are_not_confused_with_each_other() {
        assert_eq!(ratio_percent(0.125), "12.5%");
        assert_eq!(percent(12.5), "12.5%");
    }

    #[test]
    fn durations_pick_the_unit_that_stays_readable() {
        assert_eq!(duration(0.5), "0.50s");
        assert_eq!(duration(45.0), "45s");
        assert_eq!(duration(90.0), "1m 30s");
        assert_eq!(duration(3_700.0), "1h 01m");
        assert_eq!(duration(200_000.0), "2d 7h");
        assert_eq!(duration(-1.0), "0.00s");
    }
}
