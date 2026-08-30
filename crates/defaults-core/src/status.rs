//! Per-integration status and the aggregate a component shows.
//!
//! The aggregate never hides a partial state. It is derived from the individual
//! results by a fixed, documented precedence, and every individual result stays
//! available underneath it.

use better_core::defaults::{
    DefaultsValue, HealthPrerequisite, IntegrationId, IntegrationKind, ObservedValue, SessionEffect,
};
use better_core::manifest::ComponentId;
use serde::{Deserialize, Serialize};

/// What Better Manager knows about a component's readiness. It comes from the
/// manager's own lifecycle state; this crate does not probe for it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComponentReadiness {
    pub installed: bool,
    pub enabled: bool,
    pub healthy: bool,
}

impl ComponentReadiness {
    /// A component that is installed, enabled, and healthy.
    pub fn ready() -> Self {
        Self {
            installed: true,
            enabled: true,
            healthy: true,
        }
    }

    /// The first declared prerequisite this component does not meet.
    pub fn first_unmet(&self, prerequisites: &[HealthPrerequisite]) -> Option<HealthPrerequisite> {
        prerequisites
            .iter()
            .copied()
            .find(|prerequisite| match prerequisite {
                HealthPrerequisite::Installed => !self.installed,
                HealthPrerequisite::Enabled => !self.enabled,
                HealthPrerequisite::Healthy => !self.healthy,
            })
    }
}

/// The running system, as far as a declaration is concerned.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SystemContext {
    pub distribution: String,
    pub desktop_session: String,
}

impl SystemContext {
    pub fn new(distribution: impl Into<String>, desktop_session: impl Into<String>) -> Self {
        Self {
            distribution: distribution.into(),
            desktop_session: desktop_session.into(),
        }
    }
}

/// The state of one declared integration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum IntegrationState {
    /// The setting points at this component.
    Default,
    /// The setting is readable and points somewhere else.
    NotDefault,
    /// The setting changed after Better Manager last wrote or verified it.
    ChangedExternally { last_known: Option<DefaultsValue> },
    /// The component or the system cannot take part in this integration.
    Unavailable { reason: String },
    /// Another installed component holds this exclusive integration.
    Conflict { claimant: ComponentId },
    /// The effective value cannot be determined safely.
    Unknown { reason: String },
    /// Better Manager wrote the value and the declaration says it does not take
    /// effect until the session ends.
    NeedsSignOut,
}

/// One integration, with everything the detail view has to show.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub integration: IntegrationId,
    pub kind: IntegrationKind,
    pub state: IntegrationState,
    /// What the system says right now.
    pub current: ObservedValue,
    /// What Better OS wants it to say.
    pub desired: DefaultsValue,
    pub session_effect: SessionEffect,
    /// Whether a definite previous value was captured and can be written back.
    pub restore_available: bool,
    /// What a verifying read last saw, when Better Manager has ever verified.
    pub last_verified_value: Option<DefaultsValue>,
}

/// The eight states a component shows above its integrations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "aggregate", rename_all = "snake_case")]
pub enum AggregateState {
    Default,
    NotDefault,
    PartiallyDefault,
    ChangedExternally,
    Unavailable { reason: String },
    Conflict { claimant: ComponentId },
    Unknown { reason: String },
    NeedsSignOut,
}

impl AggregateState {
    /// Derives the component-level state from its integrations.
    ///
    /// The precedence is fixed and is about what stops the user acting, most
    /// specific first: something is out of reach, something else owns it,
    /// somebody changed it, nobody can tell what it is, it will not take effect
    /// until sign-out. Only when none of those apply does the answer come from
    /// counting how many integrations point at the component, which is where
    /// `Partially default` comes from.
    pub fn derive(statuses: &[IntegrationStatus]) -> Self {
        if statuses.is_empty() {
            return Self::Unavailable {
                reason: "defaults.no_declared_integrations".to_string(),
            };
        }
        for status in statuses {
            if let IntegrationState::Unavailable { reason } = &status.state {
                return Self::Unavailable {
                    reason: reason.clone(),
                };
            }
        }
        for status in statuses {
            if let IntegrationState::Conflict { claimant } = &status.state {
                return Self::Conflict {
                    claimant: claimant.clone(),
                };
            }
        }
        if statuses
            .iter()
            .any(|status| matches!(status.state, IntegrationState::ChangedExternally { .. }))
        {
            return Self::ChangedExternally;
        }
        for status in statuses {
            if let IntegrationState::Unknown { reason } = &status.state {
                return Self::Unknown {
                    reason: reason.clone(),
                };
            }
        }
        if statuses
            .iter()
            .any(|status| status.state == IntegrationState::NeedsSignOut)
        {
            return Self::NeedsSignOut;
        }
        let defaults = statuses
            .iter()
            .filter(|status| status.state == IntegrationState::Default)
            .count();
        if defaults == statuses.len() {
            Self::Default
        } else if defaults == 0 {
            Self::NotDefault
        } else {
            Self::PartiallyDefault
        }
    }
}

/// One component's defaults, aggregate and detail together.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComponentDefaults {
    pub component: ComponentId,
    pub aggregate: AggregateState,
    pub integrations: Vec<IntegrationStatus>,
}

/// Everything the Defaults view shows, and what the CLI prints.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DefaultsReport {
    pub components: Vec<ComponentDefaults>,
    /// Snapshots on disk that could not be read. Reported here so a caller
    /// cannot act on a history it does not know is incomplete.
    pub damaged_snapshots: Vec<String>,
}

impl DefaultsReport {
    pub fn component(&self, component: &ComponentId) -> Option<&ComponentDefaults> {
        self.components
            .iter()
            .find(|entry| &entry.component == component)
    }
}
