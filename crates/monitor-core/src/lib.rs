//! Observation contracts for Better Monitor.
//!
//! This crate owns the vocabulary every other Better Monitor crate speaks:
//! what a metric is, what a collector may say about it, and how a reading that
//! does not exist is distinguished from a reading of zero. It knows nothing
//! about Linux, GPUI, or storage, and it must not learn.

pub mod action;
pub mod collector;
pub mod metric;
pub mod observation;
pub mod store;

pub use action::{
    ActionAvailability, ActionError, ActionOutcome, ActionRefusal, NICE_MAXIMUM, NICE_MINIMUM,
    ProcessAction, ProcessController, ProcessTarget, SignalKind, unprivileged_availability,
};
pub use collector::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MAX_COLLECTOR_ID_LENGTH, Timestamp,
};
pub use metric::{
    MAX_METRIC_ID_LENGTH, MetricCapability, MetricDescriptor, MetricId, MetricSource,
    SamplingBehavior, SamplingKind, SemanticType, SupportState, Unit,
};
pub use observation::{
    MetricScalar, MetricSet, Observation, ObservationState, UnknownReason, UnsupportedReason,
};
pub use store::{CoverageCounts, Incident, InventoryItem, MonitorStore};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MonitorError {
    #[error("collector failed: {0}")]
    Collector(String),
    #[error("export failed: {0}")]
    Export(String),
    #[error("invalid metric id: {0}")]
    InvalidMetricId(String),
    #[error("invalid collector id: {0}")]
    InvalidCollectorId(String),
}
