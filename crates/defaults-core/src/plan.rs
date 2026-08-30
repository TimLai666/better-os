//! Serializable apply and restore plans, and what running one produced.
//!
//! A plan is reviewed before anything is written, and it is the same plan
//! whether one component or every component was selected. It serializes so a
//! diagnostic report can carry exactly what was going to happen, or exactly
//! what did.

use better_core::defaults::{
    AdapterId, DefaultsValue, HealthPrerequisite, IntegrationId, IntegrationKind, ObservedValue,
    SessionEffect,
};
use better_core::manifest::ComponentId;
use serde::{Deserialize, Serialize};

pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// Which components an operation covers. The global action and a single
/// component's action differ only in this value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    All,
    Components(Vec<ComponentId>),
}

impl Selection {
    pub fn one(component: ComponentId) -> Self {
        Self::Components(vec![component])
    }

    pub fn covers(&self, component: &ComponentId) -> bool {
        match self {
            Self::All => true,
            Self::Components(components) => components.contains(component),
        }
    }
}

/// Whether a plan applies Better OS defaults or puts back what was there.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    Apply,
    Restore,
}

/// Why an entry is in the plan but will not be changed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "skipped", rename_all = "snake_case")]
pub enum SkipReason {
    AlreadyDefault,
    AlreadyRestored,
    NotApplicableHere,
    PrerequisiteNotMet {
        prerequisite: HealthPrerequisite,
    },
    RequiresAdministrator,
    NoProductionAdapter {
        adapter: AdapterId,
    },
    NothingCaptured,
    EffectiveValueUnknown {
        reason: String,
    },
    /// The value changed after Better Manager last wrote it and the user has
    /// not said to overwrite that change.
    ChangedExternallyWithoutConfirmation {
        current: ObservedValue,
    },
    Conflict {
        claimant: ComponentId,
    },
}

/// Something the reviewer needs to know about an entry that will be changed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "warning", rename_all = "snake_case")]
pub enum PlanWarning {
    /// The change is stored immediately but takes effect at the next session.
    NeedsSignOut,
    NeedsRestart,
    /// The value being overwritten was never read definitely, so this change
    /// cannot be undone by writing the old value back.
    PreviousValueIndeterminate,
    /// The entry changed outside Better Manager and the user confirmed the
    /// overwrite anyway.
    OverwritesExternalChange {
        current: ObservedValue,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PlanAction {
    Apply { to: DefaultsValue },
    Restore { to: ObservedValue },
    Skip { reason: SkipReason },
}

impl PlanAction {
    pub fn changes_something(&self) -> bool {
        !matches!(self, Self::Skip { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub component: ComponentId,
    pub integration: IntegrationId,
    pub kind: IntegrationKind,
    pub adapter: AdapterId,
    pub action: PlanAction,
    /// What the setting says right now, read while the plan was built.
    pub current: ObservedValue,
    /// What Better OS wants it to say.
    pub desired: DefaultsValue,
    /// What was captured before Better OS first changed it, when anything was.
    pub captured_previous: Option<ObservedValue>,
    pub session_effect: SessionEffect,
    /// Whether this entry needed an explicit confirmation to proceed.
    pub requires_confirmation: bool,
    pub confirmed: bool,
    pub warnings: Vec<PlanWarning>,
}

/// A reviewed set of changes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DefaultsPlan {
    pub schema_version: u32,
    pub kind: PlanKind,
    pub entries: Vec<PlanEntry>,
    /// Snapshots on disk that could not be read when this plan was built.
    pub damaged_snapshots: Vec<String>,
}

impl DefaultsPlan {
    pub fn new(kind: PlanKind, entries: Vec<PlanEntry>, damaged_snapshots: Vec<String>) -> Self {
        Self {
            schema_version: PLAN_SCHEMA_VERSION,
            kind,
            entries,
            damaged_snapshots,
        }
    }

    /// The entries that would change something.
    pub fn changes(&self) -> impl Iterator<Item = &PlanEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.action.changes_something())
    }

    pub fn is_empty(&self) -> bool {
        self.changes().next().is_none()
    }

    /// Entries held back because they changed outside Better Manager. They are
    /// the ones a review screen has to ask about one at a time.
    pub fn awaiting_confirmation(&self) -> impl Iterator<Item = &PlanEntry> {
        self.entries.iter().filter(|entry| {
            matches!(
                entry.action,
                PlanAction::Skip {
                    reason: SkipReason::ChangedExternallyWithoutConfirmation { .. }
                }
            )
        })
    }
}

/// The entries the user has explicitly agreed to overwrite despite an external
/// change. Nothing else can lift that hold.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Confirmations {
    entries: Vec<(ComponentId, IntegrationId)>,
}

impl Confirmations {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn with(mut self, component: ComponentId, integration: IntegrationId) -> Self {
        self.entries.push((component, integration));
        self
    }

    pub fn contains(&self, component: &ComponentId, integration: &IntegrationId) -> bool {
        self.entries
            .iter()
            .any(|(left, right)| left == component && right == integration)
    }
}

/// What happened to one entry when the plan ran.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum EntryOutcome {
    /// Written and confirmed by a second read.
    Applied {
        value: DefaultsValue,
    },
    /// Written and confirmed, and the declaration says it is not effective
    /// until the session ends.
    AppliedNeedsSignOut {
        value: DefaultsValue,
    },
    /// Put back and confirmed by a second read.
    Restored {
        value: ObservedValue,
    },
    /// The setting already said this, so nothing was written.
    AlreadyCorrect,
    /// The write reported success and the verifying read disagreed. This is
    /// reported as its own outcome rather than folded into success.
    NotVerified {
        observed: ObservedValue,
    },
    /// The write reported success and the verifying read could not tell.
    VerificationInconclusive {
        observed: ObservedValue,
    },
    Skipped {
        reason: SkipReason,
    },
    ManualActionRequired {
        reason: String,
        detail: Option<String>,
    },
    Failed {
        reason: String,
        detail: Option<String>,
    },
}

impl EntryOutcome {
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            Self::Applied { .. }
                | Self::AppliedNeedsSignOut { .. }
                | Self::Restored { .. }
                | Self::AlreadyCorrect
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntryResult {
    pub component: ComponentId,
    pub integration: IntegrationId,
    pub outcome: EntryOutcome,
}

/// The exact per-entry result of running a plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DefaultsOutcome {
    pub kind: PlanKind,
    pub results: Vec<EntryResult>,
    /// The snapshot id written before the first change, when one was needed.
    pub baseline_snapshot: Option<String>,
    /// The snapshot id recording what the run did.
    pub recorded_snapshot: Option<String>,
}

impl DefaultsOutcome {
    pub fn succeeded(&self) -> usize {
        self.results
            .iter()
            .filter(|result| result.outcome.is_success())
            .count()
    }

    /// Whether any entry did not do what it set out to do. A partial failure is
    /// still a failure for those entries, and the successful ones stay
    /// successful.
    pub fn has_failures(&self) -> bool {
        self.results.iter().any(|result| {
            matches!(
                result.outcome,
                EntryOutcome::Failed { .. }
                    | EntryOutcome::NotVerified { .. }
                    | EntryOutcome::VerificationInconclusive { .. }
            )
        })
    }
}
