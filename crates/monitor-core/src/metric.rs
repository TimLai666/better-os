//! Metric identity.
//!
//! A number on its own is not observable data. Better Monitor only accepts a
//! reading next to a descriptor that says what the number counts, what unit it
//! is in, which kernel interface produced it, and how it has to be sampled to
//! mean anything. Everything downstream — storage, analysis, export, and the
//! GUI — reads those five facts instead of guessing from a field name.

use crate::MonitorError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// The longest metric identifier a store column, export key, or chart legend
/// can carry without being truncated somewhere along the way.
pub const MAX_METRIC_ID_LENGTH: usize = 96;

/// A dotted, lowercase metric name such as `cpu.utilization.system`.
///
/// The character set is closed so a metric name is always safe as a JSON key,
/// a file name, and a store column, and so a collector cannot invent a name
/// that a later storage format has to escape.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MetricId(String);

impl MetricId {
    pub fn new(value: impl Into<String>) -> Result<Self, MonitorError> {
        let value = value.into();
        let shaped = !value.is_empty()
            && value.len() <= MAX_METRIC_ID_LENGTH
            && value.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || character == '.'
                    || character == '_'
            })
            && !value.starts_with('.')
            && !value.ends_with('.')
            && !value.contains("..");
        if !shaped {
            return Err(MonitorError::InvalidMetricId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MetricId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The physical unit a reading is expressed in.
///
/// Conversion happens in the collector, once, so no consumer has to remember
/// that `/proc/meminfo` counts kibibytes or that `scaling_cur_freq` counts
/// kilohertz.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// A dimensionless identity or enumerated state.
    None,
    /// A fraction in `0.0..=1.0`. Utilization uses this, not percent, so
    /// rounding for display happens once in the presentation layer.
    Ratio,
    /// A percentage in `0.0..=100.0`. Only used where the kernel itself
    /// reports percent, as PSI does.
    Percent,
    Bytes,
    BytesPerSecond,
    BitsPerSecond,
    Count,
    CountPerSecond,
    Seconds,
    Milliseconds,
    Microseconds,
    Hertz,
    DegreesCelsius,
    Watts,
    /// A count of memory pages, kept distinct from `Count` because the page
    /// size is a property of the host rather than of the metric.
    Pages,
}

impl Unit {
    pub fn symbol(self) -> &'static str {
        match self {
            Unit::None => "",
            Unit::Ratio => "",
            Unit::Percent => "%",
            Unit::Bytes => "B",
            Unit::BytesPerSecond => "B/s",
            Unit::BitsPerSecond => "bit/s",
            Unit::Count => "",
            Unit::CountPerSecond => "/s",
            Unit::Seconds => "s",
            Unit::Milliseconds => "ms",
            Unit::Microseconds => "us",
            Unit::Hertz => "Hz",
            Unit::DegreesCelsius => "degC",
            Unit::Watts => "W",
            Unit::Pages => "pages",
        }
    }
}

/// What kind of statement the number makes about the system.
///
/// This is the distinction the product depends on: a monitor that only knows
/// utilization cannot tell a busy machine from a stalled one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticType {
    /// A level that is true at the instant it was read.
    Gauge,
    /// A monotonic total since boot. Only a difference between two reads is
    /// meaningful.
    Counter,
    /// A per-second value already derived from two counter reads.
    Rate,
    /// The fraction of a resource's capacity that was busy over an interval.
    Utilization,
    /// How much work waited beyond what the resource could serve: pressure,
    /// load average, queue depth, blocked tasks.
    Saturation,
    /// How long an operation took.
    Latency,
    /// Failures, drops, and rejections.
    Errors,
    /// A name, path, model, or enumerated state rather than a quantity.
    Identity,
}

/// How many raw reads a value needs, and how far apart they must be.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingKind {
    /// One read is the answer.
    Instant,
    /// Two reads of a monotonic counter, divided by the elapsed interval.
    /// The first read of a session can only produce `Unknown`.
    CounterDelta,
    /// The kernel already averaged this over its own window; the collector
    /// must not average it again.
    KernelAveraged,
}

/// The sampling contract a reading was produced under.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SamplingBehavior {
    pub kind: SamplingKind,
    /// Two reads closer together than this cannot produce an honest rate,
    /// because counter resolution dominates. The collector reports `Unknown`
    /// rather than a value shaped by rounding.
    pub minimum_interval: Duration,
    /// A retained value older than this is reported as `Stale`. It is never
    /// presented as if it were current.
    pub freshness_budget: Duration,
}

impl SamplingBehavior {
    pub fn instant(freshness_budget: Duration) -> Self {
        Self {
            kind: SamplingKind::Instant,
            minimum_interval: Duration::ZERO,
            freshness_budget,
        }
    }

    pub fn counter_delta(minimum_interval: Duration, freshness_budget: Duration) -> Self {
        Self {
            kind: SamplingKind::CounterDelta,
            minimum_interval,
            freshness_budget,
        }
    }

    pub fn kernel_averaged(freshness_budget: Duration) -> Self {
        Self {
            kind: SamplingKind::KernelAveraged,
            minimum_interval: Duration::ZERO,
            freshness_budget,
        }
    }
}

/// Where the number actually came from.
///
/// The product requirement is that no metric can claim a value without naming
/// a verifiable origin, so this is part of the contract rather than a comment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricSource {
    /// A file under `/proc`, given relative to the proc root, for example
    /// `stat` or `pressure/cpu`.
    Proc(String),
    /// A file under `/sys`, given relative to the sys root.
    Sys(String),
    /// Computed from other readings rather than read from any single file.
    /// The string names the inputs.
    Derived(String),
    /// Supplied by a third-party crate, named here so a licence and semantics
    /// audit has something to point at.
    Crate(String),
}

/// Everything a consumer needs in order to interpret a reading.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricDescriptor {
    pub id: MetricId,
    pub unit: Unit,
    pub semantic: SemanticType,
    pub source: MetricSource,
    pub sampling: SamplingBehavior,
    /// One line saying what the number counts, in terms of the kernel
    /// interface rather than in terms of a screen label.
    pub summary: String,
}

impl MetricDescriptor {
    pub fn new(
        id: MetricId,
        unit: Unit,
        semantic: SemanticType,
        source: MetricSource,
        sampling: SamplingBehavior,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            id,
            unit,
            semantic,
            source,
            sampling,
            summary: summary.into(),
        }
    }
}

/// Whether this host can produce a metric at all.
///
/// This answers a different question from a reading. `Unsupported` is a fact
/// about the machine, while a missing reading may only mean the collector has
/// not run twice yet.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportState {
    Supported,
    Unsupported(crate::UnsupportedReason),
    PermissionDenied {
        path: String,
    },
    /// Not probed yet, or probed and the answer was itself unreadable.
    Unknown,
}

/// A descriptor paired with what this host can actually do with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricCapability {
    pub descriptor: MetricDescriptor,
    pub support: SupportState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_dotted_lowercase_identifier() {
        let id = MetricId::new("cpu.utilization.system").unwrap();
        assert_eq!(id.as_str(), "cpu.utilization.system");
        assert_eq!(id.to_string(), "cpu.utilization.system");
    }

    #[test]
    fn rejects_identifiers_that_would_need_escaping() {
        for candidate in [
            "",
            "CPU.total",
            "cpu total",
            ".cpu",
            "cpu.",
            "cpu..total",
            "cpu/total",
            "cpu-total",
        ] {
            assert!(
                MetricId::new(candidate).is_err(),
                "expected {candidate:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_an_identifier_longer_than_the_column_budget() {
        let long = "a".repeat(MAX_METRIC_ID_LENGTH + 1);
        assert!(MetricId::new(long).is_err());
        let limit = "a".repeat(MAX_METRIC_ID_LENGTH);
        assert!(MetricId::new(limit).is_ok());
    }

    #[test]
    fn a_counter_delta_carries_the_interval_below_which_it_refuses_to_answer() {
        let sampling =
            SamplingBehavior::counter_delta(Duration::from_millis(200), Duration::from_secs(5));
        assert_eq!(sampling.kind, SamplingKind::CounterDelta);
        assert_eq!(sampling.minimum_interval, Duration::from_millis(200));
        assert_eq!(sampling.freshness_budget, Duration::from_secs(5));
    }

    #[test]
    fn an_instant_metric_needs_no_minimum_interval() {
        let sampling = SamplingBehavior::instant(Duration::from_secs(2));
        assert_eq!(sampling.minimum_interval, Duration::ZERO);
    }

    #[test]
    fn a_descriptor_round_trips_through_json() {
        let descriptor = MetricDescriptor::new(
            MetricId::new("memory.available").unwrap(),
            Unit::Bytes,
            SemanticType::Gauge,
            MetricSource::Proc("meminfo".into()),
            SamplingBehavior::instant(Duration::from_secs(5)),
            "MemAvailable converted from kibibytes to bytes",
        );
        let encoded = serde_json::to_string(&descriptor).unwrap();
        let decoded: MetricDescriptor = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, descriptor);
    }
}
