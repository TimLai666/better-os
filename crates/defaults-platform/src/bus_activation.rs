//! The `user-service` adapter: which program the session bus starts for a
//! well-known name (ticket 129, ADR 0041).
//!
//! The person's choice is one file, `$XDG_DATA_HOME/dbus-1/services/<name>.service`,
//! which the bus searches before the system's directories. Writing it never
//! stops whoever owns the name now: the file only says what the bus starts
//! when a call finds no owner.
//!
//! A manifest names the bus name and `boolean: true`; it never names the
//! program. [`exec_for`] is the closed table of what Better OS may put in an
//! `Exec=` line, so an untrusted manifest cannot make the bus run anything.

use std::path::{Path, PathBuf};

use better_core::defaults::{AdapterId, DefaultsValue, KeyObservation, ObservedValue};

use crate::{AdapterRequest, DefaultsAdapter, WriteOutcome, WriteValue, collapse};

/// The longest foreign activation file kept whole for a restore. A real one
/// is a few lines; anything bigger is not read as one.
pub const MAX_SERVICE_FILE: u64 = 16 * 1024;

/// The program each component may register for a bus name, by component
/// id and name. Nothing else is ever written.
pub fn exec_for(component: &str, name: &str) -> Option<&'static str> {
    match (component, name) {
        ("better-files", "org.freedesktop.FileManager1") => Some("/usr/bin/better-files --service"),
        _ => None,
    }
}

/// The activation file Better OS writes for `name`, byte for byte.
pub fn service_file(name: &str, exec: &str) -> String {
    format!("[D-BUS Service]\nName={name}\nExec={exec}\n")
}

/// What the bus is to start for a name: the `Exec=` line, and the
/// executable that is then the name's owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Activation {
    pub exec: String,
    pub program: PathBuf,
}

/// The production table, as [`Activation`]s.
pub fn production_activation(component: &str, name: &str) -> Option<Activation> {
    let exec = exec_for(component, name)?;
    Some(Activation {
        exec: exec.to_string(),
        program: PathBuf::from(exec.split_whitespace().next().unwrap_or(exec)),
    })
}

/// The prefix a reading carries when another program owns the name now.
pub const OWNED_BY: &str = better_core::defaults::OWNED_BY_PREFIX;

/// Who owns a bus name now, as the bus says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NameOwner {
    /// Nobody: the next call starts what the activation files say.
    Nobody,
    /// A process, by the executable `/proc/<pid>/exe` names.
    Program(PathBuf),
    /// Someone, and which program could not be told.
    Unknown(String),
}

/// The session bus, as this adapter needs it.
pub trait SessionBus {
    fn owner(&self, name: &str) -> NameOwner;
    /// Makes the bus read its service directories again.
    fn reload(&self) -> Result<(), String>;
}

/// The session bus at an address, or the session's own.
pub struct ZbusSession {
    address: Option<String>,
}

impl ZbusSession {
    pub fn session() -> Self {
        Self { address: None }
    }

    /// Whether the bus answers at all: a removal run from outside the
    /// person's session has none.
    pub fn reachable(&self) -> bool {
        self.connect().is_ok()
    }

    /// A private bus, for tests.
    pub fn at(address: impl Into<String>) -> Self {
        Self {
            address: Some(address.into()),
        }
    }

    fn connect(&self) -> Result<zbus::blocking::Connection, String> {
        match &self.address {
            Some(address) => zbus::blocking::connection::Builder::address(address.as_str())
                .and_then(|builder| builder.build())
                .map_err(|error| error.to_string()),
            None => zbus::blocking::Connection::session().map_err(|error| error.to_string()),
        }
    }
}

impl SessionBus for ZbusSession {
    fn owner(&self, name: &str) -> NameOwner {
        let connection = match self.connect() {
            Ok(connection) => connection,
            Err(error) => return NameOwner::Unknown(format!("bus:{error}")),
        };
        let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&connection) else {
            return NameOwner::Unknown("bus:proxy".to_string());
        };
        let Ok(bus_name) = zbus::names::BusName::try_from(name) else {
            return NameOwner::Unknown("bus:name".to_string());
        };
        let owner = match proxy.get_name_owner(bus_name) {
            Ok(owner) => owner,
            Err(zbus::fdo::Error::NameHasNoOwner(_)) => return NameOwner::Nobody,
            Err(error) => return NameOwner::Unknown(format!("bus:{error}")),
        };
        let pid = match proxy.get_connection_unix_process_id(owner.into()) {
            Ok(pid) => pid,
            Err(error) => return NameOwner::Unknown(format!("bus:{error}")),
        };
        match std::fs::read_link(format!("/proc/{pid}/exe")) {
            Ok(exe) => NameOwner::Program(running_program(&exe)),
            Err(error) => NameOwner::Unknown(format!("exe:{error}")),
        }
    }

    fn reload(&self) -> Result<(), String> {
        let connection = self.connect()?;
        let proxy =
            zbus::blocking::fdo::DBusProxy::new(&connection).map_err(|error| error.to_string())?;
        proxy.reload_config().map_err(|error| error.to_string())
    }
}

/// The program a process runs, as `/proc/<pid>/exe` names it, without the
/// ` (deleted)` the kernel adds once an update replaced the file: the same
/// program, about to restart into its new version.
pub fn running_program(exe: &Path) -> PathBuf {
    let text = exe.to_string_lossy();
    PathBuf::from(text.strip_suffix(" (deleted)").unwrap_or(&text))
}

/// The file a foreign activation file is kept in while Better OS's stands in
/// its place, so a restore puts back the person's own file as it was (its
/// mode included), not only its words.
fn kept_aside(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.before-better-os"))
}

/// Removes the activation files Better OS wrote in `services` for every
/// name `component` may register, when each is byte for byte Better OS's
/// own, and asks the bus to read its directories again. A file the person
/// wrote is left. Returns the names whose file went. This is what the
/// removal of a component does when there is no record to restore from
/// (ADR 0041).
pub fn remove_own_files(
    services: &Path,
    component: &str,
    exec: fn(&str, &str) -> Option<Activation>,
    bus: &dyn SessionBus,
) -> Vec<String> {
    let mut removed = Vec::new();
    for name in REGISTERED_NAMES {
        let Some(activation) = exec(component, name) else {
            continue;
        };
        let path = services.join(format!("{name}.service"));
        let ours = std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file())
            && std::fs::read_to_string(&path)
                .is_ok_and(|content| content == service_file(name, &activation.exec));
        if ours && std::fs::remove_file(&path).is_ok() {
            // The person's own file Better OS's stood in front of comes back
            // into use, as it was.
            let aside = kept_aside(&path);
            if std::fs::symlink_metadata(&aside).is_ok_and(|metadata| metadata.is_file()) {
                let _ = std::fs::rename(&aside, &path);
            }
            remove_empty_parents(services);
            removed.push(name.to_string());
        }
    }
    if !removed.is_empty() {
        let _ = bus.reload();
    }
    removed
}

/// The names the table in [`exec_for`] knows.
pub const REGISTERED_NAMES: &[&str] = &["org.freedesktop.FileManager1"];

/// `dbus-1/services` and `dbus-1`, when restoring left them empty.
fn remove_empty_parents(services: &Path) {
    if std::fs::remove_dir(services).is_ok()
        && let Some(parent) = services.parent()
    {
        let _ = std::fs::remove_dir(parent);
    }
}

/// `$XDG_DATA_HOME/dbus-1/services`, `~/.local/share` when it is not set:
/// the directory the session bus searches first among those that survive a
/// sign-out.
pub fn user_services_dir() -> PathBuf {
    let data = match std::env::var("XDG_DATA_HOME") {
        Ok(value) if Path::new(&value).is_absolute() => PathBuf::from(value),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"),
    };
    data.join("dbus-1/services")
}

/// Reads and writes the user-level activation file of a bus name.
pub struct BusActivationAdapter {
    /// `$XDG_DATA_HOME/dbus-1/services`.
    services: PathBuf,
    bus: Box<dyn SessionBus>,
    /// The program table: [`production_activation`] in production, a
    /// test's own helper in tests.
    exec: fn(&str, &str) -> Option<Activation>,
}

impl BusActivationAdapter {
    pub fn new(
        services: PathBuf,
        bus: Box<dyn SessionBus>,
        exec: fn(&str, &str) -> Option<Activation>,
    ) -> Self {
        Self {
            services,
            bus,
            exec,
        }
    }

    /// The person's own: the directory the session bus searches first among
    /// those that survive a sign-out, and the session bus.
    pub fn for_user() -> Self {
        Self::new(
            user_services_dir(),
            Box::new(ZbusSession::session()),
            production_activation,
        )
    }

    fn path_of(&self, name: &str) -> PathBuf {
        self.services.join(format!("{name}.service"))
    }

    fn read_key(&self, component: &str, name: &str) -> ObservedValue {
        if zbus::names::WellKnownName::try_from(name).is_err() {
            return ObservedValue::Unsupported {
                reason: format!("bus.invalid_name:{name}"),
            };
        }
        let Some(activation) = (self.exec)(component, name) else {
            return ObservedValue::Unsupported {
                reason: format!("bus.no_program_for:{name}"),
            };
        };
        let path = self.path_of(name);
        let content = match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return ObservedValue::Unset;
            }
            Err(error) => {
                return ObservedValue::Unknown {
                    reason: format!("bus.service_file_unreadable:{error}"),
                };
            }
            Ok(metadata) if !metadata.is_file() || metadata.len() > MAX_SERVICE_FILE => {
                return ObservedValue::Unknown {
                    reason: "bus.service_file_not_a_file".to_string(),
                };
            }
            Ok(_) => match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) => {
                    return ObservedValue::Unknown {
                        reason: format!("bus.service_file_unreadable:{error}"),
                    };
                }
            },
        };
        if content != service_file(name, &activation.exec) {
            return ObservedValue::Set {
                value: DefaultsValue::Text(content),
            };
        }
        match self.bus.owner(name) {
            NameOwner::Nobody => ObservedValue::Set {
                value: DefaultsValue::Boolean(true),
            },
            NameOwner::Program(program) if program == activation.program => ObservedValue::Set {
                value: DefaultsValue::Boolean(true),
            },
            NameOwner::Program(program) => ObservedValue::Set {
                value: DefaultsValue::Text(format!("{OWNED_BY}{}", program.display())),
            },
            NameOwner::Unknown(detail) => ObservedValue::Unknown {
                reason: format!("bus.owner_unknown:{detail}"),
            },
        }
    }

    fn write_key(&self, component: &str, name: &str, value: &WriteValue) -> WriteOutcome {
        let Some(activation) = (self.exec)(component, name) else {
            return WriteOutcome::manual(
                "bus.no_program_for",
                format!("Better OS registers nothing for {name}"),
            );
        };
        let path = self.path_of(name);
        let result = match value {
            WriteValue::Set {
                value: DefaultsValue::Boolean(true),
            } => {
                let ours = service_file(name, &activation.exec);
                // Someone's own file is kept aside, whole, for a restore.
                let foreign = std::fs::symlink_metadata(&path)
                    .is_ok_and(|metadata| metadata.is_file())
                    && std::fs::read_to_string(&path).map_or(true, |content| content != ours);
                if foreign
                    && !kept_aside(&path).exists()
                    && let Err(error) = std::fs::rename(&path, kept_aside(&path))
                {
                    return WriteOutcome::failed("bus.service_file_unwritable", error.to_string());
                }
                replace(&path, &ours)
            }
            // A foreign file captured before the change: the very file, as
            // it was kept aside, or else its words.
            WriteValue::Set {
                value: DefaultsValue::Text(content),
            } if !content.starts_with(OWNED_BY) => {
                let aside = kept_aside(&path);
                if std::fs::read_to_string(&aside).is_ok_and(|kept| &kept == content) {
                    std::fs::rename(&aside, &path).map_err(|error| error.to_string())
                } else {
                    let _ = std::fs::remove_file(&aside);
                    replace(&path, content)
                }
            }
            WriteValue::Clear => match std::fs::remove_file(&path) {
                Ok(()) => {
                    let _ = std::fs::remove_file(kept_aside(&path));
                    remove_empty_parents(&self.services);
                    Ok(())
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.to_string()),
            },
            other => {
                return WriteOutcome::manual(
                    "bus.value_not_writable",
                    format!("{other:?} is not an activation file"),
                );
            }
        };
        if let Err(error) = result {
            return WriteOutcome::failed("bus.service_file_unwritable", error);
        }
        // Whether or not the bus watches the directory (it does not watch
        // one that did not exist when it started), it reads it now.
        match self.bus.reload() {
            Ok(()) => WriteOutcome::Written,
            Err(error) => WriteOutcome::failed("bus.reload_failed", error),
        }
    }
}

/// Writes `content` to `path` through a file beside it and a rename, so the
/// bus never reads half a file.
fn replace(path: &Path, content: &str) -> Result<(), String> {
    use std::io::Write as _;
    let directory = path.parent().ok_or("no directory")?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let temporary = directory.join(format!(
        ".{}.better-os-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let written = std::fs::File::create(&temporary).and_then(|mut file| {
        file.write_all(content.as_bytes())?;
        file.sync_all()
    });
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        error.to_string()
    })?;
    // The rename itself survives a crash only once the directory is synced.
    if let Ok(directory) = std::fs::File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

impl DefaultsAdapter for BusActivationAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::UserService
    }

    fn read(&self, request: &AdapterRequest<'_>) -> ObservedValue {
        collapse(
            request
                .keys()
                .iter()
                .map(|key| KeyObservation {
                    key: key.clone(),
                    observed: self.read_key(request.component.as_str(), key),
                })
                .collect(),
        )
    }

    fn write(&mut self, request: &AdapterRequest<'_>, value: &WriteValue) -> WriteOutcome {
        let mut outcome = WriteOutcome::AlreadyCorrect;
        for key in request.keys() {
            match self.write_key(request.component.as_str(), key, value) {
                WriteOutcome::Written => outcome = WriteOutcome::Written,
                WriteOutcome::AlreadyCorrect => {}
                failed => return failed,
            }
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use better_core::defaults::DefaultIntegration;
    use better_core::manifest::ComponentId;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeBus {
        owner: Arc<Mutex<Option<NameOwner>>>,
        reloads: Arc<Mutex<usize>>,
    }

    impl SessionBus for FakeBus {
        fn owner(&self, _name: &str) -> NameOwner {
            self.owner
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(NameOwner::Nobody)
        }
        fn reload(&self) -> Result<(), String> {
            *self.reloads.lock().unwrap() += 1;
            Ok(())
        }
    }

    /// The declaration `better-files.yaml` makes.
    fn integration() -> DefaultIntegration {
        serde_json::from_value(serde_json::json!({
            "id": "file-manager-service",
            "kind": "user-service",
            "exclusivity": "exclusive",
            "target": {
                "desired": { "type": "boolean", "value": true },
                "keys": ["org.freedesktop.FileManager1"]
            },
            "platforms": ["ubuntu", "zorin"],
            "sessions": ["gnome"],
            "apply_adapter": "user-service",
            "verify_adapter": "user-service",
            "restore_policy": "captured-value",
            "privileges": "user",
            "session_effect": "sign-out"
        }))
        .unwrap()
    }

    #[test]
    fn the_file_is_written_whole_reloaded_and_read_back_and_a_restore_removes_it() {
        let directory = tempfile::tempdir().unwrap();
        let bus = FakeBus::default();
        let mut adapter = BusActivationAdapter::new(
            directory.path().join("dbus-1/services"),
            Box::new(bus.clone()),
            production_activation,
        );
        let component = ComponentId::new("better-files").unwrap();
        let declaration = integration();
        let request = AdapterRequest::new(&component, &declaration);
        assert_eq!(adapter.read(&request), ObservedValue::Unset);
        assert_eq!(adapter.apply(&request), WriteOutcome::Written);
        let file = directory
            .path()
            .join("dbus-1/services/org.freedesktop.FileManager1.service");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/better-files --service\n"
        );
        assert_eq!(*bus.reloads.lock().unwrap(), 1);
        assert_eq!(
            adapter.read(&request).value(),
            Some(&DefaultsValue::Boolean(true))
        );
        // Nautilus still running: the file is ours, the name is not yet.
        *bus.owner.lock().unwrap() = Some(NameOwner::Program("/usr/bin/nautilus".into()));
        assert_eq!(
            adapter.read(&request).value(),
            Some(&DefaultsValue::Text("owned-by:/usr/bin/nautilus".into()))
        );
        assert_eq!(
            adapter.restore(&request, &ObservedValue::Unset),
            WriteOutcome::Written
        );
        assert!(!file.exists());
        assert_eq!(*bus.reloads.lock().unwrap(), 2);
    }

    #[test]
    fn a_file_someone_else_wrote_is_captured_and_put_back_byte_for_byte() {
        let directory = tempfile::tempdir().unwrap();
        let services = directory.path().join("services");
        std::fs::create_dir_all(&services).unwrap();
        let theirs =
            "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/opt/other --dbus\n# kept\n";
        let file = services.join("org.freedesktop.FileManager1.service");
        std::fs::write(&file, theirs).unwrap();
        let mut adapter = BusActivationAdapter::new(
            services,
            Box::new(FakeBus::default()),
            production_activation,
        );
        let component = ComponentId::new("better-files").unwrap();
        let declaration = integration();
        let request = AdapterRequest::new(&component, &declaration);
        let captured = adapter.read(&request);
        assert_eq!(captured.value(), Some(&DefaultsValue::Text(theirs.into())));
        adapter.apply(&request);
        assert_ne!(std::fs::read_to_string(&file).unwrap(), theirs);
        assert_eq!(adapter.restore(&request, &captured), WriteOutcome::Written);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), theirs);
    }

    #[test]
    fn no_other_component_or_name_can_register_a_program() {
        let directory = tempfile::tempdir().unwrap();
        let mut adapter = BusActivationAdapter::new(
            directory.path().to_path_buf(),
            Box::new(FakeBus::default()),
            production_activation,
        );
        let component = ComponentId::new("better-git").unwrap();
        let declaration = integration();
        let request = AdapterRequest::new(&component, &declaration);
        assert!(matches!(
            adapter.read(&request),
            ObservedValue::Unsupported { .. }
        ));
        assert!(matches!(
            adapter.apply(&request),
            WriteOutcome::ManualActionRequired { .. }
        ));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        assert_eq!(exec_for("better-files", "org.example.Evil"), None);
    }

    /// Ticket 129: after an update `/proc/<pid>/exe` ends in ` (deleted)`;
    /// the running Better Files is still Better Files.
    #[test]
    fn a_program_an_update_replaced_is_still_itself() {
        assert_eq!(
            running_program(Path::new("/usr/bin/better-files (deleted)")),
            PathBuf::from("/usr/bin/better-files")
        );
        let directory = tempfile::tempdir().unwrap();
        let bus = FakeBus::default();
        let mut adapter = BusActivationAdapter::new(
            directory.path().join("dbus-1/services"),
            Box::new(bus.clone()),
            production_activation,
        );
        let component = ComponentId::new("better-files").unwrap();
        let declaration = integration();
        let request = AdapterRequest::new(&component, &declaration);
        adapter.apply(&request);
        *bus.owner.lock().unwrap() = Some(NameOwner::Program(running_program(Path::new(
            "/usr/bin/better-files (deleted)",
        ))));
        assert_eq!(
            adapter.read(&request).value(),
            Some(&DefaultsValue::Boolean(true))
        );
    }

    /// Ticket 129: a person's own file comes back as it was, its mode
    /// included; restoring to nothing leaves no empty directories behind.
    #[test]
    fn a_foreign_file_comes_back_with_its_mode_and_nothing_leaves_no_directories() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let services = directory.path().join("dbus-1/services");
        std::fs::create_dir_all(&services).unwrap();
        let file = services.join("org.freedesktop.FileManager1.service");
        let theirs = "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/opt/x\n";
        std::fs::write(&file, theirs).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut adapter = BusActivationAdapter::new(
            services.clone(),
            Box::new(FakeBus::default()),
            production_activation,
        );
        let component = ComponentId::new("better-files").unwrap();
        let declaration = integration();
        let request = AdapterRequest::new(&component, &declaration);
        let captured = adapter.read(&request);
        adapter.apply(&request);
        adapter.restore(&request, &captured);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), theirs);
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(
            std::fs::read_dir(&services).unwrap().count(),
            1,
            "nothing kept aside"
        );

        let empty = tempfile::tempdir().unwrap();
        let services = empty.path().join("dbus-1/services");
        let mut adapter = BusActivationAdapter::new(
            services.clone(),
            Box::new(FakeBus::default()),
            production_activation,
        );
        adapter.apply(&request);
        adapter.restore(&request, &ObservedValue::Unset);
        assert!(
            !empty.path().join("dbus-1").exists(),
            "the directories it made are gone"
        );
    }

    /// Ticket 129: with no record to restore from, a removal takes only a
    /// file that is byte for byte Better OS's.
    #[test]
    fn removal_without_a_record_takes_only_better_os_s_own_file() {
        let directory = tempfile::tempdir().unwrap();
        let services = directory.path().join("dbus-1/services");
        std::fs::create_dir_all(&services).unwrap();
        let file = services.join("org.freedesktop.FileManager1.service");
        std::fs::write(
            &file,
            "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/opt/x\n",
        )
        .unwrap();
        let bus = FakeBus::default();
        assert!(
            remove_own_files(&services, "better-files", production_activation, &bus).is_empty()
        );
        assert!(file.exists(), "the person's own file stays");
        std::fs::write(
            &file,
            service_file(
                "org.freedesktop.FileManager1",
                exec_for("better-files", "org.freedesktop.FileManager1").unwrap(),
            ),
        )
        .unwrap();
        assert_eq!(
            remove_own_files(&services, "better-files", production_activation, &bus),
            ["org.freedesktop.FileManager1"]
        );
        assert!(!file.exists());
        assert_eq!(*bus.reloads.lock().unwrap(), 1);
    }

    /// Ticket 129, second review: the person had their own file, Better OS
    /// kept it aside, and the component goes with no record to restore from
    /// (`apt remove`, then Better Manager's start): the person's file comes
    /// back into use, as it was.
    #[test]
    fn removal_without_a_record_puts_the_person_s_own_file_back() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let services = directory.path().join("dbus-1/services");
        std::fs::create_dir_all(&services).unwrap();
        let file = services.join("org.freedesktop.FileManager1.service");
        let theirs = "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/opt/x\n";
        std::fs::write(&file, theirs).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let bus = FakeBus::default();
        let mut adapter = BusActivationAdapter::new(
            services.clone(),
            Box::new(bus.clone()),
            production_activation,
        );
        let component = ComponentId::new("better-files").unwrap();
        let declaration = integration();
        let request = AdapterRequest::new(&component, &declaration);
        adapter.apply(&request);
        assert_ne!(std::fs::read_to_string(&file).unwrap(), theirs);
        assert_eq!(
            remove_own_files(&services, "better-files", production_activation, &bus),
            ["org.freedesktop.FileManager1"]
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), theirs);
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_dir(&services).unwrap().count(), 1);
    }
}
