//! What the machine is, as opposed to what it is doing.
//!
//! An inventory is a flat, versioned, sorted set of key/value facts. Flat
//! because the only two operations that matter are "show it" and "say what
//! changed since last time", and both are simpler over a map than over a
//! nested document. Versioned because a diff between two records written by
//! different Better Monitor versions has to be able to say so.
//!
//! Every entry carries how sensitive it is. That is what an export reads to
//! decide between publishing a value, pseudonymizing it, and withholding it —
//! the classification is made once here, where the fact is collected, instead
//! of being re-derived from a key name at export time.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The record schema. Bumping it is what makes an old inventory migratable
/// instead of unreadable.
pub const INVENTORY_SCHEMA_VERSION: u32 = 1;

/// How an entry may leave the machine.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Safe to export as it stands: a kernel version, a CPU model.
    Public,
    /// Identifies the person: a username, a home directory, a hostname.
    Personal,
    /// Identifies the machine on a network: a MAC address, an IP address, a
    /// disk serial.
    Identifier,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InventoryEntry {
    pub value: String,
    pub sensitivity: Sensitivity,
}

impl InventoryEntry {
    pub fn public(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            sensitivity: Sensitivity::Public,
        }
    }

    pub fn personal(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            sensitivity: Sensitivity::Personal,
        }
    }

    pub fn identifier(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            sensitivity: Sensitivity::Identifier,
        }
    }
}

/// One captured inventory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub schema_version: u32,
    pub captured_at_unix_ms: u64,
    pub entries: BTreeMap<String, InventoryEntry>,
}

impl Inventory {
    pub fn new(captured_at_unix_ms: u64) -> Self {
        Self {
            schema_version: INVENTORY_SCHEMA_VERSION,
            captured_at_unix_ms,
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, key: impl Into<String>, entry: InventoryEntry) -> &mut Self {
        self.entries.insert(key.into(), entry);
        self
    }

    pub fn get(&self, key: &str) -> Option<&InventoryEntry> {
        self.entries.get(key)
    }

    /// Every value at or above a sensitivity level. An export builds its
    /// redaction vocabulary from this rather than from guesswork.
    pub fn values_at_least(&self, sensitivity: Sensitivity) -> Vec<&str> {
        self.entries
            .values()
            .filter(|entry| entry.sensitivity >= sensitivity)
            .map(|entry| entry.value.as_str())
            .filter(|value| !value.is_empty())
            .collect()
    }

    /// Whether two captures describe a different machine state. The capture
    /// time is deliberately excluded: a record is only worth writing when
    /// something actually changed.
    pub fn differs_from(&self, other: &Inventory) -> bool {
        self.schema_version != other.schema_version || self.entries != other.entries
    }
}

/// One key that is not the same in both records.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InventoryChange {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub sensitivity: Sensitivity,
}

/// What changed between two inventories.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct InventoryDiff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_captured_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_captured_at_unix_ms: Option<u64>,
    pub added: Vec<InventoryChange>,
    pub removed: Vec<InventoryChange>,
    pub changed: Vec<InventoryChange>,
}

impl InventoryDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    pub fn len(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len()
    }
}

/// Compare two inventories key by key.
pub fn diff(before: &Inventory, after: &Inventory) -> InventoryDiff {
    let keys: BTreeSet<&String> = before.entries.keys().chain(after.entries.keys()).collect();
    let mut result = InventoryDiff {
        before_captured_at_unix_ms: Some(before.captured_at_unix_ms),
        after_captured_at_unix_ms: Some(after.captured_at_unix_ms),
        ..InventoryDiff::default()
    };
    for key in keys {
        match (before.entries.get(key), after.entries.get(key)) {
            (None, Some(entry)) => result.added.push(InventoryChange {
                key: key.clone(),
                before: None,
                after: Some(entry.value.clone()),
                sensitivity: entry.sensitivity,
            }),
            (Some(entry), None) => result.removed.push(InventoryChange {
                key: key.clone(),
                before: Some(entry.value.clone()),
                after: None,
                sensitivity: entry.sensitivity,
            }),
            (Some(old), Some(new)) if old.value != new.value => {
                result.changed.push(InventoryChange {
                    key: key.clone(),
                    before: Some(old.value.clone()),
                    after: Some(new.value.clone()),
                    // The stricter of the two, so a fact that became personal
                    // is not exported under the old classification.
                    sensitivity: old.sensitivity.max(new.sensitivity),
                });
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> Inventory {
        let mut inventory = Inventory::new(1_000);
        inventory
            .insert("os.name", InventoryEntry::public("Zorin OS 18"))
            .insert(
                "kernel.release",
                InventoryEntry::public("6.11.0-19-generic"),
            )
            .insert("host.name", InventoryEntry::personal("workshop"))
            .insert(
                "network.eth0.mac",
                InventoryEntry::identifier("aa:bb:cc:dd:ee:ff"),
            );
        inventory
    }

    #[test]
    fn an_unchanged_machine_produces_an_empty_diff() {
        let mut later = baseline();
        later.captured_at_unix_ms = 9_000;
        let changes = diff(&baseline(), &later);
        assert!(changes.is_empty());
        assert_eq!(changes.len(), 0);
        assert!(!later.differs_from(&baseline()));
    }

    #[test]
    fn a_kernel_upgrade_shows_up_as_a_change_with_both_values() {
        let mut later = baseline();
        later.entries.insert(
            "kernel.release".into(),
            InventoryEntry::public("6.14.0-2-generic"),
        );
        let changes = diff(&baseline(), &later);
        assert_eq!(changes.changed.len(), 1);
        assert_eq!(changes.changed[0].key, "kernel.release");
        assert_eq!(
            changes.changed[0].before.as_deref(),
            Some("6.11.0-19-generic")
        );
        assert_eq!(
            changes.changed[0].after.as_deref(),
            Some("6.14.0-2-generic")
        );
        assert!(later.differs_from(&baseline()));
    }

    #[test]
    fn an_added_and_a_removed_device_are_not_reported_as_a_change() {
        let mut later = baseline();
        later.entries.remove("network.eth0.mac");
        later.entries.insert(
            "network.wlan0.mac".into(),
            InventoryEntry::identifier("11:22:33:44:55:66"),
        );
        let changes = diff(&baseline(), &later);
        assert_eq!(changes.added.len(), 1);
        assert_eq!(changes.removed.len(), 1);
        assert!(changes.changed.is_empty());
        assert_eq!(changes.added[0].key, "network.wlan0.mac");
        assert_eq!(changes.removed[0].key, "network.eth0.mac");
    }

    #[test]
    fn a_change_keeps_the_stricter_of_the_two_classifications() {
        let mut before = Inventory::new(1);
        before.insert("session.user", InventoryEntry::public("guest"));
        let mut after = Inventory::new(2);
        after.insert("session.user", InventoryEntry::personal("tim"));
        let changes = diff(&before, &after);
        assert_eq!(changes.changed[0].sensitivity, Sensitivity::Personal);
    }

    #[test]
    fn the_redaction_vocabulary_is_everything_not_public() {
        let inventory = baseline();
        let values = inventory.values_at_least(Sensitivity::Personal);
        assert!(values.contains(&"workshop"));
        assert!(values.contains(&"aa:bb:cc:dd:ee:ff"));
        assert!(!values.contains(&"Zorin OS 18"));
    }

    #[test]
    fn an_inventory_round_trips_through_json() {
        let inventory = baseline();
        let encoded = serde_json::to_string(&inventory).unwrap();
        let decoded: Inventory = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, inventory);
        assert_eq!(decoded.schema_version, INVENTORY_SCHEMA_VERSION);
    }

    #[test]
    fn a_diff_round_trips_through_json() {
        let mut later = baseline();
        later
            .entries
            .insert("os.name".into(), InventoryEntry::public("Zorin OS 19"));
        let changes = diff(&baseline(), &later);
        let encoded = serde_json::to_string(&changes).unwrap();
        assert_eq!(
            serde_json::from_str::<InventoryDiff>(&encoded).unwrap(),
            changes
        );
    }
}
