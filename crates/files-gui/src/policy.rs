//! Choosing a device's removal policy, and the confirmation Performance mode
//! needs first.
//!
//! Issue #5 requires the trade-off explained before Performance mode is turned
//! on, and `storage-core` enforces it: the service refuses a request that does
//! not acknowledge every key in [`PERFORMANCE_RISK_KEYS`]. This module is the
//! other half — the risks in words, and a confirmation that cannot be confirmed
//! until each one has been ticked. It has no GPUI in it; the window draws it.
//!
//! Switching back to Direct Removal asks nothing, because it gives nothing up.
//!
//! The window does not offer Performance mode at all while
//! [`OFFER_PERFORMANCE_MODE`] is off; [`offered_switch`] is where a device row
//! learns what it may offer.

use std::collections::BTreeSet;

use storage_core::{PERFORMANCE_RISK_KEYS, RemovalPolicy};

use crate::i18n::Copy;

/// Whether the window offers to switch a device to Performance mode.
///
/// Off by the project owner's decision. Performance mode changes no mount
/// option and no cache setting yet, so it makes no write faster; its only
/// effect is that a device is never called safe to unplug without Eject.
/// Turn this on once the mount-option work ticket 31 left to an ADR makes the
/// mode actually speed up writes, and reword `policy_risk_throughput`, which
/// says this version delivers no speed-up, in the same change. The
/// confirmation below stays built and tested so nothing else has to be
/// rebuilt; `a_direct_removal_device_is_not_offered_performance_mode` records
/// this decision and changes with it.
///
/// A device already in Performance mode is offered Direct Removal either way.
pub const OFFER_PERFORMANCE_MODE: bool = false;

/// The policy a device's row offers to switch to, or `None` when it offers
/// none.
pub fn offered_switch(current: RemovalPolicy) -> Option<RemovalPolicy> {
    switch_for(current, OFFER_PERFORMANCE_MODE)
}

fn switch_for(current: RemovalPolicy, offer_performance: bool) -> Option<RemovalPolicy> {
    match current {
        RemovalPolicy::DirectRemoval => offer_performance.then_some(RemovalPolicy::Performance),
        RemovalPolicy::Performance => Some(RemovalPolicy::DirectRemoval),
    }
}

/// What one risk key says, in the window's language. `None` for a key this
/// build has no words for, which a test turns into a failure.
pub fn risk_text(key: &str, c: &'static Copy) -> Option<&'static str> {
    match key {
        "storage.performance.eject_required" => Some(c.policy_risk_eject_required),
        "storage.performance.data_loss_on_direct_removal" => Some(c.policy_risk_data_loss),
        "storage.performance.throughput_tradeoff" => Some(c.policy_risk_throughput),
        _ => None,
    }
}

/// What the window sends to the storage layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRequest {
    pub object_path: String,
    pub policy: RemovalPolicy,
    /// Exactly the risks the person ticked. Empty for Direct Removal.
    pub acknowledged_risks: Vec<String>,
}

impl PolicyRequest {
    /// Direct Removal, which needs no acknowledgement.
    pub fn direct_removal(object_path: impl Into<String>) -> Self {
        Self {
            object_path: object_path.into(),
            policy: RemovalPolicy::DirectRemoval,
            acknowledged_risks: Vec::new(),
        }
    }
}

/// The open Performance mode confirmation for one device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerformanceConfirmation {
    pub object_path: String,
    /// The device name the dialog shows.
    pub label: String,
    acknowledged: BTreeSet<&'static str>,
}

impl PerformanceConfirmation {
    pub fn new(object_path: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            object_path: object_path.into(),
            label: label.into(),
            acknowledged: BTreeSet::new(),
        }
    }

    /// Every risk, in `storage-core`'s order, with whether it is ticked.
    pub fn risks(&self) -> Vec<(&'static str, bool)> {
        PERFORMANCE_RISK_KEYS
            .iter()
            .map(|key| (*key, self.acknowledged.contains(key)))
            .collect()
    }

    /// Ticks or unticks one risk. A key that is not a declared risk is ignored.
    pub fn toggle(&mut self, key: &str) {
        let Some(declared) = PERFORMANCE_RISK_KEYS
            .iter()
            .copied()
            .find(|declared| *declared == key)
        else {
            return;
        };
        if !self.acknowledged.remove(declared) {
            self.acknowledged.insert(declared);
        }
    }

    /// Whether every declared risk has been ticked.
    pub fn can_confirm(&self) -> bool {
        PERFORMANCE_RISK_KEYS
            .iter()
            .all(|key| self.acknowledged.contains(key))
    }

    /// The request, once every risk is ticked. Carries the ticked keys
    /// themselves rather than the declared list, so what is sent is what the
    /// person actually confirmed.
    pub fn request(&self) -> Option<PolicyRequest> {
        self.can_confirm().then(|| PolicyRequest {
            object_path: self.object_path.clone(),
            policy: RemovalPolicy::Performance,
            acknowledged_risks: self
                .acknowledged
                .iter()
                .map(|key| key.to_string())
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{EN_US, ZH_TW};

    #[test]
    fn every_declared_risk_has_words_in_both_languages() {
        for c in [&EN_US, &ZH_TW] {
            let mut seen = BTreeSet::new();
            for key in PERFORMANCE_RISK_KEYS {
                let text = risk_text(key, c)
                    .unwrap_or_else(|| panic!("no explanation for {key}: add one to i18n"));
                assert!(text.len() > 20, "{key} is explained, not named: {text}");
                assert!(seen.insert(text), "{key} repeats another risk's words");
            }
        }
        for key in PERFORMANCE_RISK_KEYS {
            assert_ne!(risk_text(key, &EN_US), risk_text(key, &ZH_TW), "{key}");
        }
        assert_eq!(risk_text("storage.performance.unknown", &EN_US), None);
    }

    #[test]
    fn confirm_stays_disabled_until_every_risk_is_ticked() {
        let mut confirmation = PerformanceConfirmation::new("/sdb1", "PHOTOS");
        assert_eq!(confirmation.risks().len(), PERFORMANCE_RISK_KEYS.len());
        assert!(confirmation.risks().iter().all(|(_, ticked)| !ticked));
        assert!(!confirmation.can_confirm());
        assert_eq!(confirmation.request(), None);

        for (index, key) in PERFORMANCE_RISK_KEYS.iter().enumerate() {
            assert!(!confirmation.can_confirm(), "only {index} ticked");
            confirmation.toggle(key);
        }
        assert!(confirmation.can_confirm());

        // Unticking one takes the confirmation away again.
        confirmation.toggle(PERFORMANCE_RISK_KEYS[0]);
        assert!(!confirmation.can_confirm());
        assert_eq!(confirmation.request(), None);
        confirmation.toggle(PERFORMANCE_RISK_KEYS[0]);

        // A key nobody declared cannot stand in for one that was.
        confirmation.toggle("storage.performance.unknown");
        let request = confirmation.request().expect("every risk ticked");
        assert_eq!(request.object_path, "/sdb1");
        assert_eq!(request.policy, RemovalPolicy::Performance);
        let mut sent = request.acknowledged_risks.clone();
        sent.sort();
        let mut declared: Vec<String> = PERFORMANCE_RISK_KEYS
            .iter()
            .map(|key| key.to_string())
            .collect();
        declared.sort();
        assert_eq!(sent, declared, "exactly the ticked keys, nothing else");
    }

    #[test]
    fn a_direct_removal_device_is_not_offered_performance_mode() {
        // The owner's decision while Performance mode changes no mount option:
        // turning `OFFER_PERFORMANCE_MODE` on is the one place that reverses
        // it, and this assertion goes with it.
        assert_eq!(offered_switch(RemovalPolicy::DirectRemoval), None);
    }

    #[test]
    fn a_performance_mode_device_is_always_offered_direct_removal() {
        assert_eq!(
            offered_switch(RemovalPolicy::Performance),
            Some(RemovalPolicy::DirectRemoval)
        );
        for offer_performance in [false, true] {
            assert_eq!(
                switch_for(RemovalPolicy::Performance, offer_performance),
                Some(RemovalPolicy::DirectRemoval)
            );
        }
    }

    #[test]
    fn turning_the_switch_on_offers_performance_mode_again() {
        assert_eq!(
            switch_for(RemovalPolicy::DirectRemoval, true),
            Some(RemovalPolicy::Performance)
        );
    }

    #[test]
    fn switching_back_to_direct_removal_acknowledges_nothing() {
        let request = PolicyRequest::direct_removal("/sdb1");
        assert_eq!(request.policy, RemovalPolicy::DirectRemoval);
        assert!(request.acknowledged_risks.is_empty());
    }
}
