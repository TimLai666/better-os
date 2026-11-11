//! The default-application adapter, built on Better App Chooser's association
//! store.
//!
//! There is no second `mimeapps.list` parser here. Reading and writing both go
//! through `app_chooser_core`, which keeps the file byte-faithful, changes
//! exactly one line per association, and writes a rollback record before it
//! touches anything. Reusing it is not a convenience: a second editor would be
//! a second set of rules about a file the user owns.
//!
//! Putting a default back is writing the previous owner. Putting *no default*
//! back is the association store's typed removal, which takes the type's own
//! line out of `[Default Applications]` and leaves every other byte alone, so
//! restoring an integration that had no default before leaves it with none
//! rather than with a value Better OS invented.

use app_catalog_core::{DesktopId, MimeType};
use app_chooser_core::AssociationStore;
use better_core::defaults::{AdapterId, DefaultsValue, KeyObservation, ObservedValue};

use crate::{AdapterRequest, DefaultsAdapter, WriteOutcome, WriteValue, collapse};

#[derive(Clone, Debug)]
pub struct XdgDefaultAppAdapter {
    id: AdapterId,
    store: AssociationStore,
}

impl XdgDefaultAppAdapter {
    pub fn new(id: AdapterId, store: AssociationStore) -> Self {
        Self { id, store }
    }

    /// The writing adapter over the user's own association file.
    pub fn for_user() -> Result<Self, app_chooser_core::AssociationError> {
        Ok(Self::new(
            AdapterId::XdgDefaultApp,
            AssociationStore::for_user()?,
        ))
    }

    /// The same reading, through the id a manifest may name as its verify
    /// adapter. It refuses to write, which is why the manifest schema refuses
    /// to accept it as an apply adapter.
    pub fn read_only(store: AssociationStore) -> Self {
        Self::new(AdapterId::XdgEffectiveDefault, store)
    }

    /// The reading adapter over the user's own association file.
    pub fn effective_for_user() -> Result<Self, app_chooser_core::AssociationError> {
        Ok(Self::read_only(AssociationStore::for_user()?))
    }

    pub fn store(&self) -> &AssociationStore {
        &self.store
    }
}

impl DefaultsAdapter for XdgDefaultAppAdapter {
    fn id(&self) -> AdapterId {
        self.id
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        let file = match self.store.load() {
            Ok(file) => file,
            Err(error) => {
                return ObservedValue::Unknown {
                    reason: format!("xdg.mimeapps_unreadable:{error}"),
                };
            }
        };
        let associations = file.associations();
        collapse(
            request
                .keys()
                .iter()
                .map(|key| KeyObservation {
                    key: key.clone(),
                    observed: match MimeType::parse(key) {
                        None => ObservedValue::Unsupported {
                            reason: format!("xdg.invalid_mime_type:{key}"),
                        },
                        Some(mime) => match associations.default_for(&mime) {
                            Some(desktop_id) => ObservedValue::Set {
                                value: DefaultsValue::DesktopEntry(desktop_id.as_str().to_string()),
                            },
                            None => ObservedValue::Unset,
                        },
                    },
                })
                .collect(),
        )
    }

    fn write(&mut self, request: &AdapterRequest<'_>, value: &WriteValue) -> WriteOutcome {
        if self.id == AdapterId::XdgEffectiveDefault {
            return WriteOutcome::manual(
                "xdg.read_only_adapter",
                "this adapter only reports the effective default",
            );
        }
        // `None` clears the type's default rather than writing one.
        let desktop_id = match value {
            WriteValue::Clear => None,
            WriteValue::Set {
                value: DefaultsValue::DesktopEntry(id),
            } => match DesktopId::new(id.clone()) {
                Ok(desktop_id) => Some(desktop_id),
                Err(error) => {
                    return WriteOutcome::failed(
                        "xdg.invalid_desktop_id",
                        format!("{id}: {error}"),
                    );
                }
            },
            WriteValue::Set { value } => {
                return WriteOutcome::failed(
                    "xdg.value_is_not_a_desktop_entry",
                    format!("{value:?}"),
                );
            }
        };

        let mut changed = 0usize;
        for (index, key) in request.keys().iter().enumerate() {
            let Some(mime) = MimeType::parse(key) else {
                return WriteOutcome::failed(
                    "xdg.invalid_mime_type",
                    format!("{key}; {changed} of {index} earlier keys were written"),
                );
            };
            let result = match &desktop_id {
                Some(desktop_id) => self.store.set_default_id(&mime, desktop_id, Vec::new()),
                None => self.store.remove_default(&mime),
            };
            match result {
                Ok(outcome) if outcome.changed => changed += 1,
                Ok(_) => {}
                Err(error) => {
                    return WriteOutcome::failed(
                        "xdg.write_failed",
                        format!("{key}: {error}; {changed} earlier keys were written"),
                    );
                }
            }
        }
        if changed == 0 {
            WriteOutcome::AlreadyCorrect
        } else {
            WriteOutcome::Written
        }
    }
}
