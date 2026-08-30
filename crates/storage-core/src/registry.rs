//! Every connected device, and the one thing a single machine cannot see:
//! another device claiming to be it.
//!
//! Handles come from the platform (a UDisks2 object path, for instance) and are
//! unique per connection. Identity keys are derived and may collide — two
//! sticks from the same bad batch with the same hardcoded serial is the classic
//! case. When they do, both devices are marked ambiguous, neither inherits the
//! stored preference, and both fall back to Direct Removal, so a collision can
//! never hand one device the other's Performance-mode opt-in.

use crate::evidence::EvidencePolicy;
use crate::identity::{DeviceIdentity, IdentityKey};
use crate::machine::{DeviceEvent, DeviceMachine, Transition};
use crate::preferences::PreferenceSet;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The platform's name for one connection. Opaque here on purpose.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceHandle(String);

impl DeviceHandle {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Default)]
pub struct DeviceRegistry {
    machines: BTreeMap<DeviceHandle, DeviceMachine>,
    by_key: BTreeMap<IdentityKey, BTreeSet<DeviceHandle>>,
    evidence_policy: EvidencePolicy,
}

impl DeviceRegistry {
    pub fn new(evidence_policy: EvidencePolicy) -> Self {
        Self {
            machines: BTreeMap::new(),
            by_key: BTreeMap::new(),
            evidence_policy,
        }
    }

    pub fn len(&self) -> usize {
        self.machines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.machines.is_empty()
    }

    pub fn get(&self, handle: &DeviceHandle) -> Option<&DeviceMachine> {
        self.machines.get(handle)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&DeviceHandle, &DeviceMachine)> {
        self.machines.iter()
    }

    /// Registers a newly detected device and returns its transition, followed
    /// by a transition for any other device its identity now collides with.
    pub fn connect(
        &mut self,
        handle: DeviceHandle,
        identity: DeviceIdentity,
        preferences: &PreferenceSet,
        at: Timestamp,
    ) -> Vec<(DeviceHandle, Transition)> {
        let key = identity.key().clone();
        let policy = preferences.policy_for(&identity);
        let machine =
            DeviceMachine::connect(identity, policy, at).with_evidence_policy(self.evidence_policy);
        self.machines.insert(handle.clone(), machine);
        self.by_key.entry(key.clone()).or_default().insert(handle);
        self.reconcile_ambiguity(&key, at)
    }

    /// Removes a device and returns its disconnect transition, plus any device
    /// that stops being ambiguous now that this one is gone.
    pub fn disconnect(
        &mut self,
        handle: &DeviceHandle,
        at: Timestamp,
    ) -> Vec<(DeviceHandle, Transition)> {
        let Some(machine) = self.machines.get_mut(handle) else {
            return Vec::new();
        };
        let key = machine.identity().key().clone();
        let transition = machine.apply(DeviceEvent::Disconnected, at);
        self.machines.remove(handle);
        let mut transitions = vec![(handle.clone(), transition)];

        if let Some(handles) = self.by_key.get_mut(&key) {
            handles.remove(handle);
            if handles.is_empty() {
                self.by_key.remove(&key);
            }
        }
        transitions.extend(self.reconcile_ambiguity(&key, at));
        transitions
    }

    pub fn apply(
        &mut self,
        handle: &DeviceHandle,
        event: DeviceEvent,
        at: Timestamp,
    ) -> Option<Transition> {
        self.machines
            .get_mut(handle)
            .map(|machine| machine.apply(event, at))
    }

    /// Delivers one event to every connected device. Used for the service
    /// restart notice, which is not about any one device.
    pub fn apply_to_all(
        &mut self,
        event: DeviceEvent,
        at: Timestamp,
    ) -> Vec<(DeviceHandle, Transition)> {
        self.machines
            .iter_mut()
            .map(|(handle, machine)| (handle.clone(), machine.apply(event.clone(), at)))
            .collect()
    }

    /// Marks or clears ambiguity for every device sharing one key.
    fn reconcile_ambiguity(
        &mut self,
        key: &IdentityKey,
        at: Timestamp,
    ) -> Vec<(DeviceHandle, Transition)> {
        let handles: Vec<DeviceHandle> = self
            .by_key
            .get(key)
            .map(|handles| handles.iter().cloned().collect())
            .unwrap_or_default();
        let ambiguous = handles.len() > 1;
        handles
            .into_iter()
            .filter_map(|handle| {
                let machine = self.machines.get_mut(&handle)?;
                Some((handle, machine.set_ambiguous(ambiguous, at)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{IdentityEvidence, Transport};
    use crate::policy::{PerformanceOptIn, RemovalPolicy};
    use crate::state::DeviceStateKind;

    fn identity(serial: &str, path: &str) -> DeviceIdentity {
        DeviceIdentity::from_evidence(IdentityEvidence {
            drive_serial: Some(serial.to_string()),
            device_path: path.to_string(),
            transport: Transport::Usb,
            ..IdentityEvidence::default()
        })
    }

    #[test]
    fn two_devices_reporting_the_same_serial_do_not_share_a_preference() {
        let device = identity("BADBATCH01", "/dev/sdb1");
        let mut preferences = PreferenceSet::new();
        preferences
            .set_performance(&device, PerformanceOptIn::acknowledging_all_risks())
            .unwrap();

        let mut registry = DeviceRegistry::new(EvidencePolicy::default());
        let first = DeviceHandle::new("/org/freedesktop/UDisks2/block_devices/sdb1");
        let second = DeviceHandle::new("/org/freedesktop/UDisks2/block_devices/sdc1");

        registry.connect(
            first.clone(),
            device.clone(),
            &preferences,
            Timestamp::from_millis(1),
        );
        assert_eq!(
            registry.get(&first).unwrap().state().kind(),
            DeviceStateKind::PerformanceMode
        );

        // The clone arrives. Neither device may now be treated as the one the
        // preference was written for.
        let transitions = registry.connect(
            second.clone(),
            identity("BADBATCH01", "/dev/sdc1"),
            &preferences,
            Timestamp::from_millis(2),
        );
        assert_eq!(transitions.len(), 2);
        for handle in [&first, &second] {
            let machine = registry.get(handle).unwrap();
            assert_eq!(machine.state().kind(), DeviceStateKind::Unknown);
            assert!(!machine.state().permits_direct_removal());
        }

        // And the registry still holds two devices, not one merged record.
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn unplugging_the_clone_lets_the_original_be_itself_again() {
        let device = identity("BADBATCH01", "/dev/sdb1");
        let preferences = PreferenceSet::new();
        let mut registry = DeviceRegistry::new(EvidencePolicy::default());
        let first = DeviceHandle::new("a");
        let second = DeviceHandle::new("b");
        registry.connect(
            first.clone(),
            device.clone(),
            &preferences,
            Timestamp::START,
        );
        registry.connect(
            second.clone(),
            identity("BADBATCH01", "/dev/sdc1"),
            &preferences,
            Timestamp::START,
        );

        let transitions = registry.disconnect(&second, Timestamp::from_millis(9));
        assert_eq!(registry.len(), 1);
        assert!(transitions.iter().any(|(handle, _)| handle == &second));
        assert_ne!(
            registry.get(&first).unwrap().state().kind(),
            DeviceStateKind::Disconnected
        );
        assert_eq!(
            registry.get(&first).unwrap().policy(),
            RemovalPolicy::DirectRemoval
        );
    }

    #[test]
    fn distinct_devices_are_never_marked_ambiguous() {
        let preferences = PreferenceSet::new();
        let mut registry = DeviceRegistry::new(EvidencePolicy::default());
        registry.connect(
            DeviceHandle::new("a"),
            identity("SERIAL-A", "/dev/sdb1"),
            &preferences,
            Timestamp::START,
        );
        let transitions = registry.connect(
            DeviceHandle::new("b"),
            identity("SERIAL-B", "/dev/sdc1"),
            &preferences,
            Timestamp::START,
        );
        assert_eq!(transitions.len(), 1);
    }
}
