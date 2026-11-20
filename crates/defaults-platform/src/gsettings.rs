//! Desktop-entry integrations that live in GSettings keys: a launcher entry
//! (GNOME Shell's favourites, which the dash and Zorin's taskbar show) and a
//! tool entry point.
//!
//! Both read the session's dconf databases first (its own, then the user
//! databases its profile lists beneath it, [`crate::dconf_profile`]) and,
//! where none of them holds the key, the default the compiled GSettings
//! schema gives it — the value the session itself is using. Unlike [`crate::DconfAdapter`], a launcher entry has
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

use better_core::defaults::{
    AdapterId, DconfDatabase, DefaultsValue, KeyObservation, ObservedValue,
};

use crate::dconf::{ChangesetSender, DatabaseTarget, Layers, session_sender, session_with_dirs};
use crate::dconf_profile::{ProfileError, SessionDatabase};
use crate::gvariant::{ChangeValue, Changeset};
use crate::gvdb::{GVariantValue, schema_default};
use crate::{AdapterRequest, DefaultsAdapter, VerifyOutcome, WriteOutcome, WriteValue, collapse};

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
    target: DatabaseTarget,
    schemas: Vec<PathBuf>,
}

impl Keys {
    /// The session's own database and the ones beneath it, with the
    /// installed schemas.
    fn for_session() -> (Self, Option<Box<dyn ChangesetSender>>) {
        let (database, dirs) = session_with_dirs();
        let sender = session_sender(&database, &dirs);
        (
            Self::for_database(database, Some(dirs), schema_files()),
            sender,
        )
    }

    fn for_database(
        database: Result<SessionDatabase, ProfileError>,
        dirs: Option<(PathBuf, PathBuf)>,
        schemas: Vec<PathBuf>,
    ) -> Self {
        Self {
            target: DatabaseTarget::session(database, dirs),
            schemas,
        }
    }

    fn effective(&self, database: &Layers, key: &str) -> Result<Effective, ObservedValue> {
        // A key no schema defines is not one the session reads, whatever the
        // user's database happens to hold under that path.
        let Some(default) = default_of(&self.schemas, key) else {
            return Err(ObservedValue::Unsupported {
                reason: NO_SUCH_KEY.to_string(),
            });
        };
        Ok(match database.get(key)? {
            Some(value) => Effective::User(value.clone()),
            None => Effective::Default(default),
        })
    }

    /// The databases the request is about: the session's, or the one its
    /// record names.
    fn load(&self, request: &AdapterRequest<'_>) -> Result<Layers, ObservedValue> {
        self.target.for_request(request).load()
    }

    /// The capture: each key as read when the session's own database holds
    /// it, and unset when the value shown comes from a database beneath or
    /// from the schema, so a restore resets the key instead of writing a copy.
    fn capture(
        &self,
        request: &AdapterRequest<'_>,
        read: impl Fn(&AdapterRequest<'_>) -> ObservedValue,
    ) -> ObservedValue {
        let layers = match self.load(request) {
            Ok(layers) => layers,
            Err(observed) => return observed,
        };
        collapse(
            request
                .keys()
                .iter()
                .map(|key| KeyObservation {
                    key: key.clone(),
                    observed: if layers.top.get(key).is_none() {
                        ObservedValue::Unset
                    } else {
                        match request.narrowed_to(key) {
                            Some(narrowed) => read(&narrowed),
                            None => ObservedValue::Unknown {
                                reason: "gsettings.key_not_declared".to_string(),
                            },
                        }
                    },
                })
                .collect(),
        )
    }
}

/// A verify that compares "nothing held" against the capture's own view, so
/// a key reset in the session's database verifies as unset although the
/// session still shows the value beneath it.
fn verify_against(observed: ObservedValue, expected: &ObservedValue) -> VerifyOutcome {
    if !observed.is_determinate() || !expected.is_determinate() {
        return VerifyOutcome::Indeterminate { observed };
    }
    if &observed == expected {
        VerifyOutcome::Matches { observed }
    } else {
        VerifyOutcome::Differs { observed }
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
    keys: &Keys,
    sender: &mut Option<Box<dyn ChangesetSender>>,
    request: &AdapterRequest<'_>,
    changeset: &Changeset,
) -> WriteOutcome {
    let target = keys.target.for_request(request);
    if let Some(refusal) = target.write_refusal() {
        return refusal;
    }
    let Some(sender) = sender.as_mut() else {
        return WriteOutcome::manual(
            NO_WRITE_PATH,
            format!(
                "the dconf service owns these settings; change {} in GNOME instead",
                request.keys().join(", ")
            ),
        );
    };
    match sender.send_to(target.writer_path(), changeset) {
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
                target: DatabaseTarget::file(database.into()),
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

    /// The session's databases and schemas, written through the writer
    /// object of the session's own database when this build has the write
    /// path.
    pub fn for_session() -> Self {
        let (keys, sender) = Keys::for_session();
        Self { keys, sender }
    }

    /// A session database decided by the caller, with `schemas`, written
    /// through `sender` (pointed at that database's writer object). `dirs`
    /// (`$XDG_CONFIG_HOME`, `$XDG_RUNTIME_DIR`) is where a database a record
    /// names is opened.
    pub fn for_database(
        database: Result<SessionDatabase, ProfileError>,
        dirs: Option<(PathBuf, PathBuf)>,
        schemas: Vec<PathBuf>,
        sender: Option<Box<dyn ChangesetSender>>,
    ) -> Self {
        Self {
            keys: Keys::for_database(database, dirs, schemas),
            sender,
        }
    }

    fn list(&self, database: &Layers, key: &str) -> Result<Vec<String>, ObservedValue> {
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

    fn dconf_database(&self) -> Option<DconfDatabase> {
        self.keys.target.id()
    }

    fn capture(&self, request: &AdapterRequest<'_>) -> Option<ObservedValue> {
        Some(self.keys.capture(request, |narrowed| self.read(narrowed)))
    }

    fn verify(&self, request: &AdapterRequest<'_>, expected: &ObservedValue) -> VerifyOutcome {
        let observed = if expected == &ObservedValue::Unset {
            self.keys.capture(request, |narrowed| self.read(narrowed))
        } else {
            self.read(request)
        };
        verify_against(observed, expected)
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        let DefaultsValue::DesktopEntry(entry) = request.desired() else {
            return ObservedValue::Unknown {
                reason: "gsettings.launcher_entry_needs_a_desktop_entry".to_string(),
            };
        };
        let database = match self.keys.load(request) {
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
        let database = match self.keys.load(request) {
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
                    if matches!(database.top.get(key), Some(GVariantValue::TextList(held)) if held == list)
                    {
                        continue;
                    }
                    changeset.set_path(key, ChangeValue::TextList(list.clone()))
                }
                WriteValue::Clear => {
                    if database.top.get(key).is_none() {
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
        send(&self.keys, &mut self.sender, request, &changeset)
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
                target: DatabaseTarget::file(database.into()),
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

    /// The session's databases and schemas, as
    /// [`LauncherEntryAdapter::for_session`].
    pub fn for_session() -> Self {
        let (keys, sender) = Keys::for_session();
        Self { keys, sender }
    }

    /// As [`LauncherEntryAdapter::for_database`].
    pub fn for_database(
        database: Result<SessionDatabase, ProfileError>,
        dirs: Option<(PathBuf, PathBuf)>,
        schemas: Vec<PathBuf>,
        sender: Option<Box<dyn ChangesetSender>>,
    ) -> Self {
        Self {
            keys: Keys::for_database(database, dirs, schemas),
            sender,
        }
    }
}

impl DefaultsAdapter for ToolEntryPointAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::ToolEntryPoint
    }

    fn dconf_database(&self) -> Option<DconfDatabase> {
        self.keys.target.id()
    }

    fn capture(&self, request: &AdapterRequest<'_>) -> Option<ObservedValue> {
        Some(self.keys.capture(request, |narrowed| self.read(narrowed)))
    }

    fn verify(&self, request: &AdapterRequest<'_>, expected: &ObservedValue) -> VerifyOutcome {
        let observed = if expected == &ObservedValue::Unset {
            self.keys.capture(request, |narrowed| self.read(narrowed))
        } else {
            self.read(request)
        };
        verify_against(observed, expected)
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        let database = match self.keys.load(request) {
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
        send(&self.keys, &mut self.sender, request, &changeset)
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

    fn session_over(root: &std::path::Path) -> SessionDatabase {
        SessionDatabase {
            name: "betteros".to_string(),
            path: root.join("config/dconf/betteros"),
            writer_path: "/ca/desrt/dconf/Writer/betteros".to_string(),
            beneath: vec![root.join("config/dconf/user")],
            origin: crate::dconf_profile::ProfileOrigin::BuiltIn,
            notes: Vec::new(),
        }
    }

    // Review finding 6: a damaged database beneath is reported as that, for
    // the key that needed it.
    #[test]
    fn a_damaged_database_beneath_is_reported_for_a_key_that_needs_it() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("config/dconf")).unwrap();
        std::fs::write(directory.path().join("config/dconf/user"), b"damaged").unwrap();
        let adapter = LauncherEntryAdapter::for_database(
            Ok(session_over(directory.path())),
            None,
            vec![SCHEMAS.into()],
            None,
        );
        let declared = launcher(FAVORITES);
        let ObservedValue::Unknown { reason } =
            adapter.read(&AdapterRequest::new(&component(), &declared))
        else {
            panic!("a value that needs a damaged database is not known");
        };
        assert!(
            reason.starts_with("dconf.database_beneath_unreadable:"),
            "{reason}"
        );
    }

    // Review finding 2: a value the session's own database does not hold is
    // captured as unset, so a restore resets the key rather than copying the
    // value shown into the session's database, and that reset verifies.
    #[test]
    fn a_value_not_held_by_the_session_s_database_is_captured_as_unset() {
        let adapter = LauncherEntryAdapter::new(EMPTY_DATABASE, vec![SCHEMAS.into()]);
        let declared = launcher(FAVORITES);
        let component = component();
        let request = AdapterRequest::new(&component, &declared);
        assert!(matches!(adapter.read(&request), ObservedValue::Set { .. }));
        assert_eq!(adapter.capture(&request), Some(ObservedValue::Unset));
        assert!(matches!(
            adapter.verify(&request, &ObservedValue::Unset),
            VerifyOutcome::Matches { .. }
        ));
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
