//! What a collector actually observed.
//!
//! The whole point of this type is that "no number" is not one state. A metric
//! the kernel never exposes, a metric this process may not read, a metric that
//! has not been sampled twice yet, a value that is too old to trust, and a
//! genuine measurement of zero are five different answers, and a monitor that
//! collapses them into `0` or `None` will tell the user the machine is idle
//! when it is actually unobserved.

use crate::metric::{MetricId, SupportState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// A measured quantity, in the unit its descriptor declares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricScalar {
    Unsigned(u64),
    Signed(i64),
    Float(f64),
    Bool(bool),
    /// An identity or enumerated state, such as a process name or a link
    /// operational state.
    Text(String),
}

impl MetricScalar {
    /// The value as a float where that is meaningful. Identity values return
    /// `None` rather than a number a chart could plot by accident.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            MetricScalar::Unsigned(value) => Some(*value as f64),
            MetricScalar::Signed(value) => Some(*value as f64),
            MetricScalar::Float(value) => Some(*value),
            MetricScalar::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
            MetricScalar::Text(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            MetricScalar::Text(value) => Some(value),
            _ => None,
        }
    }
}

/// Why this host cannot produce a metric.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason {
    /// The kernel interface the metric is read from does not exist here. PSI
    /// on a kernel built without `CONFIG_PSI` is the common case.
    InterfaceMissing { path: String },
    /// The interface exists but reports nothing for this entity — a wireless
    /// interface with no `speed`, an AMD package sensor with no per-core
    /// label.
    NotReported { detail: String },
    /// Collection is configured not to gather this. Process command lines are
    /// withheld by default; that is a policy decision, not a kernel one, and
    /// it must not read as if the kernel were silent.
    PolicyWithheld { policy: String },
}

/// Why a metric that this host does support has no current value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    /// A counter-delta metric that has only been read once so far.
    NotYetSampled,
    /// Two reads arrived closer together than the metric's minimum interval,
    /// so the delta would be noise.
    IntervalTooShort,
    /// The interface was there and readable, but the read failed this time.
    ReadFailed { detail: String },
    /// The interface produced bytes that do not match its documented format.
    Malformed { detail: String },
    /// The entity vanished between enumeration and reading — a process that
    /// exited mid-scan.
    EntityDisappeared,
}

/// One collector's answer for one metric.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    /// A real measurement. `Value(Unsigned(0))` means the counter really is
    /// zero, and is not interchangeable with any other variant here.
    Value(MetricScalar),
    /// The last good value, kept because it is still worth showing, but older
    /// than its freshness budget and labelled as such.
    Stale {
        value: MetricScalar,
        age: Duration,
    },
    Unknown(UnknownReason),
    Unsupported(UnsupportedReason),
    PermissionDenied {
        path: String,
    },
}

/// The five-way distinction, flattened for assertions and for coverage
/// reporting that does not care about the detail.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationState {
    Value,
    Stale,
    Unknown,
    Unsupported,
    PermissionDenied,
}

impl Observation {
    pub fn unsigned(value: u64) -> Self {
        Observation::Value(MetricScalar::Unsigned(value))
    }

    pub fn float(value: f64) -> Self {
        Observation::Value(MetricScalar::Float(value))
    }

    pub fn boolean(value: bool) -> Self {
        Observation::Value(MetricScalar::Bool(value))
    }

    pub fn text(value: impl Into<String>) -> Self {
        Observation::Value(MetricScalar::Text(value.into()))
    }

    pub fn state(&self) -> ObservationState {
        match self {
            Observation::Value(_) => ObservationState::Value,
            Observation::Stale { .. } => ObservationState::Stale,
            Observation::Unknown(_) => ObservationState::Unknown,
            Observation::Unsupported(_) => ObservationState::Unsupported,
            Observation::PermissionDenied { .. } => ObservationState::PermissionDenied,
        }
    }

    /// The current value, if there is one. A stale value is deliberately not
    /// returned here: a caller that wants it has to acknowledge the age.
    pub fn value(&self) -> Option<&MetricScalar> {
        match self {
            Observation::Value(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        self.value().and_then(MetricScalar::as_f64)
    }

    pub fn as_text(&self) -> Option<&str> {
        self.value().and_then(MetricScalar::as_text)
    }

    /// What this observation says about whether the host supports the metric.
    ///
    /// A value or a stale value proves support. `Unknown` proves nothing
    /// either way, which is exactly why it is not the same answer as
    /// `Unsupported`.
    pub fn support_state(&self) -> SupportState {
        match self {
            Observation::Value(_) | Observation::Stale { .. } => SupportState::Supported,
            Observation::Unknown(_) => SupportState::Unknown,
            Observation::Unsupported(reason) => SupportState::Unsupported(reason.clone()),
            Observation::PermissionDenied { path } => {
                SupportState::PermissionDenied { path: path.clone() }
            }
        }
    }
}

/// The observations one collector produced for one entity, keyed by metric.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MetricSet(BTreeMap<MetricId, Observation>);

impl MetricSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: MetricId, observation: Observation) -> &mut Self {
        self.0.insert(id, observation);
        self
    }

    pub fn get(&self, id: &MetricId) -> Option<&Observation> {
        self.0.get(id)
    }

    /// The state of a metric, treating an entry this collector never wrote as
    /// `Unknown` rather than as absent.
    pub fn state_of(&self, id: &MetricId) -> ObservationState {
        self.0
            .get(id)
            .map(Observation::state)
            .unwrap_or(ObservationState::Unknown)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&MetricId, &Observation)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(MetricId, Observation)> for MetricSet {
    fn from_iter<T: IntoIterator<Item = (MetricId, Observation)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> MetricId {
        MetricId::new(raw).unwrap()
    }

    #[test]
    fn a_measured_zero_is_not_any_kind_of_missing_value() {
        let zero = Observation::unsigned(0);
        assert_eq!(zero.state(), ObservationState::Value);
        assert_eq!(zero.as_f64(), Some(0.0));
        assert_eq!(zero.support_state(), SupportState::Supported);

        for missing in [
            Observation::Unknown(UnknownReason::NotYetSampled),
            Observation::Unsupported(UnsupportedReason::InterfaceMissing {
                path: "/proc/pressure/cpu".into(),
            }),
            Observation::PermissionDenied {
                path: "/proc/1/fd".into(),
            },
        ] {
            assert_ne!(missing, zero);
            assert_ne!(missing.state(), ObservationState::Value);
            assert_eq!(missing.as_f64(), None);
        }
    }

    #[test]
    fn the_five_states_are_all_distinct() {
        let states = [
            Observation::unsigned(0).state(),
            Observation::Stale {
                value: MetricScalar::Unsigned(0),
                age: Duration::from_secs(30),
            }
            .state(),
            Observation::Unknown(UnknownReason::NotYetSampled).state(),
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "no per-core label".into(),
            })
            .state(),
            Observation::PermissionDenied {
                path: "/proc/1/fd".into(),
            }
            .state(),
        ];
        let unique: std::collections::BTreeSet<_> = states.iter().copied().collect();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn a_stale_value_is_not_offered_as_a_current_value() {
        let stale = Observation::Stale {
            value: MetricScalar::Float(41.5),
            age: Duration::from_secs(90),
        };
        assert_eq!(stale.state(), ObservationState::Stale);
        assert_eq!(stale.value(), None);
        assert_eq!(stale.as_f64(), None);
        // It still proves the host can produce the metric.
        assert_eq!(stale.support_state(), SupportState::Supported);
    }

    #[test]
    fn unknown_does_not_claim_the_host_lacks_the_metric() {
        let unknown = Observation::Unknown(UnknownReason::NotYetSampled);
        assert_eq!(unknown.support_state(), SupportState::Unknown);

        let unsupported = Observation::Unsupported(UnsupportedReason::InterfaceMissing {
            path: "/proc/pressure/io".into(),
        });
        assert_ne!(unknown.support_state(), unsupported.support_state());
    }

    #[test]
    fn a_withheld_command_line_is_distinguishable_from_a_missing_interface() {
        let withheld = Observation::Unsupported(UnsupportedReason::PolicyWithheld {
            policy: "process.command_line".into(),
        });
        let missing = Observation::Unsupported(UnsupportedReason::InterfaceMissing {
            path: "/proc/42/cmdline".into(),
        });
        assert_eq!(withheld.state(), missing.state());
        assert_ne!(withheld, missing);
    }

    #[test]
    fn identity_values_are_never_plottable_as_numbers() {
        let name = Observation::text("systemd");
        assert_eq!(name.as_text(), Some("systemd"));
        assert_eq!(name.as_f64(), None);
    }

    #[test]
    fn a_metric_the_collector_never_wrote_reads_as_unknown() {
        let mut set = MetricSet::new();
        set.insert(id("cpu.utilization.busy"), Observation::float(0.12));
        assert_eq!(
            set.state_of(&id("cpu.utilization.busy")),
            ObservationState::Value
        );
        assert_eq!(
            set.state_of(&id("cpu.utilization.steal")),
            ObservationState::Unknown
        );
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn a_metric_set_round_trips_through_json() {
        let set: MetricSet = [
            (id("cpu.utilization.busy"), Observation::float(0.5)),
            (
                id("cpu.temperature"),
                Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "k10temp exposes Tctl only".into(),
                }),
            ),
        ]
        .into_iter()
        .collect();
        let encoded = serde_json::to_string(&set).unwrap();
        let decoded: MetricSet = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, set);
    }
}
