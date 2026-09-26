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
        let mut adapters = AdapterSet::in_memory();
        adapters.insert(Box::new(adapter));
        Self {
            catalog: catalog(vec![manifest(
                "better-files",
                vec![shortcut(&["<Super>f"])],
            )]),
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
