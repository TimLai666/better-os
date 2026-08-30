//! Shared descriptor construction.
//!
//! Every metric this crate can emit is declared here-adjacent, in its own
//! collector module, through these builders. The builders exist so that unit,
//! semantic type, source, and sampling behaviour cannot be filled in
//! inconsistently across six collectors.

use monitor_core::{
    CollectorId, MetricDescriptor, MetricId, MetricSource, SamplingBehavior, SemanticType, Unit,
};
use std::time::Duration;

/// How long an instantaneous reading stays current.
///
/// The default collection cadence is not decided yet — the specification lists
/// it as a deferred decision — so this is a deliberately generous budget that
/// only catches a genuinely stalled collector rather than encoding a cadence
/// by the back door.
pub const INSTANT_FRESHNESS: Duration = Duration::from_secs(15);

/// How long a derived rate stays current.
pub const RATE_FRESHNESS: Duration = Duration::from_secs(30);

/// Below this interval a counter delta is dominated by counter resolution.
///
/// `/proc/stat` and `/proc/diskstats` advance in USER_HZ ticks and
/// milliseconds respectively, so a 100 ms window can differ by a whole unit
/// purely from rounding.
pub const MINIMUM_DELTA_INTERVAL: Duration = Duration::from_millis(250);

/// A metric identifier from the crate's own catalog.
///
/// Catalog names are crate constants rather than user input, so a name that
/// does not satisfy `MetricId` is a programming error and panics here. The
/// `catalog_is_well_formed` test in each collector module is what keeps that
/// promise true.
pub fn metric_id(raw: &str) -> MetricId {
    MetricId::new(raw).expect("catalog metric id must be well formed")
}

pub fn collector_id(raw: &str) -> CollectorId {
    CollectorId::new(raw).expect("catalog collector id must be well formed")
}

pub fn gauge(
    id: &str,
    unit: Unit,
    source: MetricSource,
    summary: impl Into<String>,
) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        unit,
        SemanticType::Gauge,
        source,
        SamplingBehavior::instant(INSTANT_FRESHNESS),
        summary,
    )
}

pub fn identity(id: &str, source: MetricSource, summary: impl Into<String>) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        Unit::None,
        SemanticType::Identity,
        source,
        SamplingBehavior::instant(INSTANT_FRESHNESS),
        summary,
    )
}

pub fn counter(
    id: &str,
    unit: Unit,
    source: MetricSource,
    summary: impl Into<String>,
) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        unit,
        SemanticType::Counter,
        source,
        SamplingBehavior::instant(INSTANT_FRESHNESS),
        summary,
    )
}

pub fn rate(
    id: &str,
    unit: Unit,
    source: MetricSource,
    summary: impl Into<String>,
) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        unit,
        SemanticType::Rate,
        source,
        SamplingBehavior::counter_delta(MINIMUM_DELTA_INTERVAL, RATE_FRESHNESS),
        summary,
    )
}

pub fn utilization(id: &str, source: MetricSource, summary: impl Into<String>) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        Unit::Ratio,
        SemanticType::Utilization,
        source,
        SamplingBehavior::counter_delta(MINIMUM_DELTA_INTERVAL, RATE_FRESHNESS),
        summary,
    )
}

pub fn saturation(
    id: &str,
    unit: Unit,
    source: MetricSource,
    summary: impl Into<String>,
) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        unit,
        SemanticType::Saturation,
        source,
        SamplingBehavior::instant(INSTANT_FRESHNESS),
        summary,
    )
}

/// A value the kernel already averaged over its own window, such as PSI
/// `avg10`. The collector must present it as-is rather than averaging again.
pub fn kernel_averaged(
    id: &str,
    unit: Unit,
    source: MetricSource,
    summary: impl Into<String>,
) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        unit,
        SemanticType::Saturation,
        source,
        SamplingBehavior::kernel_averaged(INSTANT_FRESHNESS),
        summary,
    )
}

pub fn latency(id: &str, source: MetricSource, summary: impl Into<String>) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        Unit::Milliseconds,
        SemanticType::Latency,
        source,
        SamplingBehavior::counter_delta(MINIMUM_DELTA_INTERVAL, RATE_FRESHNESS),
        summary,
    )
}

pub fn errors(id: &str, source: MetricSource, summary: impl Into<String>) -> MetricDescriptor {
    MetricDescriptor::new(
        metric_id(id),
        Unit::CountPerSecond,
        SemanticType::Errors,
        source,
        SamplingBehavior::counter_delta(MINIMUM_DELTA_INTERVAL, RATE_FRESHNESS),
        summary,
    )
}

pub fn proc_source(relative: &str) -> MetricSource {
    MetricSource::Proc(relative.to_string())
}

pub fn sys_source(relative: &str) -> MetricSource {
    MetricSource::Sys(relative.to_string())
}

pub fn derived_source(from: &str) -> MetricSource {
    MetricSource::Derived(from.to_string())
}
