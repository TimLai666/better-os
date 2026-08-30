//! View models for Better Monitor.
//!
//! Everything the Apps, Processes, and Overview screens decide lives here, and
//! none of it depends on GPUI. Grouping, sorting, filtering, tree building,
//! aggregation, verdicts, and the rule that a missing reading is never drawn
//! as a zero are all testable without a display server, which is the only way
//! they can be tested at all in CI.
//!
//! The GUI above this crate reads models and draws them. It does not read
//! `/proc`, hold a privileged handle, or decide what a number means.

pub mod apps;
pub mod facts;
pub mod field;
pub mod format;
pub mod grouping;
pub mod overview;
pub mod process_table;

pub use apps::{Aggregate, AppRow, AppSort, AppsModel};
pub use facts::ProcessFacts;
pub use field::Field;
pub use format::{Cell, NonValue};
pub use grouping::{
    AppGroup, AppKind, Confidence, EvidenceKind, Grouping, GroupingEvidence, GroupingPrecedence,
    MemberAttribution, group_processes,
};
pub use overview::{
    CollectorStatus, CoverageSummary, MemorySummary, OverviewModel, ResourceSummary,
    ResourceVerdict, ThrottlingState, ThroughputSummary,
};
pub use process_table::{ProcessColumn, ProcessTableModel, SortDirection, VisibleRow};
