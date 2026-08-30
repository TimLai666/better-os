//! Reading GNOME keybindings and desktop settings out of the user's dconf
//! database.
//!
//! What this adapter does and does not do is worth stating plainly, because the
//! difference is the whole point of the manual-action outcome:
//!
//! - **Reads** the user's own dconf database directly, as a typed GVariant
//!   value. No `gsettings`, no shell, no formatted string.
//! - **Verifies** by reading again and comparing typed values.
//! - **Does not write.** A user-scope dconf write has to go through the dconf
//!   service, which owns the file and rewrites it; editing the bytes underneath
//!   it would be ignored or clobbered. Rather than guess a command, an attempt
//!   to change a setting returns [`WriteOutcome::ManualActionRequired`] naming
//!   the keys the user has to change and why Better OS did not.
//!
//! A key the user's database does not hold is reported as unknown, not as a
//! default. The effective value in that case comes from the compiled GSettings
//! schema, which this adapter does not read, and inventing an answer would be
//! exactly the guess this crate exists to avoid.

use std::path::{Path, PathBuf};

use better_core::defaults::{AdapterId, DefaultsValue, ObservedValue};

use crate::gvdb::{GVariantValue, GvdbDatabase};
use crate::{AdapterRequest, DefaultsAdapter, WriteOutcome, WriteValue, collapse};

/// Why this adapter will not write, in the words the user needs to act on it.
const NO_WRITE_PATH: &str = "gnome.dconf_write_needs_the_dconf_service";

#[derive(Clone, Debug)]
pub struct DconfAdapter {
    id: AdapterId,
    path: PathBuf,
}

impl DconfAdapter {
    pub fn new(id: AdapterId, path: impl Into<PathBuf>) -> Self {
        Self {
            id,
            path: path.into(),
        }
    }

    /// The per-user database at the location the XDG base directory
    /// specification names.
    pub fn for_user(id: AdapterId) -> Self {
        Self::new(id, user_database_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> Result<GvdbDatabase, ObservedValue> {
        match std::fs::read(&self.path) {
            Ok(bytes) => GvdbDatabase::parse(&bytes).map_err(|error| ObservedValue::Unknown {
                reason: format!("dconf.database_unreadable:{error}"),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(ObservedValue::Unknown {
                    reason: "dconf.no_user_database".to_string(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                Err(ObservedValue::PermissionDenied {
                    reason: "dconf.database_not_readable".to_string(),
                })
            }
            Err(error) => Err(ObservedValue::Unknown {
                reason: format!("dconf.database_unreadable:{error}"),
            }),
        }
    }
}

fn user_database_path() -> PathBuf {
    let config = match std::env::var("XDG_CONFIG_HOME") {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
    };
    config.join("dconf/user")
}

fn observe(value: Option<&GVariantValue>) -> ObservedValue {
    match value {
        // A key with no user-scope entry falls back to the compiled schema
        // default, which this adapter does not read.
        None => ObservedValue::Unknown {
            reason: "dconf.no_user_scope_value".to_string(),
        },
        Some(GVariantValue::Text(text)) => ObservedValue::Set {
            value: DefaultsValue::Text(text.clone()),
        },
        Some(GVariantValue::TextList(values)) => ObservedValue::Set {
            value: DefaultsValue::TextList(values.clone()),
        },
        Some(GVariantValue::Boolean(value)) => ObservedValue::Set {
            value: DefaultsValue::Boolean(*value),
        },
        // A double is decodable but has no `DefaultsValue` to become. Better
        // Touchpad reads those keys through `GvdbDatabase` directly rather than
        // widening this schema for a value no integration declares.
        Some(GVariantValue::Double(_)) => ObservedValue::Unsupported {
            reason: "dconf.unsupported_value_type:d".to_string(),
        },
        Some(GVariantValue::Unsupported { signature }) => ObservedValue::Unsupported {
            reason: format!("dconf.unsupported_value_type:{signature}"),
        },
        Some(GVariantValue::Malformed { signature }) => ObservedValue::Unknown {
            reason: format!("dconf.malformed_value:{signature}"),
        },
    }
}

impl DefaultsAdapter for DconfAdapter {
    fn id(&self) -> AdapterId {
        self.id
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        let database = match self.load() {
            Ok(database) => database,
            Err(observed) => return observed,
        };
        collapse(
            request
                .keys()
                .iter()
                .map(|key| observe(database.get(key)))
                .collect(),
        )
    }

    fn write(&mut self, request: &AdapterRequest<'_>, _value: &WriteValue) -> WriteOutcome {
        WriteOutcome::manual(
            NO_WRITE_PATH,
            format!(
                "the dconf service owns {}; change {} in GNOME Settings instead",
                self.path.display(),
                request.keys().join(", ")
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use better_core::defaults::{
        DefaultIntegration, IntegrationExclusivity, IntegrationId, IntegrationKind,
        IntegrationTarget, RequiredPrivilege, RestorePolicy, SessionEffect,
    };
    use better_core::manifest::ComponentId;

    fn integration(keys: &[&str]) -> DefaultIntegration {
        DefaultIntegration {
            id: IntegrationId::new("open-file-manager-shortcut").unwrap(),
            kind: IntegrationKind::GlobalShortcut,
            exclusivity: IntegrationExclusivity::Exclusive,
            target: IntegrationTarget {
                desired: DefaultsValue::TextList(vec!["<Super>e".to_string()]),
                keys: keys.iter().map(|key| key.to_string()).collect(),
            },
            platforms: vec!["zorin".to_string()],
            sessions: vec!["gnome".to_string()],
            apply_adapter: AdapterId::GnomeKeybinding,
            verify_adapter: AdapterId::GnomeKeybinding,
            restore_policy: RestorePolicy::CapturedValue,
            privileges: RequiredPrivilege::User,
            session_effect: SessionEffect::Immediate,
            health_prerequisites: Vec::new(),
        }
    }

    fn adapter() -> DconfAdapter {
        DconfAdapter::new(
            AdapterId::GnomeKeybinding,
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dconf/user"),
        )
    }

    fn read(keys: &[&str]) -> ObservedValue {
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(keys);
        adapter().read(&AdapterRequest::new(&component, &integration))
    }

    #[test]
    fn reads_the_keybinding_the_user_database_holds() {
        assert_eq!(
            read(&["/org/gnome/settings-daemon/plugins/media-keys/home"]),
            ObservedValue::Set {
                value: DefaultsValue::TextList(vec!["<Super>e".to_string()])
            }
        );
    }

    #[test]
    fn a_key_with_no_user_value_is_unknown_rather_than_assumed() {
        let observed = read(&["/org/gnome/desktop/wm/keybindings/minimize"]);
        assert_eq!(
            observed,
            ObservedValue::Unknown {
                reason: "dconf.no_user_scope_value".to_string()
            }
        );
        assert!(!observed.is_determinate());
    }

    #[test]
    fn declared_keys_that_disagree_do_not_collapse_into_a_winner() {
        let observed = read(&[
            "/org/gnome/settings-daemon/plugins/media-keys/home",
            "/org/gnome/settings-daemon/plugins/media-keys/www",
        ]);
        assert!(matches!(observed, ObservedValue::Unknown { .. }));
    }

    #[test]
    fn a_missing_database_is_unknown_not_empty() {
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&["/org/gnome/settings-daemon/plugins/media-keys/home"]);
        let adapter = DconfAdapter::new(AdapterId::GnomeKeybinding, "/nonexistent/dconf/user");
        assert_eq!(
            adapter.read(&AdapterRequest::new(&component, &integration)),
            ObservedValue::Unknown {
                reason: "dconf.no_user_database".to_string()
            }
        );
    }

    #[test]
    fn verifying_a_value_that_is_there_matches_and_one_that_is_not_differs() {
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&["/org/gnome/settings-daemon/plugins/media-keys/home"]);
        let request = AdapterRequest::new(&component, &integration);
        let adapter = adapter();

        assert!(matches!(
            adapter.verify(
                &request,
                &ObservedValue::Set {
                    value: DefaultsValue::TextList(vec!["<Super>e".to_string()])
                }
            ),
            crate::VerifyOutcome::Matches { .. }
        ));
        assert!(matches!(
            adapter.verify(
                &request,
                &ObservedValue::Set {
                    value: DefaultsValue::TextList(vec!["<Super>f".to_string()])
                }
            ),
            crate::VerifyOutcome::Differs { .. }
        ));
    }

    #[test]
    fn applying_names_the_keys_and_changes_nothing() {
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&["/org/gnome/settings-daemon/plugins/media-keys/home"]);
        let request = AdapterRequest::new(&component, &integration);
        let mut adapter = adapter();
        let before = std::fs::read(adapter.path()).unwrap();

        let outcome = adapter.apply(&request);
        let WriteOutcome::ManualActionRequired { reason, detail } = outcome else {
            panic!("a dconf write must not report success");
        };
        assert_eq!(reason, NO_WRITE_PATH);
        assert!(
            detail
                .unwrap_or_default()
                .contains("/org/gnome/settings-daemon/plugins/media-keys/home")
        );
        assert_eq!(std::fs::read(adapter.path()).unwrap(), before);
    }
}
