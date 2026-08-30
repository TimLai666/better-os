//! The typed adapters that read and change desktop defaults.
//!
//! Everything above this crate — planning, aggregate status, the CLI, and
//! later the GUI — works in typed values. This is the only layer that knows
//! where a default actually lives, and it still never builds a shell string:
//! there is no `gsettings`, no `xdg-mime`, and no command anywhere in it.
//!
//! Three rules hold throughout:
//!
//! - An adapter reports what it saw, including that it saw nothing usable.
//!   [`better_core::ObservedValue`] keeps "holds this value", "holds nothing",
//!   "cannot be determined", "not supported here", and "not allowed to look"
//!   apart, because only one of those can be safely overwritten.
//! - An adapter that cannot change something says so. [`WriteOutcome::
//!   ManualActionRequired`] carries a stable machine key, and no adapter ever
//!   returns success for work it did not do. An integration kind with no
//!   production adapter reaches the same outcome by having no adapter at all.
//! - Verification is a second read, never an assumption. [`DefaultsAdapter::
//!   verify`] re-reads through the declared verify adapter and compares.

pub mod dconf;
pub mod gvdb;
pub mod mock;
pub mod xdg;

use std::collections::BTreeMap;

use better_core::defaults::{AdapterId, DefaultIntegration, DefaultsValue, ObservedValue};
use better_core::manifest::ComponentId;
use serde::{Deserialize, Serialize};

pub use dconf::DconfAdapter;
pub use mock::{InMemoryAdapter, MockBehavior, MockDesktop};
pub use xdg::XdgDefaultAppAdapter;

/// What an adapter is being asked about.
#[derive(Clone, Copy, Debug)]
pub struct AdapterRequest<'a> {
    pub component: &'a ComponentId,
    pub integration: &'a DefaultIntegration,
}

impl<'a> AdapterRequest<'a> {
    pub fn new(component: &'a ComponentId, integration: &'a DefaultIntegration) -> Self {
        Self {
            component,
            integration,
        }
    }

    pub fn keys(&self) -> &'a [String] {
        &self.integration.target.keys
    }

    pub fn desired(&self) -> &'a DefaultsValue {
        &self.integration.target.desired
    }
}

/// What a write asks for. Clearing is its own case: restoring a setting that
/// held nothing means removing it, not writing an empty string.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "write", rename_all = "snake_case")]
pub enum WriteValue {
    Set { value: DefaultsValue },
    Clear,
}

impl WriteValue {
    /// The write that reproduces an earlier observation, or nothing when that
    /// observation was never definite enough to reproduce.
    pub fn from_observation(observed: &ObservedValue) -> Option<Self> {
        match observed {
            ObservedValue::Set { value } => Some(Self::Set {
                value: value.clone(),
            }),
            ObservedValue::Unset => Some(Self::Clear),
            _ => None,
        }
    }
}

/// The result of asking an adapter to change something.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum WriteOutcome {
    /// The adapter changed the setting. Whether it took is decided by a
    /// verifying read, not by this value.
    Written,
    /// Nothing was written because the setting already said this.
    AlreadyCorrect,
    /// The adapter cannot make this change and did nothing. `reason` is a
    /// stable machine key and `detail` is diagnostic context.
    ManualActionRequired {
        reason: String,
        detail: Option<String>,
    },
    /// The adapter tried and failed.
    Failed {
        reason: String,
        detail: Option<String>,
    },
}

impl WriteOutcome {
    pub fn manual(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::ManualActionRequired {
            reason: reason.into(),
            detail: Some(detail.into()),
        }
    }

    pub fn failed(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
            detail: Some(detail.into()),
        }
    }
}

/// The result of reading a setting back and comparing it with what was wanted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verified", rename_all = "snake_case")]
pub enum VerifyOutcome {
    Matches {
        observed: ObservedValue,
    },
    Differs {
        observed: ObservedValue,
    },
    /// The read was not definite, so neither agreement nor disagreement can be
    /// claimed.
    Indeterminate {
        observed: ObservedValue,
    },
}

/// Reads and changes one class of desktop setting.
pub trait DefaultsAdapter {
    fn id(&self) -> AdapterId;

    /// The effective value the system reports for this integration.
    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue;

    /// Writes a value. Implementations must never report success for work they
    /// did not do.
    fn write(&mut self, request: &AdapterRequest<'_>, value: &WriteValue) -> WriteOutcome;

    /// Makes the declared desired value the current one.
    fn apply(&mut self, request: &AdapterRequest<'_>) -> WriteOutcome {
        self.write(
            request,
            &WriteValue::Set {
                value: request.desired().clone(),
            },
        )
    }

    /// Puts back a value captured earlier. A capture that was never definite
    /// cannot be reproduced, and saying so is the only honest answer.
    fn restore(&mut self, request: &AdapterRequest<'_>, captured: &ObservedValue) -> WriteOutcome {
        match WriteValue::from_observation(captured) {
            Some(value) => self.write(request, &value),
            None => WriteOutcome::manual(
                "defaults.captured_value_is_indeterminate",
                "the captured value was never read definitely, so it cannot be written back",
            ),
        }
    }

    /// Reads the setting again and compares it with `expected`.
    fn verify(&self, request: &AdapterRequest<'_>, expected: &ObservedValue) -> VerifyOutcome {
        let observed = self.read(request);
        if !observed.is_determinate() || !expected.is_determinate() {
            return VerifyOutcome::Indeterminate { observed };
        }
        if &observed == expected {
            VerifyOutcome::Matches { observed }
        } else {
            VerifyOutcome::Differs { observed }
        }
    }
}

/// Reduces one reading per declared key to one reading for the integration.
///
/// A declaration that names several keys means the component wants all of them.
/// When they disagree the effective value genuinely cannot be stated, and
/// saying "unknown" keeps the mixed state from being silently flattened into
/// one owner and overwritten.
pub fn collapse(per_key: Vec<ObservedValue>) -> ObservedValue {
    let Some(first) = per_key.first().cloned() else {
        return ObservedValue::Unknown {
            reason: "defaults.no_keys_declared".to_string(),
        };
    };
    if per_key.iter().all(|value| value == &first) {
        return first;
    }
    if let Some(unsupported) = per_key
        .iter()
        .find(|value| matches!(value, ObservedValue::Unsupported { .. }))
    {
        return unsupported.clone();
    }
    if let Some(denied) = per_key
        .iter()
        .find(|value| matches!(value, ObservedValue::PermissionDenied { .. }))
    {
        return denied.clone();
    }
    ObservedValue::Unknown {
        reason: "defaults.declared_keys_disagree".to_string(),
    }
}

/// The adapters available to a run.
///
/// A missing entry is not an error here. It is the fact that an integration
/// kind has no production adapter, which the planner turns into manual action
/// required rather than a guessed command.
#[derive(Default)]
pub struct AdapterSet {
    adapters: BTreeMap<AdapterId, Box<dyn DefaultsAdapter>>,
}

impl AdapterSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, adapter: Box<dyn DefaultsAdapter>) -> Self {
        self.insert(adapter);
        self
    }

    pub fn insert(&mut self, adapter: Box<dyn DefaultsAdapter>) {
        self.adapters.insert(adapter.id(), adapter);
    }

    pub fn get(&self, id: AdapterId) -> Option<&dyn DefaultsAdapter> {
        self.adapters.get(&id).map(AsRef::as_ref)
    }

    pub fn get_mut(&mut self, id: AdapterId) -> Option<&mut (dyn DefaultsAdapter + 'static)> {
        self.adapters.get_mut(&id).map(AsMut::as_mut)
    }

    pub fn contains(&self, id: AdapterId) -> bool {
        self.adapters.contains_key(&id)
    }

    pub fn ids(&self) -> impl Iterator<Item = AdapterId> + '_ {
        self.adapters.keys().copied()
    }

    /// An adapter for every declared id, mutating nothing outside its own
    /// memory. This is what `--execution mock` and the tests run against, and
    /// it is the only way every integration kind is exercised before a
    /// production adapter for that kind exists.
    pub fn in_memory() -> Self {
        let mut set = Self::new();
        for id in mock::ALL_ADAPTER_IDS {
            set.insert(Box::new(InMemoryAdapter::new(id)));
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_in_memory_set_covers_every_declared_adapter() {
        let set = AdapterSet::in_memory();
        for id in mock::ALL_ADAPTER_IDS {
            assert!(set.contains(id), "no in-memory adapter for {id:?}");
        }
    }

    #[test]
    fn readings_that_agree_collapse_to_one_reading() {
        let value = ObservedValue::Set {
            value: DefaultsValue::Text("always".to_string()),
        };
        assert_eq!(collapse(vec![value.clone(), value.clone()]), value);
    }

    #[test]
    fn readings_that_disagree_collapse_to_unknown_rather_than_a_winner() {
        let collapsed = collapse(vec![
            ObservedValue::Set {
                value: DefaultsValue::Text("a".to_string()),
            },
            ObservedValue::Unset,
        ]);
        assert!(matches!(collapsed, ObservedValue::Unknown { .. }));
    }

    #[test]
    fn an_unsupported_key_wins_over_a_bare_disagreement() {
        let collapsed = collapse(vec![
            ObservedValue::Set {
                value: DefaultsValue::Text("a".to_string()),
            },
            ObservedValue::Unsupported {
                reason: "test".to_string(),
            },
        ]);
        assert!(matches!(collapsed, ObservedValue::Unsupported { .. }));
    }
}
