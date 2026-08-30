//! The two removal policies, and the one-way door between them.
//!
//! Direct Removal is the default for every supported device, including one this
//! host has never seen. Performance mode is only ever reached through an
//! explicit opt-in that carries an acknowledgement of what it costs, so no code
//! path — not a heuristic, not a migration, not a "the device looks fast" rule —
//! can turn it on by itself.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Machine keys for the risks a user must be shown before Performance mode is
/// switched on. Presentation layers own the wording; this crate owns the list,
/// so a surface cannot quietly ship an opt-in dialog that omits one.
pub const PERFORMANCE_RISK_KEYS: &[&str] = &[
    "storage.performance.eject_required",
    "storage.performance.data_loss_on_direct_removal",
    "storage.performance.throughput_tradeoff",
];

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RemovalPolicy {
    /// Short-lived writeback, prompt flushing after file operations, and a
    /// readiness claim that is allowed to be made. The default, always.
    #[default]
    DirectRemoval,
    /// More buffered write throughput, and no readiness claim. Eject is
    /// required before unplugging.
    Performance,
}

impl RemovalPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            RemovalPolicy::DirectRemoval => "direct_removal",
            RemovalPolicy::Performance => "performance",
        }
    }

    /// Whether this policy requires Eject before physical removal.
    pub fn requires_eject(self) -> bool {
        matches!(self, RemovalPolicy::Performance)
    }
}

/// Evidence that a user was shown the trade-off and accepted it.
///
/// The acknowledged keys are carried rather than a bare boolean so that adding
/// a risk to [`PERFORMANCE_RISK_KEYS`] invalidates opt-ins recorded before that
/// risk existed, instead of silently inheriting consent nobody gave.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PerformanceOptIn {
    acknowledged_keys: Vec<String>,
}

impl PerformanceOptIn {
    /// Records an opt-in against exactly the risks that were shown.
    pub fn acknowledging(keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut acknowledged_keys: Vec<String> = keys.into_iter().map(Into::into).collect();
        acknowledged_keys.sort();
        acknowledged_keys.dedup();
        Self { acknowledged_keys }
    }

    /// The opt-in a surface produces after showing every required risk.
    pub fn acknowledging_all_risks() -> Self {
        Self::acknowledging(PERFORMANCE_RISK_KEYS.iter().copied())
    }

    pub fn acknowledged_keys(&self) -> impl Iterator<Item = &str> {
        self.acknowledged_keys.iter().map(String::as_str)
    }

    /// Which required risks this opt-in does not cover.
    pub fn missing_risks(&self) -> Vec<&'static str> {
        PERFORMANCE_RISK_KEYS
            .iter()
            .copied()
            .filter(|key| !self.acknowledged_keys.iter().any(|held| held == key))
            .collect()
    }

    pub fn covers_required_risks(&self) -> bool {
        self.missing_risks().is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum PolicyError {
    #[error("performance mode needs an acknowledgement of: {}", .missing.join(", "))]
    RiskNotAcknowledged { missing: Vec<String> },
    #[error("a preference cannot be stored for a device identified only by its current path")]
    IdentityNotPersistable,
    #[error("two connected devices report the same identity, so no preference is applied")]
    AmbiguousIdentity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_removal_is_what_a_device_gets_when_nothing_was_chosen() {
        assert_eq!(RemovalPolicy::default(), RemovalPolicy::DirectRemoval);
        assert!(!RemovalPolicy::DirectRemoval.requires_eject());
        assert!(RemovalPolicy::Performance.requires_eject());
    }

    #[test]
    fn an_opt_in_that_skipped_a_risk_does_not_count_as_consent() {
        let partial = PerformanceOptIn::acknowledging(["storage.performance.eject_required"]);
        assert!(!partial.covers_required_risks());
        assert_eq!(
            partial.missing_risks().len(),
            PERFORMANCE_RISK_KEYS.len() - 1
        );

        assert!(PerformanceOptIn::acknowledging_all_risks().covers_required_risks());
    }

    #[test]
    fn an_empty_opt_in_is_never_consent() {
        assert!(!PerformanceOptIn::default().covers_required_risks());
    }
}
