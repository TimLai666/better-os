//! GNOME keybindings and desktop settings: read out of the user's dconf
//! database, written through the dconf service.
//!
//! - **Reads** the user's own dconf database directly, as a typed GVariant
//!   value. No `gsettings`, no shell, no formatted string.
//! - **Writes** by sending one change set to `ca.desrt.dconf.Writer.Change`
//!   over the session bus (the `dconf_writer` module). Every declared key goes in
//!   the same change set, so the service applies all of them or none. The
//!   database file itself is never edited: the service owns it, caches it, and
//!   rewrites it, so bytes written underneath it would be ignored or clobbered.
//! - **Verifies** by reading the database again and comparing typed values. A
//!   write the service accepted is not a success until that read agrees.
//!
//! An adapter built without a way to reach the service — [`DconfAdapter::new`],
//! or any build without the `dconf-write` feature — still reads and verifies,
//! and answers a change with [`WriteOutcome::ManualActionRequired`] naming the
//! keys the user has to change.
//!
//! A key the user's database does not hold reads as [`ObservedValue::Unset`]:
//! nothing is set in the user's own scope. That is the reading a restore can
//! reproduce, by resetting the key so the session's default applies again,
//! rather than by writing whatever this adapter might guess that default is.
//! What the compiled GSettings schema then says is not read and not claimed.

use std::path::{Path, PathBuf};

use better_core::defaults::{AdapterId, DefaultsValue, KeyObservation, ObservedValue};

use crate::gvariant::{ChangeValue, Changeset};
use crate::gvdb::{GVariantValue, GvdbDatabase};
use crate::{AdapterRequest, DefaultsAdapter, WriteOutcome, WriteValue, collapse};

/// Why this adapter will not write, in the words the user needs to act on it.
const NO_WRITE_PATH: &str = "gnome.dconf_write_needs_the_dconf_service";

/// Hands one change set to whatever applies it. In production that is the
/// dconf service on the session bus; a test hands it to a fake, or to a real
/// dconf service on a private bus.
pub trait ChangesetSender {
    fn send(&mut self, changeset: &Changeset) -> Result<(), String>;
}

#[cfg(feature = "dconf-write")]
impl ChangesetSender for crate::dconf_writer::DconfWriter {
    fn send(&mut self, changeset: &Changeset) -> Result<(), String> {
        self.change(changeset)
            .map(|_tag| ())
            .map_err(|error| error.to_string())
    }
}

/// The session's own dconf service, connected on the first write rather than
/// when the adapter is built, so reading and planning never open a bus
/// connection. A connection that fails is reported with that write and tried
/// again on the next one.
#[cfg(feature = "dconf-write")]
#[derive(Default)]
pub struct SessionSender {
    writer: Option<crate::dconf_writer::DconfWriter>,
}

#[cfg(feature = "dconf-write")]
impl ChangesetSender for SessionSender {
    fn send(&mut self, changeset: &Changeset) -> Result<(), String> {
        if self.writer.is_none() {
            self.writer = Some(
                crate::dconf_writer::DconfWriter::connect().map_err(|error| error.to_string())?,
            );
        }
        match self.writer.as_mut() {
            Some(writer) => writer.send(changeset),
            None => Err("no connection to the dconf service".to_string()),
        }
    }
}

pub struct DconfAdapter {
    id: AdapterId,
    path: PathBuf,
    sender: Option<Box<dyn ChangesetSender>>,
}

impl DconfAdapter {
    /// An adapter that reads `path` and has no way to write.
    pub fn new(id: AdapterId, path: impl Into<PathBuf>) -> Self {
        Self {
            id,
            path: path.into(),
            sender: None,
        }
    }

    /// An adapter that reads `path` and writes through `sender`.
    pub fn with_sender(
        id: AdapterId,
        path: impl Into<PathBuf>,
        sender: Box<dyn ChangesetSender>,
    ) -> Self {
        Self {
            id,
            path: path.into(),
            sender: Some(sender),
        }
    }

    /// The per-user database at the location the XDG base directory
    /// specification names, written through the session's dconf service when
    /// this build has the write path.
    pub fn for_user(id: AdapterId) -> Self {
        #[cfg(feature = "dconf-write")]
        {
            Self::with_sender(id, user_database_path(), Box::new(SessionSender::default()))
        }
        #[cfg(not(feature = "dconf-write"))]
        {
            Self::new(id, user_database_path())
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a change can be sent at all.
    pub fn can_write(&self) -> bool {
        self.sender.is_some()
    }

    fn no_write_path(&self, request: &AdapterRequest<'_>) -> WriteOutcome {
        WriteOutcome::manual(
            NO_WRITE_PATH,
            format!(
                "the dconf service owns {}; change {} in GNOME Settings instead",
                self.path.display(),
                request.keys().join(", ")
            ),
        )
    }

    fn load(&self) -> Result<GvdbDatabase, ObservedValue> {
        match std::fs::read(&self.path) {
            Ok(bytes) => GvdbDatabase::parse(&bytes).map_err(|error| ObservedValue::Unknown {
                reason: format!("dconf.database_unreadable:{error}"),
            }),
            // dconf creates the database on the first write, so a user who has
            // never changed a setting has no file at all.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(GvdbDatabase::default())
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

impl std::fmt::Debug for DconfAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DconfAdapter")
            .field("id", &self.id)
            .field("path", &self.path)
            .field("can_write", &self.can_write())
            .finish()
    }
}

/// The GVariant a typed value is stored as. A desktop entry id has none: no
/// dconf-backed integration stores one, and writing it as a plain string would
/// read back as text and never verify.
fn change_value(value: &DefaultsValue) -> Option<ChangeValue> {
    match value {
        DefaultsValue::Text(text) => Some(ChangeValue::Text(text.clone())),
        DefaultsValue::TextList(values) => Some(ChangeValue::TextList(values.clone())),
        DefaultsValue::Boolean(value) => Some(ChangeValue::Boolean(*value)),
        DefaultsValue::DesktopEntry(_) => None,
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
        // Nothing in the user's scope. The session's default applies, and
        // resetting the key is how to get back here.
        None => ObservedValue::Unset,
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
                .map(|key| KeyObservation {
                    key: key.clone(),
                    observed: observe(database.get(key)),
                })
                .collect(),
        )
    }

    fn write(&mut self, request: &AdapterRequest<'_>, value: &WriteValue) -> WriteOutcome {
        if !self.can_write() {
            return self.no_write_path(request);
        }
        let (change, target) = match value {
            WriteValue::Set { value } => match change_value(value) {
                Some(change) => (
                    Some(change),
                    ObservedValue::Set {
                        value: value.clone(),
                    },
                ),
                None => {
                    return WriteOutcome::failed(
                        "dconf.value_has_no_dconf_type",
                        format!("{value:?} cannot be stored in a dconf key"),
                    );
                }
            },
            WriteValue::Clear => (None, ObservedValue::Unset),
        };

        // Every key goes in one change set: the service applies all of them or
        // none, so a failure here never leaves half a declaration written.
        let mut changeset = match Changeset::new("/") {
            Ok(changeset) => changeset,
            Err(error) => return WriteOutcome::failed("dconf.invalid_key", error.to_string()),
        };
        for key in request.keys() {
            let staged = match &change {
                Some(change) => changeset.set_path(key, change.clone()),
                None => changeset.reset_path(key),
            };
            if let Err(error) = staged {
                return WriteOutcome::failed("dconf.invalid_key", error.to_string());
            }
        }
        if self.read(request) == target {
            return WriteOutcome::AlreadyCorrect;
        }
        let Some(sender) = self.sender.as_mut() else {
            return self.no_write_path(request);
        };
        match sender.send(&changeset) {
            Ok(()) => WriteOutcome::Written,
            Err(detail) => WriteOutcome::failed("dconf.write_failed", detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gvariant::{ChangeValue, Changeset};
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
    fn a_key_with_no_user_value_reads_as_nothing_set() {
        // "Nothing set" is a fact about the user's database, and it is the one
        // reading a restore can reproduce: by resetting the key. What the
        // compiled schema default then says is not claimed.
        let observed = read(&["/org/gnome/desktop/wm/keybindings/minimize"]);
        assert_eq!(observed, ObservedValue::Unset);
        assert!(observed.is_determinate());
    }

    #[test]
    fn an_empty_string_array_is_a_value_rather_than_nothing_set() {
        assert_eq!(
            read(&["/org/gnome/desktop/wm/keybindings/close"]),
            ObservedValue::Set {
                value: DefaultsValue::TextList(Vec::new())
            }
        );
    }

    #[test]
    fn declared_keys_that_disagree_do_not_collapse_into_a_winner() {
        let observed = read(&[
            "/org/gnome/settings-daemon/plugins/media-keys/home",
            "/org/gnome/settings-daemon/plugins/media-keys/www",
        ]);
        let ObservedValue::Mixed { per_key } = observed else {
            panic!("two keys holding different values must be read one by one");
        };
        assert_eq!(per_key.len(), 2);
        assert_eq!(
            per_key[1].observed,
            ObservedValue::Set {
                value: DefaultsValue::TextList(vec![
                    "<Super>b".to_string(),
                    "<Super>w".to_string()
                ])
            }
        );
    }

    #[test]
    fn a_missing_database_means_nothing_has_been_set() {
        // dconf creates the file on the first write, so a user who has never
        // changed a setting has none. `touchpad-platform` reads it the same way.
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&["/org/gnome/settings-daemon/plugins/media-keys/home"]);
        let adapter = DconfAdapter::new(AdapterId::GnomeKeybinding, "/nonexistent/dconf/user");
        assert_eq!(
            adapter.read(&AdapterRequest::new(&component, &integration)),
            ObservedValue::Unset
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
    fn an_adapter_with_no_write_path_names_the_keys_and_changes_nothing() {
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

    /// Records every change set it is handed, and answers as it was told to.
    /// The dconf service is the only thing that writes the database, so a fake
    /// that accepts a change leaves the database exactly as it was.
    #[derive(Clone, Default)]
    struct FakeSender {
        sent: std::rc::Rc<std::cell::RefCell<Vec<Changeset>>>,
        refuse: Option<String>,
    }

    impl ChangesetSender for FakeSender {
        fn send(&mut self, changeset: &Changeset) -> Result<(), String> {
            self.sent.borrow_mut().push(changeset.clone());
            match &self.refuse {
                Some(reason) => Err(reason.clone()),
                None => Ok(()),
            }
        }
    }

    const HOME: &str = "/org/gnome/settings-daemon/plugins/media-keys/home";
    const MINIMIZE: &str = "/org/gnome/desktop/wm/keybindings/minimize";

    fn writing(sender: &FakeSender) -> DconfAdapter {
        DconfAdapter::with_sender(
            AdapterId::GnomeKeybinding,
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dconf/user"),
            Box::new(sender.clone()),
        )
    }

    fn expected(build: impl FnOnce(&mut Changeset)) -> Vec<Changeset> {
        let mut changeset = Changeset::new("/").unwrap();
        build(&mut changeset);
        vec![changeset]
    }

    #[test]
    fn applying_sends_every_declared_key_in_one_change_set() {
        let sender = FakeSender::default();
        let mut adapter = writing(&sender);
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&[HOME, MINIMIZE]);

        assert_eq!(
            adapter.apply(&AdapterRequest::new(&component, &integration)),
            WriteOutcome::Written
        );
        let desired = ChangeValue::TextList(vec!["<Super>e".to_string()]);
        assert_eq!(
            *sender.sent.borrow(),
            expected(|changeset| {
                changeset.set_path(HOME, desired.clone()).unwrap();
                changeset.set_path(MINIMIZE, desired.clone()).unwrap();
            })
        );
    }

    #[test]
    fn restoring_a_key_that_held_nothing_resets_it_rather_than_writing_a_default() {
        let sender = FakeSender::default();
        let mut adapter = writing(&sender);
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&[HOME]);

        assert_eq!(
            adapter.restore(
                &AdapterRequest::new(&component, &integration),
                &ObservedValue::Unset
            ),
            WriteOutcome::Written
        );
        assert_eq!(
            *sender.sent.borrow(),
            expected(|changeset| changeset.reset_path(HOME).unwrap())
        );
    }

    #[test]
    fn a_value_the_key_already_holds_is_not_sent_again() {
        let sender = FakeSender::default();
        let mut adapter = writing(&sender);
        let component = ComponentId::new("better-files").unwrap();
        let mut integration = integration(&[HOME]);
        integration.target.desired = DefaultsValue::TextList(vec!["<Super>e".to_string()]);

        assert_eq!(
            adapter.apply(&AdapterRequest::new(&component, &integration)),
            WriteOutcome::AlreadyCorrect
        );
        assert!(sender.sent.borrow().is_empty());
    }

    #[test]
    fn a_refused_write_is_a_failure_and_the_database_is_untouched() {
        let sender = FakeSender {
            refuse: Some("the dconf service refused the call: denied".to_string()),
            ..FakeSender::default()
        };
        let mut adapter = writing(&sender);
        let before = std::fs::read(adapter.path()).unwrap();
        let component = ComponentId::new("better-files").unwrap();
        let mut integration = integration(&[HOME]);
        integration.target.desired = DefaultsValue::TextList(vec!["<Super>f".to_string()]);

        let WriteOutcome::Failed { reason, detail } =
            adapter.apply(&AdapterRequest::new(&component, &integration))
        else {
            panic!("a refused write must be reported as a failure");
        };
        assert_eq!(reason, "dconf.write_failed");
        assert!(detail.unwrap_or_default().contains("denied"));
        assert_eq!(std::fs::read(adapter.path()).unwrap(), before);
    }

    #[test]
    fn a_value_with_no_dconf_type_is_refused_before_anything_is_sent() {
        let sender = FakeSender::default();
        let mut adapter = writing(&sender);
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&[HOME]);

        let outcome = adapter.write(
            &AdapterRequest::new(&component, &integration),
            &WriteValue::Set {
                value: DefaultsValue::DesktopEntry("io.betteros.Files.desktop".to_string()),
            },
        );
        assert!(matches!(
            outcome,
            WriteOutcome::Failed { ref reason, .. } if reason == "dconf.value_has_no_dconf_type"
        ));
        assert!(sender.sent.borrow().is_empty());
    }

    #[test]
    fn a_key_that_is_not_a_dconf_path_is_refused_before_anything_is_sent() {
        let sender = FakeSender::default();
        let mut adapter = writing(&sender);
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&[HOME, "org/gnome/not-absolute"]);

        assert!(matches!(
            adapter.apply(&AdapterRequest::new(&component, &integration)),
            WriteOutcome::Failed { ref reason, .. } if reason == "dconf.invalid_key"
        ));
        assert!(sender.sent.borrow().is_empty());
    }
}
