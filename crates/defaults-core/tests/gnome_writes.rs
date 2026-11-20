//! Better Defaults changing a GNOME setting itself, end to end.
//!
//! Two kinds of test live here, and neither touches the developer's own
//! settings:
//!
//! - **A fake sender** stands in for the dconf service where the point is what
//!   the engine does when a write fails or does not take. The fake never writes
//!   the database, which is exactly the "the service accepted it and nothing
//!   moved" case.
//! - **A real dconf service on a private bus** is where the point is that a
//!   change is written, read back, and put back. `dconf-service` is started on
//!   a `dbus-daemon` this test owns, with every XDG directory pointed into a
//!   temporary directory, and the bus is configured with no service directories
//!   so nothing on it can be activated with the developer's environment. When
//!   either program is missing, the test says so and does nothing.

mod common;

use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use better_core::defaults::{
    AdapterId, DefaultIntegration, DefaultsValue, IntegrationKind, ObservedValue,
};
use common::*;
use defaults_core::{
    ComponentReadiness, Confirmations, DefaultsEngine, EntryOutcome, IntegrationState, PlanAction,
    Selection, SystemContext,
};
use defaults_platform::dconf::ChangesetSender;
use defaults_platform::dconf_profile::{ProfileEnvironment, session_database_in};
use defaults_platform::dconf_writer::{DconfWriter, USER_WRITER_PATH};
use defaults_platform::gvariant::{ChangeValue, Changeset};
use defaults_platform::{AdapterSet, DconfAdapter};
use defaults_store::SnapshotStore;

const HOME_KEY: &str = "/org/gnome/settings-daemon/plugins/media-keys/home";
const INTEGRATION: &str = "open-file-manager-shortcut";

fn shortcut(desired: &[&str]) -> DefaultIntegration {
    let mut integration = integration(INTEGRATION, HOME_KEY, "unused.desktop");
    integration.kind = IntegrationKind::GlobalShortcut;
    integration.apply_adapter = AdapterId::GnomeKeybinding;
    integration.verify_adapter = AdapterId::GnomeKeybinding;
    integration.target.desired = list(desired);
    integration
}

fn list(values: &[&str]) -> DefaultsValue {
    DefaultsValue::TextList(values.iter().map(|value| value.to_string()).collect())
}

fn keys(values: &[&str]) -> ObservedValue {
    ObservedValue::Set {
        value: list(values),
    }
}

struct Run {
    catalog: better_core::ComponentCatalog,
    store: SnapshotStore,
    adapters: AdapterSet,
}

impl Run {
    fn new(snapshots: &Path, adapter: DconfAdapter) -> Self {
        Self::declaring(snapshots, Box::new(adapter), shortcut(&["<Super>f"]))
    }

    fn declaring(
        snapshots: &Path,
        adapter: Box<dyn defaults_platform::DefaultsAdapter>,
        integration: DefaultIntegration,
    ) -> Self {
        let mut adapters = AdapterSet::in_memory();
        adapters.insert(adapter);
        Self {
            catalog: catalog(vec![manifest("better-files", vec![integration])]),
            store: SnapshotStore::at_path(snapshots),
            adapters,
        }
    }

    fn engine(&self) -> DefaultsEngine<'_> {
        engine(&self.catalog)
    }

    fn state(&self) -> (IntegrationState, ObservedValue) {
        let history = self.store.history().unwrap();
        let report = self
            .engine()
            .inspect(&Selection::All, &self.adapters, &history);
        let status = report
            .component(&component("better-files"))
            .unwrap()
            .integrations[0]
            .clone();
        (status.state, status.current)
    }

    fn apply(&mut self) -> defaults_core::DefaultsOutcome {
        let history = self.store.history().unwrap();
        let plan = self.engine().plan_apply(
            &Selection::All,
            &self.adapters,
            &history,
            &Confirmations::none(),
        );
        engine(&self.catalog)
            .execute(&plan, &mut self.adapters, &self.store)
            .unwrap()
    }

    fn restore(&mut self) -> defaults_core::DefaultsOutcome {
        let history = self.store.history().unwrap();
        let plan = self.engine().plan_restore(
            &Selection::All,
            &self.adapters,
            &history,
            &Confirmations::none(),
        );
        engine(&self.catalog)
            .execute(&plan, &mut self.adapters, &self.store)
            .unwrap()
    }
}

fn engine(catalog: &better_core::ComponentCatalog) -> DefaultsEngine<'_> {
    DefaultsEngine::new(catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready())
}

/// Hands every change set to nobody. Whether it reports success is the test's
/// choice; either way the database does not move.
#[derive(Clone, Default)]
struct FakeSender {
    sent: Rc<RefCell<Vec<Changeset>>>,
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

fn faked(directory: &Path, sender: &FakeSender) -> Run {
    Run::new(
        &directory.join("snapshots"),
        DconfAdapter::with_sender(
            AdapterId::GnomeKeybinding,
            directory.join("config/dconf/user"),
            Box::new(sender.clone()),
        ),
    )
}

#[test]
fn a_write_the_read_does_not_confirm_is_reported_and_records_nothing_as_ours() {
    let directory = tempfile::tempdir().unwrap();
    let sender = FakeSender::default();
    let mut run = faked(directory.path(), &sender);

    let outcome = run.apply();

    assert_eq!(sender.sent.borrow().len(), 1, "the change was not sent");
    assert_eq!(
        outcome.results[0].outcome,
        EntryOutcome::NotVerified {
            observed: ObservedValue::Unset
        }
    );
    assert!(outcome.has_failures());
    // The capture is written before the change; nothing after it claims the
    // value Better Manager asked for.
    assert!(outcome.baseline_snapshot.is_some());
    assert_eq!(outcome.recorded_snapshot, None);
    let history = run.store.history().unwrap();
    let record = history
        .latest_entry(&component("better-files"), &integration_id(INTEGRATION))
        .unwrap();
    assert_eq!(record.previous_value, ObservedValue::Unset);
    assert_eq!(record.applied_value, None);
    assert_eq!(run.state().0, IntegrationState::NotDefault);
}

#[test]
fn a_refused_write_is_a_failure_and_the_setting_is_left_as_it_was() {
    let directory = tempfile::tempdir().unwrap();
    let sender = FakeSender {
        refuse: Some("no session bus is reachable: test".to_string()),
        ..FakeSender::default()
    };
    let mut run = faked(directory.path(), &sender);

    let outcome = run.apply();

    let EntryOutcome::Failed { reason, detail } = &outcome.results[0].outcome else {
        panic!("expected a failure, got {:?}", outcome.results[0].outcome);
    };
    assert_eq!(reason, "dconf.write_failed");
    assert!(
        detail
            .as_deref()
            .unwrap_or_default()
            .contains("no session bus")
    );
    assert_eq!(outcome.recorded_snapshot, None);
    assert!(!directory.path().join("config/dconf/user").exists());
    assert_eq!(
        run.state(),
        (IntegrationState::NotDefault, ObservedValue::Unset)
    );
}

/// A `dbus-daemon` and a `dconf-service` that belong to this test alone.
struct PrivateDconf {
    bus: Child,
    service: Child,
    address: String,
    database: PathBuf,
    _directory: tempfile::TempDir,
}

impl PrivateDconf {
    fn start() -> Option<Self> {
        let service_binary = ["/usr/libexec/dconf-service", "/usr/lib/dconf/dconf-service"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists())?;
        let directory = tempfile::tempdir().ok()?;
        let root = directory.path();
        for name in ["config", "runtime", "home", "cache"] {
            std::fs::create_dir_all(root.join(name)).ok()?;
        }
        // No <servicedir>, so nothing on this bus can be activated — in
        // particular not a dconf-service carrying the developer's environment.
        let config = root.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:tmpdir={}</listen>\
                 <auth>EXTERNAL</auth><policy context=\"default\">\
                 <allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/>\
                 <allow own=\"*\"/></policy></busconfig>\n",
                root.display()
            ),
        )
        .ok()?;
        let path = std::env::var("PATH").unwrap_or_default();
        let isolated = |command: &mut Command| {
            command
                .env_clear()
                .env("PATH", &path)
                .env("HOME", root.join("home"))
                .env("XDG_CONFIG_HOME", root.join("config"))
                .env("XDG_RUNTIME_DIR", root.join("runtime"))
                .env("XDG_CACHE_HOME", root.join("cache"));
        };

        let mut bus_command = Command::new("dbus-daemon");
        bus_command
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        isolated(&mut bus_command);
        let mut bus = bus_command.spawn().ok()?;
        let mut address = String::new();
        BufReader::new(bus.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = bus.kill();
            let _ = bus.wait();
            return None;
        }

        let mut service_command = Command::new(service_binary);
        service_command.stdout(Stdio::null()).stderr(Stdio::null());
        isolated(&mut service_command);
        service_command.env("DBUS_SESSION_BUS_ADDRESS", &address);
        let service = match service_command.spawn() {
            Ok(service) => service,
            Err(_) => {
                let _ = bus.kill();
                let _ = bus.wait();
                return None;
            }
        };
        let dconf = Self {
            bus,
            service,
            address,
            database: root.join("config/dconf/user"),
            _directory: directory,
        };

        // The service owns its name a moment after it starts. Nothing can
        // activate it on this bus, so until then a call simply fails.
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if dconf.writer().probe().is_ok() {
                return Some(dconf);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    fn writer(&self) -> DconfWriter {
        DconfWriter::connect_to(&self.address, USER_WRITER_PATH)
            .expect("the private bus is listening")
    }

    fn adapter(&self) -> DconfAdapter {
        DconfAdapter::with_sender(
            AdapterId::GnomeKeybinding,
            &self.database,
            Box::new(self.writer()),
        )
    }
}

impl Drop for PrivateDconf {
    fn drop(&mut self) {
        let _ = self.service.kill();
        let _ = self.service.wait();
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

macro_rules! dconf_or_skip {
    () => {
        match PrivateDconf::start() {
            Some(dconf) => dconf,
            None => {
                eprintln!(
                    "skipping: dbus-daemon or dconf-service is not available in this environment"
                );
                return;
            }
        }
    };
}

#[test]
fn a_key_that_held_nothing_is_applied_verified_and_reset_on_restore() {
    let dconf = dconf_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(snapshots.path(), dconf.adapter());
    assert_eq!(
        run.state(),
        (IntegrationState::NotDefault, ObservedValue::Unset)
    );

    let applied = run.apply();
    assert_eq!(
        applied.results[0].outcome,
        EntryOutcome::Applied {
            value: list(&["<Super>f"])
        }
    );
    assert!(dconf.database.exists(), "the service wrote no database");
    assert_eq!(
        run.state(),
        (IntegrationState::Default, keys(&["<Super>f"]))
    );

    let history = run.store.history().unwrap();
    let plan = run.engine().plan_restore(
        &Selection::All,
        &run.adapters,
        &history,
        &Confirmations::none(),
    );
    assert_eq!(
        plan.entries[0].action,
        PlanAction::Restore {
            to: ObservedValue::Unset
        }
    );
    let restored = run.restore();
    assert_eq!(
        restored.results[0].outcome,
        EntryOutcome::Restored {
            value: ObservedValue::Unset
        }
    );
    // Reset, not rewritten: the key is gone from the user's database.
    let database = std::fs::read(&dconf.database).unwrap();
    let parsed = defaults_platform::gvdb::GvdbDatabase::parse(&database).unwrap();
    assert!(
        parsed.get(HOME_KEY).is_none(),
        "the key was written, not reset"
    );
    assert_eq!(run.state().1, ObservedValue::Unset);
}

#[test]
fn a_key_the_user_had_set_is_put_back_to_their_value() {
    let dconf = dconf_or_skip!();
    let mut own = Changeset::new("/").unwrap();
    own.set_path(
        HOME_KEY,
        ChangeValue::TextList(vec!["<Super>h".to_string(), "<Alt>h".to_string()]),
    )
    .unwrap();
    dconf.writer().change(&own).unwrap();

    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(snapshots.path(), dconf.adapter());
    assert_eq!(
        run.state(),
        (IntegrationState::NotDefault, keys(&["<Super>h", "<Alt>h"]))
    );

    assert!(run.apply().results[0].outcome.is_success());
    assert_eq!(run.state().1, keys(&["<Super>f"]));

    let restored = run.restore();
    assert_eq!(
        restored.results[0].outcome,
        EntryOutcome::Restored {
            value: keys(&["<Super>h", "<Alt>h"])
        }
    );
    assert_eq!(run.state().1, keys(&["<Super>h", "<Alt>h"]));
}

const FAVORITES: &str = "/org/example/shell/favorite-apps";
const SCHEMAS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../defaults-platform/tests/fixtures/schemas/gschemas.compiled"
);

fn launcher_entry() -> DefaultIntegration {
    let mut integration = integration("overview-entry", FAVORITES, "io.betteros.Files.desktop");
    integration.kind = IntegrationKind::DesktopLauncherEntry;
    integration.apply_adapter = AdapterId::DesktopLauncherEntry;
    integration.verify_adapter = AdapterId::DesktopLauncherEntry;
    integration.exclusivity = better_core::defaults::IntegrationExclusivity::Shared;
    integration.restore_policy = better_core::defaults::RestorePolicy::LeaveInPlace;
    integration
}

/// Better Files pinned to the dock through the real dconf service: the list
/// GNOME was showing (the schema's default, since the user never set it)
/// with Better Files added, verified by reading it back, and left in place
/// by a restore, as the declaration's leave-in-place policy says.
#[test]
fn a_launcher_entry_is_added_to_the_list_gnome_shows_and_left_on_restore() {
    let dconf = dconf_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let adapter = defaults_platform::LauncherEntryAdapter::with_sender(
        &dconf.database,
        vec![PathBuf::from(SCHEMAS)],
        Box::new(dconf.writer()),
    );
    let mut run = Run::declaring(snapshots.path(), Box::new(adapter), launcher_entry());
    assert_eq!(
        run.state(),
        (
            IntegrationState::NotDefault,
            keys(&["org.gnome.Nautilus.desktop", "firefox.desktop"])
        )
    );

    assert!(run.apply().results[0].outcome.is_success());
    assert_eq!(
        run.state(),
        (IntegrationState::Default, set("io.betteros.Files.desktop"))
    );
    let written =
        defaults_platform::gvdb::GvdbDatabase::parse(&std::fs::read(&dconf.database).unwrap())
            .unwrap();
    assert_eq!(
        written.get(FAVORITES),
        Some(&defaults_platform::gvdb::GVariantValue::TextList(vec![
            "org.gnome.Nautilus.desktop".to_string(),
            "firefox.desktop".to_string(),
            "io.betteros.Files.desktop".to_string(),
        ]))
    );

    run.restore();
    assert_eq!(run.state().0, IntegrationState::Default);
}

/// A tool entry point whose key no installed schema defines is unavailable
/// on this desktop, says why, and is never written.
#[test]
fn a_tool_entry_no_schema_defines_is_unavailable_and_never_written() {
    let dconf = dconf_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let adapter = defaults_platform::ToolEntryPointAdapter::with_sender(
        &dconf.database,
        vec![PathBuf::from(SCHEMAS)],
        Box::new(dconf.writer()),
    );
    let mut tool = integration(
        "file-manager-tool-entry-point",
        "/org/gnome/desktop/applications/file-manager",
        "io.betteros.Files.desktop",
    );
    tool.kind = IntegrationKind::ToolEntryPoint;
    tool.apply_adapter = AdapterId::ToolEntryPoint;
    tool.verify_adapter = AdapterId::ToolEntryPoint;
    let mut run = Run::declaring(snapshots.path(), Box::new(adapter), tool);
    let reason = defaults_platform::gsettings::NO_SUCH_KEY.to_string();
    assert_eq!(
        run.state(),
        (
            IntegrationState::Unavailable {
                reason: reason.clone()
            },
            ObservedValue::Unsupported { reason }
        )
    );
    assert!(!run.apply().results[0].outcome.is_success());
    assert!(!dconf.database.exists(), "something was written");
}

// Ticket 207: the database the session's dconf profile writes to, decided by
// `defaults_platform::dconf_profile` over the private service's own scratch
// directories, never the developer's.

impl PrivateDconf {
    /// The scratch directory every XDG variable of the service points into.
    fn root(&self) -> PathBuf {
        self.database
            .ancestors()
            .nth(3)
            .expect("the database is root/config/dconf/user")
            .to_path_buf()
    }

    fn writer_at(&self, object_path: &str) -> DconfWriter {
        DconfWriter::connect_to(&self.address, object_path).expect("the private bus is listening")
    }

    /// The profile environment of a session over this service's directories.
    fn environment(&self, dconf_profile: Option<&str>, desktop: &str) -> ProfileEnvironment {
        let root = self.root();
        ProfileEnvironment {
            dconf_profile: dconf_profile.map(str::to_string),
            desktop: better_core::CurrentDesktop::parse(desktop),
            mandatory_dir: root.join("run/dconf/user"),
            uid: 1000,
            runtime_dir: root.join("runtime"),
            sysconf_dir: root.join("etc"),
            data_dirs: vec![root.join("share")],
            config_home: root.join("config"),
            config_home_on_nfs: false,
        }
    }

    /// The Better OS session's profile, as its package installs it.
    fn install_session_profile(&self) {
        let profile = self.root().join("share/dconf/profile/betteros");
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(profile, "user-db:betteros\nuser-db:user\n").unwrap();
    }

    /// GNOME's own database, given values the way GNOME Settings would.
    fn seed_gnome(&self, key: &str, value: ChangeValue) {
        let mut change = Changeset::new("/").unwrap();
        change.set_path(key, value).unwrap();
        self.writer().change(&change).unwrap();
    }
}

fn read_database(path: &Path) -> defaults_platform::gvdb::GvdbDatabase {
    defaults_platform::gvdb::GvdbDatabase::parse(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn in_the_better_os_session_a_key_is_written_to_its_database_and_gnome_s_is_untouched() {
    let dconf = dconf_or_skip!();
    dconf.seed_gnome(
        HOME_KEY,
        ChangeValue::TextList(vec!["<Super>h".to_string()]),
    );
    let gnome_before = std::fs::read(&dconf.database).unwrap();
    dconf.install_session_profile();

    let session =
        session_database_in(&dconf.environment(Some("betteros"), "BetterOS:COSMIC")).unwrap();
    assert_eq!(session.path, dconf.root().join("config/dconf/betteros"));
    assert_eq!(session.writer_path, "/ca/desrt/dconf/Writer/betteros");
    let writer = dconf.writer_at(&session.writer_path);
    let adapter = DconfAdapter::for_database(
        AdapterId::GnomeKeybinding,
        Ok(session.clone()),
        Some(Box::new(writer)),
    );
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(snapshots.path(), adapter);
    // The session's own database holds nothing yet.
    assert_eq!(
        run.state(),
        (IntegrationState::NotDefault, ObservedValue::Unset)
    );

    let applied = run.apply();
    assert_eq!(
        applied.results[0].outcome,
        EntryOutcome::Applied {
            value: list(&["<Super>f"])
        }
    );
    assert_eq!(
        read_database(&session.path).get(HOME_KEY),
        Some(&defaults_platform::gvdb::GVariantValue::TextList(vec![
            "<Super>f".to_string()
        ]))
    );
    // Read back from the database the session reads first.
    assert_eq!(
        run.state(),
        (IntegrationState::Default, keys(&["<Super>f"]))
    );
    assert_eq!(
        std::fs::read(&dconf.database).unwrap(),
        gnome_before,
        "GNOME's database changed"
    );

    let restored = run.restore();
    assert_eq!(
        restored.results[0].outcome,
        EntryOutcome::Restored {
            value: ObservedValue::Unset
        }
    );
    assert!(read_database(&session.path).get(HOME_KEY).is_none());
    assert_eq!(
        std::fs::read(&dconf.database).unwrap(),
        gnome_before,
        "GNOME's database changed on restore"
    );
}

#[test]
fn in_the_better_os_session_a_launcher_list_starts_from_what_the_session_shows() {
    let dconf = dconf_or_skip!();
    // GNOME's favourites, which the session shows through `user-db:user`
    // while its own database holds none.
    dconf.seed_gnome(
        FAVORITES,
        ChangeValue::TextList(vec!["org.example.Mine.desktop".to_string()]),
    );
    let gnome_before = std::fs::read(&dconf.database).unwrap();
    dconf.install_session_profile();
    let session =
        session_database_in(&dconf.environment(Some("betteros"), "BetterOS:COSMIC")).unwrap();
    let adapter = defaults_platform::LauncherEntryAdapter::for_database(
        Ok(session.clone()),
        vec![PathBuf::from(SCHEMAS)],
        Some(Box::new(dconf.writer_at(&session.writer_path))),
    );
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::declaring(snapshots.path(), Box::new(adapter), launcher_entry());
    assert_eq!(
        run.state(),
        (
            IntegrationState::NotDefault,
            keys(&["org.example.Mine.desktop"])
        )
    );

    assert!(run.apply().results[0].outcome.is_success());
    assert_eq!(
        read_database(&session.path).get(FAVORITES),
        Some(&defaults_platform::gvdb::GVariantValue::TextList(vec![
            "org.example.Mine.desktop".to_string(),
            "io.betteros.Files.desktop".to_string(),
        ]))
    );
    assert_eq!(
        run.state(),
        (IntegrationState::Default, set("io.betteros.Files.desktop"))
    );
    assert_eq!(std::fs::read(&dconf.database).unwrap(), gnome_before);
}

#[test]
fn with_gnome_s_default_profile_the_write_goes_where_it_always_did() {
    let dconf = dconf_or_skip!();
    let session = session_database_in(&dconf.environment(None, "zorin:GNOME")).unwrap();
    assert_eq!(session.path, dconf.database);
    assert_eq!(session.writer_path, USER_WRITER_PATH);
    assert!(session.beneath.is_empty());

    let adapter = DconfAdapter::for_database(
        AdapterId::GnomeKeybinding,
        Ok(session.clone()),
        Some(Box::new(dconf.writer_at(&session.writer_path))),
    );
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(snapshots.path(), adapter);
    assert!(run.apply().results[0].outcome.is_success());
    assert_eq!(
        run.state(),
        (IntegrationState::Default, keys(&["<Super>f"]))
    );
    assert!(dconf.database.exists());
    assert!(!dconf.root().join("config/dconf/betteros").exists());
}

#[test]
fn a_session_without_its_profile_in_the_environment_writes_nothing() {
    let dconf = dconf_or_skip!();
    let decided = session_database_in(&dconf.environment(None, "BetterOS:COSMIC"));
    let adapter = DconfAdapter::for_database(
        AdapterId::GnomeKeybinding,
        decided,
        Some(Box::new(dconf.writer())),
    );
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(snapshots.path(), adapter);
    assert!(!run.apply().results[0].outcome.is_success());
    assert!(!dconf.database.exists(), "GNOME's database was written");
}
