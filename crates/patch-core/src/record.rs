//! The record of what a fix changed, kept so its undo takes back exactly
//! that.
//!
//! A record names every key a fix wrote, the value it wrote, and the value
//! that was there before. An undo puts each key back only while it still
//! holds what the fix wrote; a key someone changed afterwards is "changed by
//! another program" and stays as they left it until the person confirms
//! replacing it, the rule Better Defaults follows. Keys the fix never wrote
//! are never in a record, so an undo cannot reach them.

use serde::{Deserialize, Serialize};

use crate::entry::{Entry, EntryId, Subject, UndoKind};
use crate::flatpak::{AppId, AppName};

/// The version of the records file this build writes.
pub const RECORDS_SCHEMA_VERSION: u32 = 1;

/// One key of a Flatpak override file.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverrideKey {
    /// A socket in `[Context] sockets`.
    Socket(String),
    /// A variable in `[Environment]`.
    Environment(String),
}

/// What an override key holds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyValue {
    /// Not in the file.
    Absent,
    /// A socket listed as granted (`x11`).
    Granted,
    /// A socket listed as denied (`!x11`).
    Denied,
    /// A variable's value.
    Value(String),
}

/// One key a fix changed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyChange {
    pub key: OverrideKey,
    pub previous: KeyValue,
    pub written: KeyValue,
}

/// A key that no longer holds what the fix wrote, and what it holds now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedElsewhere {
    pub key: OverrideKey,
    pub now: KeyValue,
}

/// What a fix changed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Recorded {
    /// The font cache was rebuilt; nothing to put back.
    RebuiltFontCache,
    /// Keys set in the person's override file for one app.
    OverrideKeys {
        app: AppId,
        /// The app's name when it was fixed, for the page to show once the
        /// fault is gone.
        #[serde(default)]
        name: AppName,
        /// There was no file before the fix, so an undo that leaves it
        /// saying nothing removes it.
        created_file: bool,
        /// Groups the fix added, which an undo takes out again when they are
        /// left empty.
        created_groups: Vec<String>,
        /// The `[Context] sockets` value exactly as the person wrote it
        /// before the fix (`None`: there was none), so an undo that takes
        /// every socket change back writes it back character for character.
        #[serde(default)]
        sockets_before: Option<String>,
        keys: Vec<KeyChange>,
    },
}

/// One applied fix.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppliedRecord {
    pub entry: EntryId,
    pub subject: Subject,
    /// When it was applied, in seconds since the Unix epoch.
    pub applied_at: u64,
    pub change: Recorded,
}

/// Every fix that is applied and can be undone, as kept on disk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Records {
    pub schema_version: u32,
    pub applied: Vec<AppliedRecord>,
}

impl Default for Records {
    fn default() -> Self {
        Self {
            schema_version: RECORDS_SCHEMA_VERSION,
            applied: Vec::new(),
        }
    }
}

impl Records {
    pub fn find(&self, entry: EntryId, subject: &Subject) -> Option<&AppliedRecord> {
        self.applied
            .iter()
            .find(|record| record.entry == entry && &record.subject == subject)
    }

    /// Every record of one entry.
    pub fn of(&self, entry: EntryId) -> impl Iterator<Item = &AppliedRecord> {
        self.applied
            .iter()
            .filter(move |record| record.entry == entry)
    }

    /// Keeps a fix's record, replacing an earlier one for the same subject
    /// and merging its keys, so a second fix of the same app still undoes to
    /// what was there before the first. A fix that needs no undo is not
    /// kept: there is nothing it could put back.
    pub fn keep(&mut self, entry: &dyn Entry, record: AppliedRecord) {
        if entry.undo_kind() == UndoKind::NotNeeded {
            return;
        }
        match self
            .applied
            .iter_mut()
            .find(|kept| kept.entry == record.entry && kept.subject == record.subject)
        {
            Some(kept) => {
                if let (
                    Recorded::OverrideKeys {
                        keys: kept_keys,
                        created_groups: kept_groups,
                        ..
                    },
                    Recorded::OverrideKeys {
                        keys: new_keys,
                        created_groups: new_groups,
                        ..
                    },
                ) = (&mut kept.change, &record.change)
                {
                    for change in new_keys {
                        match kept_keys.iter_mut().find(|kept| kept.key == change.key) {
                            Some(kept) => kept.written = change.written.clone(),
                            None => kept_keys.push(change.clone()),
                        }
                    }
                    for group in new_groups {
                        if !kept_groups.contains(group) {
                            kept_groups.push(group.clone());
                        }
                    }
                    kept.applied_at = record.applied_at;
                } else {
                    *kept = record;
                }
            }
            None => self.applied.push(record),
        }
    }

    /// Replaces a record with what an undo left of it, or drops it.
    pub fn after_undo(&mut self, entry: EntryId, subject: &Subject, remaining: Option<Recorded>) {
        match remaining {
            Some(change) => {
                if let Some(record) = self
                    .applied
                    .iter_mut()
                    .find(|record| record.entry == entry && &record.subject == subject)
                {
                    record.change = change;
                }
            }
            None => self
                .applied
                .retain(|record| !(record.entry == entry && &record.subject == subject)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entries::{electron_cursor::ElectronCursor, font_cache::FontCache};

    fn app() -> AppId {
        AppId::parse("org.example.Electron").unwrap()
    }

    fn override_record(keys: Vec<KeyChange>, at: u64) -> AppliedRecord {
        AppliedRecord {
            entry: EntryId::ElectronCursor,
            subject: Subject::FlatpakApp(app()),
            applied_at: at,
            change: Recorded::OverrideKeys {
                app: app(),
                name: AppName::default(),
                created_file: true,
                created_groups: vec!["Context".into()],
                sockets_before: None,
                keys,
            },
        }
    }

    fn hint(previous: KeyValue, written: &str) -> KeyChange {
        KeyChange {
            key: OverrideKey::Environment("ELECTRON_OZONE_PLATFORM_HINT".into()),
            previous,
            written: KeyValue::Value(written.into()),
        }
    }

    #[test]
    fn a_record_survives_the_file_unchanged() {
        let mut records = Records::default();
        records.keep(
            &ElectronCursor,
            override_record(vec![hint(KeyValue::Absent, "x11")], 5),
        );
        let text = serde_json::to_string_pretty(&records).unwrap();
        let back: Records = serde_json::from_str(&text).unwrap();
        assert_eq!(back, records);
        assert_eq!(back.schema_version, RECORDS_SCHEMA_VERSION);
    }

    #[test]
    fn a_fix_that_needs_no_undo_is_not_kept() {
        let mut records = Records::default();
        records.keep(
            &FontCache,
            AppliedRecord {
                entry: EntryId::FontconfigCache,
                subject: Subject::Machine,
                applied_at: 1,
                change: Recorded::RebuiltFontCache,
            },
        );
        assert!(records.applied.is_empty());
    }

    #[test]
    fn a_second_fix_keeps_the_value_from_before_the_first() {
        let mut records = Records::default();
        records.keep(
            &ElectronCursor,
            override_record(vec![hint(KeyValue::Absent, "x11")], 1),
        );
        // Someone set the hint to wayland, and the fix ran again.
        records.keep(
            &ElectronCursor,
            override_record(vec![hint(KeyValue::Value("wayland".into()), "x11")], 2),
        );
        let record = records
            .find(EntryId::ElectronCursor, &Subject::FlatpakApp(app()))
            .unwrap();
        assert_eq!(record.applied_at, 2);
        let Recorded::OverrideKeys { keys, .. } = &record.change else {
            panic!("override record");
        };
        assert_eq!(keys, &[hint(KeyValue::Absent, "x11")]);
    }

    #[test]
    fn an_undo_drops_the_record_or_keeps_what_it_left() {
        let mut records = Records::default();
        let subject = Subject::FlatpakApp(app());
        records.keep(
            &ElectronCursor,
            override_record(vec![hint(KeyValue::Absent, "x11")], 1),
        );
        let left = override_record(Vec::new(), 1).change;
        records.after_undo(EntryId::ElectronCursor, &subject, Some(left.clone()));
        assert_eq!(
            records
                .find(EntryId::ElectronCursor, &subject)
                .map(|r| &r.change),
            Some(&left)
        );
        records.after_undo(EntryId::ElectronCursor, &subject, None);
        assert!(records.find(EntryId::ElectronCursor, &subject).is_none());
        assert_eq!(records.of(EntryId::ElectronCursor).count(), 0);
    }
}
