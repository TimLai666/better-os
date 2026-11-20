//! GNOME keybindings and desktop settings: read out of the user's dconf
//! database, written through the dconf service.
//!
//! - **Reads** the session's own dconf database directly, as a typed GVariant
//!   value. No `gsettings`, no shell, no formatted string. Which database
//!   that is comes from the session's dconf profile ([`crate::dconf_profile`]):
//!   `~/.config/dconf/user` under GNOME, the session's own under a profile
//!   that puts another `user-db` first.
//! - **Writes** by sending one change set to `ca.desrt.dconf.Writer.Change`
//!   over the session bus (the `dconf_writer` module), at the writer object of
//!   that same database. Every declared key goes in
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

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use better_core::defaults::{
    AdapterId, DconfDatabase, DefaultsValue, KeyObservation, ObservedValue,
};

use crate::dconf_profile::{ProfileError, SessionDatabase};
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

    /// Sends to the writer object at `writer_path`, which an adapter names
    /// when a defaults record says the value lives in another database than
    /// the one this sender was made for. A sender with only one destination
    /// (a test's fake) keeps [`Self::send`]'s.
    fn send_to(&mut self, writer_path: &str, changeset: &Changeset) -> Result<(), String> {
        let _ = writer_path;
        self.send(changeset)
    }
}

#[cfg(feature = "dconf-write")]
impl ChangesetSender for crate::dconf_writer::DconfWriter {
    fn send(&mut self, changeset: &Changeset) -> Result<(), String> {
        self.change(changeset)
            .map(|_tag| ())
            .map_err(|error| error.to_string())
    }

    fn send_to(&mut self, writer_path: &str, changeset: &Changeset) -> Result<(), String> {
        self.change_at(writer_path, changeset)
            .map(|_tag| ())
            .map_err(|error| error.to_string())
    }
}

/// The session's own dconf service, connected on the first write rather than
/// when the adapter is built, so reading and planning never open a bus
/// connection. A connection that fails is reported with that write and tried
/// again on the next one.
#[cfg(feature = "dconf-write")]
pub struct SessionSender {
    writer_path: String,
    writer: Option<crate::dconf_writer::DconfWriter>,
}

#[cfg(feature = "dconf-write")]
impl SessionSender {
    /// A sender to the writer object of `database`.
    pub fn for_database(database: &SessionDatabase) -> Self {
        Self::at(&database.writer_path)
    }

    /// A sender to the writer object at `writer_path`.
    pub fn at(writer_path: &str) -> Self {
        Self {
            writer_path: writer_path.to_string(),
            writer: None,
        }
    }

    fn writer(&mut self) -> Result<&mut crate::dconf_writer::DconfWriter, String> {
        if self.writer.is_none() {
            self.writer = Some(
                crate::dconf_writer::DconfWriter::connect_session(&self.writer_path)
                    .map_err(|error| error.to_string())?,
            );
        }
        self.writer
            .as_mut()
            .ok_or_else(|| "no connection to the dconf service".to_string())
    }
}

#[cfg(feature = "dconf-write")]
impl ChangesetSender for SessionSender {
    fn send(&mut self, changeset: &Changeset) -> Result<(), String> {
        self.writer()?.send(changeset)
    }

    fn send_to(&mut self, writer_path: &str, changeset: &Changeset) -> Result<(), String> {
        self.writer()?.send_to(writer_path, changeset)
    }
}

pub struct DconfAdapter {
    id: AdapterId,
    target: DatabaseTarget,
    sender: Option<Box<dyn ChangesetSender>>,
}

impl DconfAdapter {
    /// An adapter that reads `path` and has no way to write.
    pub fn new(id: AdapterId, path: impl Into<PathBuf>) -> Self {
        Self {
            id,
            target: DatabaseTarget::file(path.into()),
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
            target: DatabaseTarget::file(path.into()),
            sender: Some(sender),
        }
    }

    /// The database the session reads first, as its dconf profile decides,
    /// written through `sender` (which a caller points at the same
    /// database's writer object). `dirs` is where a database a record names
    /// is opened: `$XDG_CONFIG_HOME` and `$XDG_RUNTIME_DIR`.
    pub fn for_database(
        id: AdapterId,
        database: Result<SessionDatabase, ProfileError>,
        dirs: Option<(PathBuf, PathBuf)>,
        sender: Option<Box<dyn ChangesetSender>>,
    ) -> Self {
        Self {
            id,
            target: DatabaseTarget::session(database, dirs),
            sender,
        }
    }

    /// This process's session: the database its dconf profile writes to,
    /// written through that database's writer object when this build has the
    /// write path. A profile that cannot be decided leaves the adapter
    /// reporting why on every read and write of a value it has no record
    /// for; a recorded value is still read and put back where it was written.
    pub fn for_session(id: AdapterId) -> Self {
        let (database, dirs) = session_with_dirs();
        let sender = session_sender(&database, &dirs);
        Self::for_database(id, database, Some(dirs), sender)
    }

    /// The database file read, or the empty path when the session's profile
    /// could not be decided ([`Self::unavailable`] says why).
    pub fn path(&self) -> &Path {
        &self.target.path
    }

    /// Why the session's database could not be decided, if it could not.
    pub fn unavailable(&self) -> Option<&ProfileError> {
        self.target.unavailable.as_ref()
    }

    /// Problems dconf warns about and works around, from the profile.
    pub fn notes(&self) -> &[String] {
        &self.target.notes
    }

    /// Whether a change can be sent at all.
    pub fn can_write(&self) -> bool {
        self.sender.is_some() && self.target.unavailable.is_none()
    }
}

/// This process's session database, and the two directories a recorded one
/// is opened under.
pub(crate) fn session_with_dirs() -> (Result<SessionDatabase, ProfileError>, (PathBuf, PathBuf)) {
    let environment = crate::dconf_profile::ProfileEnvironment::from_env();
    let dirs = (
        environment.config_home.clone(),
        environment.runtime_dir.clone(),
    );
    let database = crate::dconf_profile::session_database_in(&environment);
    if let Ok(database) = &database {
        for note in &database.notes {
            eprintln!("better-os: {note}");
        }
    }
    (database, dirs)
}

/// The sender production uses: the session database's writer object, or
/// for a session whose database could not be decided, one that only ever
/// writes a database a record names (every write names one).
pub(crate) fn session_sender(
    database: &Result<SessionDatabase, ProfileError>,
    dirs: &(PathBuf, PathBuf),
) -> Option<Box<dyn ChangesetSender>> {
    #[cfg(feature = "dconf-write")]
    {
        let _ = dirs;
        let writer_path = match database {
            Ok(database) => database.writer_path.clone(),
            Err(_) => DconfDatabase::GNOME_USER_WRITER.to_string(),
        };
        Some(Box::new(SessionSender::at(&writer_path)) as Box<dyn ChangesetSender>)
    }
    #[cfg(not(feature = "dconf-write"))]
    {
        let _ = (database, dirs);
        None
    }
}

/// Which database files an adapter reads: the session's own, the user
/// databases its profile lists beneath it, or why there is no answer.
#[derive(Clone, Debug)]
pub(crate) struct DatabaseTarget {
    pub(crate) path: PathBuf,
    writer_path: String,
    name: String,
    beneath: Vec<PathBuf>,
    /// `$XDG_CONFIG_HOME` and `$XDG_RUNTIME_DIR`, to open a database a record
    /// names. A file adapter knows the first from its own path.
    dirs: Option<(PathBuf, Option<PathBuf>)>,
    notes: Vec<String>,
    pub(crate) unavailable: Option<ProfileError>,
}

/// The session's own database and the ones beneath it, read. A database
/// beneath that cannot be read fails only a key that needs it.
pub(crate) struct Layers {
    pub(crate) top: GvdbDatabase,
    beneath: Vec<(PathBuf, Result<GvdbDatabase, ObservedValue>)>,
}

impl Layers {
    /// The value the session sees from its user databases: its own first,
    /// then each one beneath in profile order.
    pub(crate) fn get(&self, key: &str) -> Result<Option<&GVariantValue>, ObservedValue> {
        if let Some(value) = self.top.get(key) {
            return Ok(Some(value));
        }
        for (path, database) in &self.beneath {
            match database {
                Ok(database) => {
                    if let Some(value) = database.get(key) {
                        return Ok(Some(value));
                    }
                }
                Err(_) => {
                    return Err(ObservedValue::Unknown {
                        reason: format!("dconf.database_beneath_unreadable:{}", path.display()),
                    });
                }
            }
        }
        Ok(None)
    }
}

impl DatabaseTarget {
    /// One database file named by the caller, with nothing beneath it: a
    /// `user-db` named after the file, under the configuration directory two
    /// levels up (`<config>/dconf/<name>`).
    pub(crate) fn file(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let config_home = path
            .parent()
            .filter(|dir| dir.file_name().is_some_and(|name| name == "dconf"))
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        Self {
            writer_path: format!("{}{name}", crate::dconf_profile::WRITER_PATH_PREFIX),
            name,
            path,
            beneath: Vec::new(),
            dirs: config_home.map(|config| (config, None)),
            notes: Vec::new(),
            unavailable: None,
        }
    }

    pub(crate) fn session(
        database: Result<SessionDatabase, ProfileError>,
        dirs: Option<(PathBuf, PathBuf)>,
    ) -> Self {
        let dirs = dirs.map(|(config, runtime)| (config, Some(runtime)));
        match database {
            Ok(database) => Self {
                path: database.path,
                writer_path: database.writer_path,
                name: database.name,
                beneath: database.beneath,
                dirs,
                notes: database.notes,
                unavailable: None,
            },
            Err(error) => Self {
                path: PathBuf::new(),
                writer_path: String::new(),
                name: String::new(),
                beneath: Vec::new(),
                dirs,
                notes: Vec::new(),
                unavailable: Some(error),
            },
        }
    }

    /// The database this target writes, when it has one.
    pub(crate) fn id(&self) -> Option<DconfDatabase> {
        self.unavailable.is_none().then(|| DconfDatabase {
            name: self.name.clone(),
            writer_path: self.writer_path.clone(),
        })
    }

    pub(crate) fn writer_path(&self) -> &str {
        &self.writer_path
    }

    /// The target a request is about: this one, or the database the
    /// request's record names when that is another.
    pub(crate) fn for_request(&self, request: &AdapterRequest<'_>) -> Cow<'_, DatabaseTarget> {
        let Some(recorded) = request.dconf_database() else {
            return Cow::Borrowed(self);
        };
        if self.id().as_ref() == Some(recorded) {
            return Cow::Borrowed(self);
        }
        let opened = match &self.dirs {
            Some((config, runtime)) => {
                SessionDatabase::recorded(recorded, config, runtime.as_deref())
            }
            None => Err(ProfileError::RecordedDatabaseInvalid {
                name: recorded.name.clone(),
                writer_path: recorded.writer_path.clone(),
            }),
        };
        Cow::Owned(DatabaseTarget::session(opened, None).with_dirs(self.dirs.clone()))
    }

    fn with_dirs(mut self, dirs: Option<(PathBuf, Option<PathBuf>)>) -> Self {
        self.dirs = dirs;
        self
    }

    fn refusal(&self) -> Option<ObservedValue> {
        self.unavailable
            .as_ref()
            .map(|error| ObservedValue::Unknown {
                reason: format!("{}:{error}", error.reason()),
            })
    }

    /// The session's own database alone.
    pub(crate) fn load_top(&self) -> Result<GvdbDatabase, ObservedValue> {
        if let Some(refusal) = self.refusal() {
            return Err(refusal);
        }
        load_user_database(&self.path)
    }

    /// The session's own database and every one beneath it.
    pub(crate) fn load(&self) -> Result<Layers, ObservedValue> {
        Ok(Layers {
            top: self.load_top()?,
            beneath: self
                .beneath
                .iter()
                .map(|path| (path.clone(), load_user_database(path)))
                .collect(),
        })
    }

    /// Why a write to this target cannot be made, if it cannot.
    pub(crate) fn write_refusal(&self) -> Option<WriteOutcome> {
        self.unavailable
            .as_ref()
            .map(|error| WriteOutcome::manual(error.reason(), error.to_string()))
    }
}

/// The user's dconf database, read directly. A missing file is an empty
/// database: dconf creates it on the first write.
pub(crate) fn load_user_database(path: &Path) -> Result<GvdbDatabase, ObservedValue> {
    match std::fs::read(path) {
        Ok(bytes) => GvdbDatabase::parse(&bytes).map_err(|error| ObservedValue::Unknown {
            reason: format!("dconf.database_unreadable:{error}"),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(GvdbDatabase::default()),
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

impl std::fmt::Debug for DconfAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DconfAdapter")
            .field("id", &self.id)
            .field("path", &self.target.path)
            .field("unavailable", &self.target.unavailable)
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

    fn dconf_database(&self) -> Option<DconfDatabase> {
        self.target.id()
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        // The session's own scope only: a key it does not hold reads as
        // unset, and resetting it is how a restore gets back there.
        let database = match self.target.for_request(request).load_top() {
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
        let target = self.target.for_request(request).into_owned();
        if let Some(refusal) = target.write_refusal() {
            return refusal;
        }
        if self.sender.is_none() {
            return no_write_path(&target, request);
        }
        let (change, wanted) = match value {
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
        if self.read(request) == wanted {
            return WriteOutcome::AlreadyCorrect;
        }
        let Some(sender) = self.sender.as_mut() else {
            return no_write_path(&target, request);
        };
        match sender.send_to(target.writer_path(), &changeset) {
            Ok(()) => WriteOutcome::Written,
            Err(detail) => WriteOutcome::failed("dconf.write_failed", detail),
        }
    }
}

fn no_write_path(target: &DatabaseTarget, request: &AdapterRequest<'_>) -> WriteOutcome {
    WriteOutcome::manual(
        NO_WRITE_PATH,
        format!(
            "the dconf service owns {}; change {} in GNOME Settings instead",
            target.path.display(),
            request.keys().join(", ")
        ),
    )
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

    // Ticket 207: a session whose dconf profile cannot be decided is reported
    // on every read and write, and nothing is sent to any writer.
    #[test]
    fn an_undecided_session_database_is_reported_and_nothing_is_sent() {
        let sender = FakeSender::default();
        let mut adapter = DconfAdapter::for_database(
            AdapterId::GnomeKeybinding,
            Err(ProfileError::NotTheSessionEnvironment {
                desktop: "betteros:cosmic".to_string(),
            }),
            None,
            Some(Box::new(sender.clone())),
        );
        let component = ComponentId::new("better-files").unwrap();
        let mut integration = integration(&[HOME]);
        integration.target.desired = DefaultsValue::TextList(vec!["<Super>f".to_string()]);
        let request = AdapterRequest::new(&component, &integration);

        let ObservedValue::Unknown { reason } = adapter.read(&request) else {
            panic!("an undecided database must not read as a value");
        };
        assert!(reason.starts_with("dconf.session_profile_not_in_environment:"));
        assert!(!adapter.can_write());
        let WriteOutcome::ManualActionRequired { reason, .. } = adapter.apply(&request) else {
            panic!("an undecided database must not be written");
        };
        assert_eq!(reason, "dconf.session_profile_not_in_environment");
        let restored = adapter.restore(&request, &ObservedValue::Unset);
        assert!(!matches!(restored, WriteOutcome::Written));
        assert!(sender.sent.borrow().is_empty());
    }

    // The session's own scope is what this adapter reads: a key only the
    // database beneath holds reads as unset, which a restore reproduces by
    // resetting the key in the session's own database.
    #[test]
    fn only_the_session_s_own_database_is_read_not_the_one_beneath() {
        let fixture = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/dconf/user"
        ));
        let adapter = DconfAdapter::for_database(
            AdapterId::GnomeKeybinding,
            Ok(SessionDatabase {
                name: "betteros".to_string(),
                path: PathBuf::from("/nonexistent/dconf/betteros"),
                writer_path: "/ca/desrt/dconf/Writer/betteros".to_string(),
                beneath: vec![fixture],
                origin: crate::dconf_profile::ProfileOrigin::BuiltIn,
                notes: Vec::new(),
            }),
            None,
            None,
        );
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&[HOME]);
        assert_eq!(
            adapter.read(&AdapterRequest::new(&component, &integration)),
            ObservedValue::Unset
        );
        assert_eq!(adapter.path(), Path::new("/nonexistent/dconf/betteros"));
    }

    /// Records the writer object each change set was sent to.
    #[derive(Clone, Default)]
    struct RoutedSender {
        sent: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    }

    impl ChangesetSender for RoutedSender {
        fn send(&mut self, _changeset: &Changeset) -> Result<(), String> {
            Err("a routed write must name its writer".to_string())
        }

        fn send_to(&mut self, writer_path: &str, _changeset: &Changeset) -> Result<(), String> {
            self.sent.borrow_mut().push(writer_path.to_string());
            Ok(())
        }
    }

    fn session_in(root: &Path) -> SessionDatabase {
        SessionDatabase {
            name: "betteros".to_string(),
            path: root.join("config/dconf/betteros"),
            writer_path: "/ca/desrt/dconf/Writer/betteros".to_string(),
            beneath: vec![root.join("config/dconf/user")],
            origin: crate::dconf_profile::ProfileOrigin::BuiltIn,
            notes: Vec::new(),
        }
    }

    // Review finding 1: a record names the database it was written to, and
    // the adapter reads and writes that one, not the session's.
    #[test]
    fn a_request_naming_a_recorded_database_reads_and_writes_that_database() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("config/dconf")).unwrap();
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dconf/user"),
            root.join("config/dconf/user"),
        )
        .unwrap();
        let sender = RoutedSender::default();
        let mut adapter = DconfAdapter::for_database(
            AdapterId::GnomeKeybinding,
            Ok(session_in(root)),
            Some((root.join("config"), root.join("run"))),
            Some(Box::new(sender.clone())),
        );
        assert_eq!(
            adapter.dconf_database().unwrap().writer_path,
            "/ca/desrt/dconf/Writer/betteros"
        );
        let component = ComponentId::new("better-files").unwrap();
        let mut integration = integration(&[HOME]);
        integration.target.desired = DefaultsValue::TextList(vec!["<Super>f".to_string()]);
        let gnome = DconfDatabase::gnome_user();
        let recorded =
            AdapterRequest::new(&component, &integration).in_dconf_database(Some(&gnome));

        assert_eq!(
            adapter.read(&recorded),
            ObservedValue::Set {
                value: DefaultsValue::TextList(vec!["<Super>e".to_string()])
            }
        );
        assert_eq!(
            adapter.read(&AdapterRequest::new(&component, &integration)),
            ObservedValue::Unset
        );
        assert_eq!(adapter.apply(&recorded), WriteOutcome::Written);
        assert_eq!(
            adapter.apply(&AdapterRequest::new(&component, &integration)),
            WriteOutcome::Written
        );
        assert_eq!(
            *sender.sent.borrow(),
            vec![
                "/ca/desrt/dconf/Writer/user".to_string(),
                "/ca/desrt/dconf/Writer/betteros".to_string()
            ]
        );

        // A session whose profile could not be decided still reads and puts
        // back what a record says it wrote.
        let mut undecided = DconfAdapter::for_database(
            AdapterId::GnomeKeybinding,
            Err(ProfileError::NotTheSessionEnvironment {
                desktop: "cosmic".to_string(),
            }),
            Some((root.join("config"), root.join("run"))),
            Some(Box::new(sender.clone())),
        );
        assert!(matches!(
            undecided.read(&recorded),
            ObservedValue::Set { .. }
        ));
        assert_eq!(
            undecided.restore(&recorded, &ObservedValue::Unset),
            WriteOutcome::Written
        );
        let forged = DconfDatabase {
            name: "user".to_string(),
            writer_path: "/elsewhere".to_string(),
        };
        let forged = AdapterRequest::new(&component, &integration).in_dconf_database(Some(&forged));
        assert!(matches!(
            undecided.read(&forged),
            ObservedValue::Unknown { ref reason } if reason.starts_with("dconf.recorded_database_invalid:")
        ));
        assert!(matches!(
            undecided.apply(&forged),
            WriteOutcome::ManualActionRequired { ref reason, .. } if reason == "dconf.recorded_database_invalid"
        ));
    }

    // Review finding 6: a damaged database beneath does not touch a reader
    // that only uses the session's own.
    #[test]
    fn a_damaged_database_beneath_does_not_fail_a_read_of_the_session_s_own() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("config/dconf")).unwrap();
        std::fs::write(root.join("config/dconf/user"), b"not a database").unwrap();
        let adapter = DconfAdapter::for_database(
            AdapterId::GnomeKeybinding,
            Ok(session_in(root)),
            None,
            None,
        );
        let component = ComponentId::new("better-files").unwrap();
        let integration = integration(&[HOME]);
        assert_eq!(
            adapter.read(&AdapterRequest::new(&component, &integration)),
            ObservedValue::Unset
        );
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
