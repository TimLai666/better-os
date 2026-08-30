//! Per-device removal preferences, and the plan that undoes them.
//!
//! A preference is only ever a deviation from Direct Removal, so an empty set
//! and a set full of Direct Removal records mean the same thing and the file
//! stays small. Restoring defaults is expressed as a plan rather than performed
//! silently, because uninstalling the component has to be able to say what it
//! is about to change.

use crate::identity::{DeviceIdentity, IdentityKey};
use crate::policy::{PerformanceOptIn, PolicyError, RemovalPolicy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// The on-disk schema. A newer file is preserved rather than reset, the same
/// rule `manager-store` follows.
pub const PREFERENCES_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceRecord {
    pub policy: RemovalPolicy,
    /// The risks the user was shown when they chose this. An opt-in that no
    /// longer covers every required risk stops being honored.
    #[serde(default)]
    pub opt_in: PerformanceOptIn,
    /// The name the device had when the preference was recorded. Presentation
    /// only; never part of matching.
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceSet {
    pub schema_version: u32,
    #[serde(default)]
    records: BTreeMap<IdentityKey, PreferenceRecord>,
}

impl Default for PreferenceSet {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            records: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum PreferenceError {
    #[error("storage preference schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("storage preferences are malformed: {0}")]
    Malformed(String),
}

/// One device's preference returning to the default.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RestoreEntry {
    pub identity: IdentityKey,
    pub from: RemovalPolicy,
    pub to: RemovalPolicy,
    pub label: Option<String>,
}

/// What restoring defaults would change. An empty plan means the system is
/// already at its default behavior, which is what an uninstall check asserts.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RestoreDefaultPlan {
    pub entries: Vec<RestoreEntry>,
}

impl RestoreDefaultPlan {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl PreferenceSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses a stored document, checking the schema before the body so a file
    /// written by a newer version is refused rather than misread.
    pub fn from_json(document: &str) -> Result<Self, PreferenceError> {
        let value: serde_json::Value = serde_json::from_str(document)
            .map_err(|error| PreferenceError::Malformed(error.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| PreferenceError::Malformed("schema_version is missing".to_string()))?;
        let version = u32::try_from(version)
            .map_err(|_| PreferenceError::Malformed("schema_version exceeds u32".to_string()))?;
        if version > PREFERENCES_SCHEMA_VERSION {
            return Err(PreferenceError::UnsupportedSchema(version));
        }
        if version != PREFERENCES_SCHEMA_VERSION {
            return Err(PreferenceError::UnsupportedSchema(version));
        }
        serde_json::from_value(value).map_err(|error| PreferenceError::Malformed(error.to_string()))
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn record(&self, key: &IdentityKey) -> Option<&PreferenceRecord> {
        self.records.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&IdentityKey, &PreferenceRecord)> {
        self.records.iter()
    }

    /// The policy to start a device in.
    ///
    /// Every path that is not an intact, still-valid Performance opt-in under a
    /// persistable identity ends at Direct Removal. That is the default the
    /// issue requires for a device nobody has ever seen, and it is also the
    /// safe answer for a record that can no longer be trusted.
    pub fn policy_for(&self, identity: &DeviceIdentity) -> RemovalPolicy {
        if !identity.confidence().persistable() {
            return RemovalPolicy::DirectRemoval;
        }
        match self.records.get(identity.key()) {
            Some(record)
                if record.policy == RemovalPolicy::Performance
                    && record.opt_in.covers_required_risks() =>
            {
                RemovalPolicy::Performance
            }
            _ => RemovalPolicy::DirectRemoval,
        }
    }

    /// Switches a device to Performance mode.
    ///
    /// Refused without an opt-in that covers every required risk, and refused
    /// for a device that can only be named by its current kernel path — writing
    /// that record would mean applying it later to whatever holds that path.
    pub fn set_performance(
        &mut self,
        identity: &DeviceIdentity,
        opt_in: PerformanceOptIn,
    ) -> Result<(), PolicyError> {
        if !identity.confidence().persistable() {
            return Err(PolicyError::IdentityNotPersistable);
        }
        if !opt_in.covers_required_risks() {
            return Err(PolicyError::RiskNotAcknowledged {
                missing: opt_in
                    .missing_risks()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            });
        }
        self.records.insert(
            identity.key().clone(),
            PreferenceRecord {
                policy: RemovalPolicy::Performance,
                opt_in,
                label: Some(identity.display_name()),
            },
        );
        Ok(())
    }

    /// Returns a device to the default. Direct Removal is the absence of a
    /// record, not a record saying so.
    pub fn set_direct_removal(&mut self, identity: &DeviceIdentity) {
        self.records.remove(identity.key());
    }

    /// What returning every device to the default would change, without
    /// changing it.
    pub fn planned_restore_defaults(&self) -> RestoreDefaultPlan {
        RestoreDefaultPlan {
            entries: self
                .records
                .iter()
                .filter(|(_, record)| record.policy != RemovalPolicy::DirectRemoval)
                .map(|(key, record)| RestoreEntry {
                    identity: key.clone(),
                    from: record.policy,
                    to: RemovalPolicy::DirectRemoval,
                    label: record.label.clone(),
                })
                .collect(),
        }
    }

    /// Carries out the restore and returns what it changed. Used by uninstall
    /// and by an explicit "reset storage preferences" action.
    pub fn restore_defaults(&mut self) -> RestoreDefaultPlan {
        let plan = self.planned_restore_defaults();
        self.records.clear();
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{IdentityEvidence, Transport};

    fn identity(uuid: Option<&str>) -> DeviceIdentity {
        DeviceIdentity::from_evidence(IdentityEvidence {
            filesystem_uuid: uuid.map(str::to_string),
            device_path: "/dev/sdb1".to_string(),
            transport: Transport::Usb,
            label: Some("FIELD DATA".to_string()),
            ..IdentityEvidence::default()
        })
    }

    #[test]
    fn a_device_nobody_has_configured_gets_direct_removal() {
        let preferences = PreferenceSet::new();
        assert_eq!(
            preferences.policy_for(&identity(Some("A1B2-C3D4"))),
            RemovalPolicy::DirectRemoval
        );
        assert!(preferences.is_empty());
    }

    #[test]
    fn a_performance_preference_survives_a_reconnect_under_a_new_kernel_path() {
        let mut preferences = PreferenceSet::new();
        let device = identity(Some("A1B2-C3D4"));
        preferences
            .set_performance(&device, PerformanceOptIn::acknowledging_all_risks())
            .unwrap();

        let reconnected = DeviceIdentity::from_evidence(IdentityEvidence {
            filesystem_uuid: Some("A1B2-C3D4".to_string()),
            device_path: "/dev/sdg1".to_string(),
            transport: Transport::Usb,
            ..IdentityEvidence::default()
        });
        assert_eq!(
            preferences.policy_for(&reconnected),
            RemovalPolicy::Performance
        );
    }

    #[test]
    fn performance_mode_is_refused_without_an_acknowledgement_of_every_risk() {
        let mut preferences = PreferenceSet::new();
        let error = preferences
            .set_performance(
                &identity(Some("A1B2-C3D4")),
                PerformanceOptIn::acknowledging(["storage.performance.eject_required"]),
            )
            .expect_err("a partial acknowledgement is not consent");
        assert!(matches!(error, PolicyError::RiskNotAcknowledged { .. }));
        assert!(preferences.is_empty());
    }

    #[test]
    fn a_device_known_only_by_its_current_path_can_never_hold_a_preference() {
        let mut preferences = PreferenceSet::new();
        let volatile = identity(None);
        assert!(matches!(
            preferences.set_performance(&volatile, PerformanceOptIn::acknowledging_all_risks()),
            Err(PolicyError::IdentityNotPersistable)
        ));
        assert_eq!(
            preferences.policy_for(&volatile),
            RemovalPolicy::DirectRemoval
        );
    }

    #[test]
    fn a_stored_opt_in_stops_being_honored_when_a_new_risk_is_added() {
        // Simulates a file written before a risk key existed.
        let device = identity(Some("A1B2-C3D4"));
        let mut preferences = PreferenceSet::new();
        preferences.records.insert(
            device.key().clone(),
            PreferenceRecord {
                policy: RemovalPolicy::Performance,
                opt_in: PerformanceOptIn::acknowledging(["storage.performance.eject_required"]),
                label: None,
            },
        );
        assert_eq!(
            preferences.policy_for(&device),
            RemovalPolicy::DirectRemoval
        );
    }

    #[test]
    fn restoring_defaults_reports_exactly_what_it_changed() {
        let mut preferences = PreferenceSet::new();
        let device = identity(Some("A1B2-C3D4"));
        preferences
            .set_performance(&device, PerformanceOptIn::acknowledging_all_risks())
            .unwrap();

        let planned = preferences.planned_restore_defaults();
        assert_eq!(planned.entries.len(), 1);
        assert_eq!(planned.entries[0].from, RemovalPolicy::Performance);
        assert_eq!(planned.entries[0].to, RemovalPolicy::DirectRemoval);

        let applied = preferences.restore_defaults();
        assert_eq!(applied, planned);
        assert!(preferences.planned_restore_defaults().is_empty());
        assert_eq!(
            preferences.policy_for(&device),
            RemovalPolicy::DirectRemoval
        );
    }

    #[test]
    fn a_file_from_a_newer_version_is_refused_rather_than_misread() {
        let document = r#"{"schema_version":99,"records":{}}"#;
        assert!(matches!(
            PreferenceSet::from_json(document),
            Err(PreferenceError::UnsupportedSchema(99))
        ));
    }

    #[test]
    fn preferences_round_trip_through_their_stored_form() {
        let mut preferences = PreferenceSet::new();
        preferences
            .set_performance(
                &identity(Some("A1B2-C3D4")),
                PerformanceOptIn::acknowledging_all_risks(),
            )
            .unwrap();
        let document = preferences.to_json().unwrap();
        assert_eq!(PreferenceSet::from_json(&document).unwrap(), preferences);
    }
}
