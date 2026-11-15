//! Desktop-entry integrations that live in GSettings keys: a launcher entry
//! (GNOME Shell's favourites, which the dash and Zorin's taskbar show) and a
//! tool entry point.
//!
//! Both read the user's dconf database first and, where the user never set
//! the key, the default the compiled GSettings schema gives it — the value
//! GNOME itself is using. Unlike [`crate::DconfAdapter`], a launcher entry has
//! to know that default: adding one application to a list the user never
//! touched means writing the whole list GNOME was showing plus that one, and
//! writing the application alone would empty the dock.
//!
//! A key that no installed schema defines is read by nothing in the session,
//! so these adapters report it as not supported here rather than writing a
//! value no program would ever look at. GNOME 46 has no
//! `/org/gnome/desktop/applications/file-manager`, for example: that schema
//! directory holds only `terminal` and `office`.
//!
//! Writes go through the same dconf service, one change set per declaration.

use std::path::PathBuf;

use better_core::defaults::{AdapterId, DefaultsValue, KeyObservation, ObservedValue};

use crate::dconf::{ChangesetSender, load_user_database, user_database_path};
use crate::gvariant::{ChangeValue, Changeset};
use crate::gvdb::{GVariantValue, GvdbDatabase, schema_default};
use crate::{AdapterRequest, DefaultsAdapter, WriteOutcome, WriteValue, collapse};

/// The reason a key no schema defines is reported with.
pub const NO_SUCH_KEY: &str = "gsettings.no_such_key";
const NO_WRITE_PATH: &str = "gnome.dconf_write_needs_the_dconf_service";

/// Where GLib looks for compiled schemas, most important first:
/// `GSETTINGS_SCHEMA_DIR`, the user's data directory, then `XDG_DATA_DIRS`.
pub fn schema_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(dirs) = std::env::var("GSETTINGS_SCHEMA_DIR") {
        files.extend(
            dirs.split(':')
                .filter(|dir| !dir.is_empty())
                .map(|dir| PathBuf::from(dir).join("gschemas.compiled")),
        );
    }
    let user = match std::env::var("XDG_DATA_HOME") {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"),
    };
    files.push(user.join("glib-2.0/schemas/gschemas.compiled"));
    let system = match std::env::var("XDG_DATA_DIRS") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => "/usr/local/share/:/usr/share/".to_string(),
    };
    files.extend(
        system
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(dir).join("glib-2.0/schemas/gschemas.compiled")),
    );
    files
}

/// The first compiled schema that defines `key`, as GLib resolves it. A file
/// that is missing or unreadable is skipped, as GLib skips it.
fn default_of(files: &[PathBuf], key: &str) -> Option<GVariantValue> {
    files.iter().find_map(|file| {
        let bytes = std::fs::read(file).ok()?;
        schema_default(&bytes, key).ok().flatten()
    })
}

/// What a key holds in effect: the user's value, or the schema's default.
enum Effective {
    User(GVariantValue),
    Default(GVariantValue),
}

impl Effective {
    fn value(&self) -> &GVariantValue {
        match self {
            Self::User(value) | Self::Default(value) => value,
        }
    }
}

struct Keys {
    database: PathBuf,
    schemas: Vec<PathBuf>,
}

impl Keys {
    fn effective(&self, database: &GvdbDatabase, key: &str) -> Result<Effective, ObservedValue> {
        // A key no schema defines is not one the session reads, whatever the
        // user's database happens to hold under that path.
        let Some(default) = default_of(&self.schemas, key) else {
            return Err(ObservedValue::Unsupported {
                reason: NO_SUCH_KEY.to_string(),
            });
        };
        Ok(match database.get(key) {
            Some(value) => Effective::User(value.clone()),
            None => Effective::Default(default),
        })
    }

    fn load(&self) -> Result<GvdbDatabase, ObservedValue> {
        load_user_database(&self.database)
    }
}

fn wrong_type(value: &GVariantValue) -> ObservedValue {
    match value {
        GVariantValue::Malformed { signature } => ObservedValue::Unknown {
            reason: format!("dconf.malformed_value:{signature}"),
        },
        GVariantValue::Unsupported { signature } => ObservedValue::Unsupported {
            reason: format!("dconf.unsupported_value_type:{signature}"),
        },
        _ => ObservedValue::Unknown {
            reason: "gsettings.unexpected_value_type".to_string(),
        },
    }
}

fn send(
    sender: &mut Option<Box<dyn ChangesetSender>>,
    request: &AdapterRequest<'_>,
    changeset: &Changeset,
) -> WriteOutcome {
    let Some(sender) = sender.as_mut() else {
        return WriteOutcome::manual(
            NO_WRITE_PATH,
            format!(
                "the dconf service owns these settings; change {} in GNOME instead",
                request.keys().join(", ")
            ),
        );
    };
    match sender.send(changeset) {
        Ok(()) => WriteOutcome::Written,
        Err(detail) => WriteOutcome::failed("dconf.write_failed", detail),
    }
}

/// A desktop entry in a launcher list such as GNOME Shell's favourites.
///
/// It reads as the declared entry when the list contains it, and as the
/// whole list otherwise — so a capture before a change is exactly what the
/// dock showed. Applying adds the entry at the end and leaves every other
/// entry where it was; a restore writes back the captured list (or resets a
/// key the user never set).
pub struct LauncherEntryAdapter {
    keys: Keys,
    sender: Option<Box<dyn ChangesetSender>>,
}

impl LauncherEntryAdapter {
    /// Reads `database` and the given compiled schemas, and cannot write.
    pub fn new(database: impl Into<PathBuf>, schemas: Vec<PathBuf>) -> Self {
        Self {
            keys: Keys {
                database: database.into(),
                schemas,
            },
            sender: None,
        }
    }

    pub fn with_sender(
        database: impl Into<PathBuf>,
        schemas: Vec<PathBuf>,
        sender: Box<dyn ChangesetSender>,
    ) -> Self {
        Self {
            sender: Some(sender),
            ..Self::new(database, schemas)
        }
    }

    /// The user's own database and schemas, written through the session's
    /// dconf service when this build has the write path.
    pub fn for_user() -> Self {
        #[cfg(feature = "dconf-write")]
        {
            Self::with_sender(
                user_database_path(),
                schema_files(),
                Box::new(crate::dconf::SessionSender::default()),
            )
        }
        #[cfg(not(feature = "dconf-write"))]
        {
            Self::new(user_database_path(), schema_files())
        }
    }

    fn list(&self, database: &GvdbDatabase, key: &str) -> Result<Vec<String>, ObservedValue> {
        let effective = self.keys.effective(database, key)?;
        match effective.value() {
            GVariantValue::TextList(values) => Ok(values.clone()),
            other => Err(wrong_type(other)),
        }
    }
}

impl DefaultsAdapter for LauncherEntryAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::DesktopLauncherEntry
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        let DefaultsValue::DesktopEntry(entry) = request.desired() else {
            return ObservedValue::Unknown {
                reason: "gsettings.launcher_entry_needs_a_desktop_entry".to_string(),
            };
        };
        let database = match self.keys.load() {
            Ok(database) => database,
            Err(observed) => return observed,
        };
        collapse(
            request
                .keys()
                .iter()
                .map(|key| KeyObservation {
                    key: key.clone(),
                    observed: match self.list(&database, key) {
                        Ok(list) if list.contains(entry) => ObservedValue::Set {
                            value: DefaultsValue::DesktopEntry(entry.clone()),
                        },
                        Ok(list) => ObservedValue::Set {
                            value: DefaultsValue::TextList(list),
                        },
                        Err(observed) => observed,
                    },
                })
                .collect(),
        )
    }

    fn write(&mut self, request: &AdapterRequest<'_>, value: &WriteValue) -> WriteOutcome {
        let database = match self.keys.load() {
            Ok(database) => database,
            Err(observed) => {
                return WriteOutcome::failed(
                    "gsettings.unreadable",
                    format!("the list could not be read before writing: {observed:?}"),
                );
            }
        };
        let mut changeset = match Changeset::new("/") {
            Ok(changeset) => changeset,
            Err(error) => return WriteOutcome::failed("dconf.invalid_key", error.to_string()),
        };
        let mut changes = 0;
        for key in request.keys() {
            let staged = match value {
                WriteValue::Set {
                    value: DefaultsValue::DesktopEntry(entry),
                } => {
                    let mut list = match self.list(&database, key) {
                        Ok(list) => list,
                        Err(observed) => {
                            return WriteOutcome::manual(
                                "gsettings.launcher_list_unavailable",
                                format!("{key}: {observed:?}"),
                            );
                        }
                    };
                    if list.contains(entry) {
                        continue;
                    }
                    list.push(entry.clone());
                    changeset.set_path(key, ChangeValue::TextList(list))
                }
                WriteValue::Set {
                    value: DefaultsValue::TextList(list),
                } => {
                    if matches!(database.get(key), Some(GVariantValue::TextList(held)) if held == list)
                    {
                        continue;
                    }
                    changeset.set_path(key, ChangeValue::TextList(list.clone()))
                }
                WriteValue::Clear => {
                    if database.get(key).is_none() {
                        continue;
                    }
                    changeset.reset_path(key)
                }
                WriteValue::Set { value } => {
                    return WriteOutcome::failed(
                        "gsettings.launcher_entry_value",
                        format!("{value:?} is not a launcher list or entry"),
                    );
                }
            };
            if let Err(error) = staged {
                return WriteOutcome::failed("dconf.invalid_key", error.to_string());
            }
            changes += 1;
        }
        if changes == 0 {
            return WriteOutcome::AlreadyCorrect;
        }
        send(&mut self.sender, request, &changeset)
    }
}

/// A desktop entry stored as a string in a GSettings key: the tool entry
/// point a desktop opens for one job.
///
/// On a desktop whose schemas do not define the declared key it reads as not
/// supported, with [`NO_SUCH_KEY`], and changes nothing: nothing in the
/// session reads such a key.
pub struct ToolEntryPointAdapter {
    keys: Keys,
    sender: Option<Box<dyn ChangesetSender>>,
}

impl ToolEntryPointAdapter {
    pub fn new(database: impl Into<PathBuf>, schemas: Vec<PathBuf>) -> Self {
        Self {
            keys: Keys {
                database: database.into(),
                schemas,
            },
            sender: None,
        }
    }

    pub fn with_sender(
        database: impl Into<PathBuf>,
        schemas: Vec<PathBuf>,
        sender: Box<dyn ChangesetSender>,
    ) -> Self {
        Self {
            sender: Some(sender),
            ..Self::new(database, schemas)
        }
    }

    pub fn for_user() -> Self {
        #[cfg(feature = "dconf-write")]
        {
            Self::with_sender(
                user_database_path(),
                schema_files(),
                Box::new(crate::dconf::SessionSender::default()),
            )
        }
        #[cfg(not(feature = "dconf-write"))]
        {
            Self::new(user_database_path(), schema_files())
        }
    }
}

impl DefaultsAdapter for ToolEntryPointAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::ToolEntryPoint
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        let database = match self.keys.load() {
            Ok(database) => database,
            Err(observed) => return observed,
        };
        collapse(
            request
                .keys()
                .iter()
                .map(|key| KeyObservation {
                    key: key.clone(),
                    observed: match self.keys.effective(&database, key) {
                        Ok(Effective::User(GVariantValue::Text(text)))
                        | Ok(Effective::Default(GVariantValue::Text(text))) => {
                            if text.is_empty() {
                                ObservedValue::Unset
                            } else {
                                ObservedValue::Set {
                                    value: DefaultsValue::DesktopEntry(text),
                                }
                            }
                        }
                        Ok(other) => wrong_type(other.value()),
                        Err(observed) => observed,
                    },
                })
                .collect(),
        )
    }

    fn write(&mut self, request: &AdapterRequest<'_>, value: &WriteValue) -> WriteOutcome {
        let current = self.read(request);
        if let ObservedValue::Unsupported { reason } = &current {
            return WriteOutcome::manual(
                reason.clone(),
                format!("no installed schema defines {}", request.keys().join(", ")),
            );
        }
        let (change, target) = match value {
            WriteValue::Set {
                value: DefaultsValue::DesktopEntry(entry),
            } => (
                Some(ChangeValue::Text(entry.clone())),
                ObservedValue::Set {
                    value: DefaultsValue::DesktopEntry(entry.clone()),
                },
            ),
            WriteValue::Clear => (None, ObservedValue::Unset),
            WriteValue::Set { value } => {
                return WriteOutcome::failed(
                    "gsettings.tool_entry_value",
                    format!("{value:?} is not a desktop entry"),
                );
            }
        };
        if current == target {
            return WriteOutcome::AlreadyCorrect;
        }
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
        send(&mut self.sender, request, &changeset)
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
    use std::cell::RefCell;
    use std::rc::Rc;

    const SCHEMAS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/schemas/gschemas.compiled"
    );
    /// The dconf fixture holds no key under `/org/example/`.
    const EMPTY_DATABASE: &str = "/nonexistent/dconf/user";
    const FAVORITES: &str = "/org/example/shell/favorite-apps";
    const FILES: &str = "io.betteros.Files.desktop";

    #[derive(Clone, Default)]
    struct FakeSender {
        sent: Rc<RefCell<Vec<Changeset>>>,
    }

    impl ChangesetSender for FakeSender {
        fn send(&mut self, changeset: &Changeset) -> Result<(), String> {
            self.sent.borrow_mut().push(changeset.clone());
            Ok(())
        }
    }

    fn integration(kind: IntegrationKind, adapter: AdapterId, key: &str) -> DefaultIntegration {
        DefaultIntegration {
            id: IntegrationId::new("entry").unwrap(),
            kind,
            exclusivity: IntegrationExclusivity::Shared,
            target: IntegrationTarget {
                desired: DefaultsValue::DesktopEntry(FILES.to_string()),
                keys: vec![key.to_string()],
            },
            platforms: vec!["zorin".to_string()],
            sessions: vec!["gnome".to_string()],
            apply_adapter: adapter,
            verify_adapter: adapter,
            restore_policy: RestorePolicy::LeaveInPlace,
            privileges: RequiredPrivilege::User,
            session_effect: SessionEffect::Immediate,
            health_prerequisites: Vec::new(),
        }
    }

    fn launcher(key: &str) -> DefaultIntegration {
        integration(
            IntegrationKind::DesktopLauncherEntry,
            AdapterId::DesktopLauncherEntry,
            key,
        )
    }

    fn component() -> ComponentId {
        ComponentId::new("better-files").unwrap()
    }

    fn list(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn a_list_the_user_never_set_reads_as_the_schema_s_default() {
        let adapter = LauncherEntryAdapter::new(EMPTY_DATABASE, vec![SCHEMAS.into()]);
        let declared = launcher(FAVORITES);
        assert_eq!(
            adapter.read(&AdapterRequest::new(&component(), &declared)),
            ObservedValue::Set {
                value: DefaultsValue::TextList(list(&[
                    "org.gnome.Nautilus.desktop",
                    "firefox.desktop"
                ]))
            }
        );
    }

    /// Adding to a list the user never set writes GNOME's own list plus the
    /// entry: writing the entry alone would empty the dock.
    #[test]
    fn applying_adds_the_entry_to_the_end_of_the_list_gnome_shows() {
        let sender = FakeSender::default();
        let mut adapter = LauncherEntryAdapter::with_sender(
            EMPTY_DATABASE,
            vec![SCHEMAS.into()],
            Box::new(sender.clone()),
        );
        let declared = launcher(FAVORITES);
        assert_eq!(
            adapter.apply(&AdapterRequest::new(&component(), &declared)),
            WriteOutcome::Written
        );
        let mut expected = Changeset::new("/").unwrap();
        expected
            .set_path(
                FAVORITES,
                ChangeValue::TextList(list(&[
                    "org.gnome.Nautilus.desktop",
                    "firefox.desktop",
                    FILES,
                ])),
            )
            .unwrap();
        assert_eq!(*sender.sent.borrow(), vec![expected]);
    }

    #[test]
    fn restoring_a_list_the_user_never_set_resets_it() {
        let sender = FakeSender::default();
        let mut adapter = LauncherEntryAdapter::with_sender(
            EMPTY_DATABASE,
            vec![SCHEMAS.into()],
            Box::new(sender.clone()),
        );
        let declared = launcher(FAVORITES);
        // Nothing is held in the user's database, so a reset changes nothing.
        assert_eq!(
            adapter.write(
                &AdapterRequest::new(&component(), &declared),
                &WriteValue::Clear
            ),
            WriteOutcome::AlreadyCorrect
        );
        assert!(sender.sent.borrow().is_empty());
    }

    #[test]
    fn a_key_no_schema_defines_is_not_supported_and_never_written() {
        let sender = FakeSender::default();
        let mut adapter = ToolEntryPointAdapter::with_sender(
            EMPTY_DATABASE,
            vec![SCHEMAS.into()],
            Box::new(sender.clone()),
        );
        let declared = integration(
            IntegrationKind::ToolEntryPoint,
            AdapterId::ToolEntryPoint,
            "/org/gnome/desktop/applications/file-manager",
        );
        let owner = component();
        let request = AdapterRequest::new(&owner, &declared);
        assert_eq!(
            adapter.read(&request),
            ObservedValue::Unsupported {
                reason: NO_SUCH_KEY.to_string()
            }
        );
        assert!(matches!(
            adapter.apply(&request),
            WriteOutcome::ManualActionRequired { ref reason, .. } if reason == NO_SUCH_KEY
        ));
        assert!(sender.sent.borrow().is_empty());

        // The launcher reports the same for a list no schema defines.
        let launcher_adapter = LauncherEntryAdapter::new(EMPTY_DATABASE, vec![SCHEMAS.into()]);
        let missing = launcher("/org/example/shell/no-such-list");
        assert_eq!(
            launcher_adapter.read(&AdapterRequest::new(&component(), &missing)),
            ObservedValue::Unsupported {
                reason: NO_SUCH_KEY.to_string()
            }
        );
    }

    #[test]
    fn a_tool_entry_that_is_not_a_string_is_not_guessed_at() {
        let sender = FakeSender::default();
        let adapter = ToolEntryPointAdapter::with_sender(
            EMPTY_DATABASE,
            vec![SCHEMAS.into()],
            Box::new(sender.clone()),
        );
        // `enabled` is a boolean: a tool entry point that is not a string is
        // a reading this adapter refuses to guess at.
        let wrong = integration(
            IntegrationKind::ToolEntryPoint,
            AdapterId::ToolEntryPoint,
            "/org/example/shell/enabled",
        );
        let owner = component();
        let request = AdapterRequest::new(&owner, &wrong);
        assert!(!adapter.read(&request).is_determinate());
        assert!(sender.sent.borrow().is_empty());
    }
}
