//! What the touchpad said before Better OS touched it.
//!
//! The backup is captured once, before the first mutation, and never
//! overwritten by a later apply. That is the whole reason restore can promise
//! to return to the previous state: a second capture taken after a change would
//! record Better OS's own value as the thing to go back to.
//!
//! A capture records a reading per setting, including the readings that are not
//! values. "Nothing was set here" has to survive into the backup, because
//! restoring it means removing the entry rather than writing a number.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::settings::{Reading, SettingId};

pub const BACKUP_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub schema_version: u32,
    /// Seconds since the Unix epoch, supplied by the caller so this crate needs
    /// no clock and a test can pin it.
    pub captured_at: u64,
    /// Which backend read these values. Restoring a GNOME capture through a
    /// different backend is not obviously safe, and the reader can only notice
    /// that if the capture says where it came from.
    pub backend: String,
    /// The stable identity of the device the capture is about, when the values
    /// were per-device rather than global.
    pub device: Option<String>,
    entries: BTreeMap<SettingId, Reading>,
}

impl Backup {
    pub fn capture(
        backend: impl Into<String>,
        device: Option<String>,
        readings: Vec<(SettingId, Reading)>,
        captured_at: u64,
    ) -> Self {
        Self {
            schema_version: BACKUP_SCHEMA_VERSION,
            captured_at,
            backend: backend.into(),
            device,
            entries: readings.into_iter().collect(),
        }
    }

    pub fn reading(&self, setting: SettingId) -> Option<&Reading> {
        self.entries.get(&setting)
    }

    pub fn readings(&self) -> impl Iterator<Item = (SettingId, &Reading)> {
        self.entries
            .iter()
            .map(|(setting, reading)| (*setting, reading))
    }

    pub fn covers(&self, setting: SettingId) -> bool {
        self.entries.contains_key(&setting)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Adds readings for settings this capture does not already cover.
    ///
    /// A setting already in the backup is left exactly as it was. That is what
    /// makes this safe to call before every apply: the first capture of a
    /// setting is the only one that ever counts.
    pub fn extend_untouched(&mut self, readings: Vec<(SettingId, Reading)>) -> Vec<SettingId> {
        let mut added = Vec::new();
        for (setting, reading) in readings {
            if let std::collections::btree_map::Entry::Vacant(slot) = self.entries.entry(setting) {
                slot.insert(reading);
                added.push(setting);
            }
        }
        added
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingValue;

    #[test]
    fn a_capture_keeps_readings_that_are_not_values() {
        let backup = Backup::capture(
            "gnome",
            Some("usb:1234:5678:SynPS/2".to_string()),
            vec![
                (
                    SettingId::PointerSensitivity,
                    Reading::session_default("gnome.no_user_scope_value"),
                ),
                (
                    SettingId::TapToClick,
                    Reading::value(SettingValue::toggle(false)),
                ),
            ],
            1_700_000_000,
        );
        assert_eq!(backup.schema_version, BACKUP_SCHEMA_VERSION);
        assert!(matches!(
            backup.reading(SettingId::PointerSensitivity),
            Some(Reading::SessionDefault { .. })
        ));
        assert!(!backup.covers(SettingId::DragLock));
        assert_eq!(backup.device.as_deref(), Some("usb:1234:5678:SynPS/2"));
    }

    #[test]
    fn a_second_capture_never_replaces_the_first_reading_of_a_setting() {
        let mut backup = Backup::capture(
            "gnome",
            None,
            vec![(
                SettingId::TapToClick,
                Reading::value(SettingValue::toggle(false)),
            )],
            0,
        );
        let added = backup.extend_untouched(vec![
            // Better OS has since written `true`. The backup must keep `false`.
            (
                SettingId::TapToClick,
                Reading::value(SettingValue::toggle(true)),
            ),
            (
                SettingId::DragLock,
                Reading::value(SettingValue::toggle(true)),
            ),
        ]);

        assert_eq!(added, vec![SettingId::DragLock]);
        assert_eq!(
            backup.reading(SettingId::TapToClick),
            Some(&Reading::value(SettingValue::toggle(false)))
        );
    }

    #[test]
    fn a_capture_round_trips_through_json() {
        let backup = Backup::capture(
            "gnome",
            None,
            vec![(
                SettingId::ClickMethod,
                Reading::value(SettingValue::click(crate::value::ClickMethod::Fingers)),
            )],
            42,
        );
        let text = serde_json::to_string(&backup).unwrap();
        assert_eq!(serde_json::from_str::<Backup>(&text).unwrap(), backup);
    }
}
