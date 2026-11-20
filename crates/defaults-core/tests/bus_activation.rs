//! Better Files becomes what the session bus starts for
//! `org.freedesktop.FileManager1`, and Restore puts Nautilus back (ticket
//! 129, ADR 0041), on a private `dbus-daemon` this test owns.
//!
//! The bus searches a "user" directory before a "system" one, as the
//! session bus searches `$XDG_DATA_HOME/dbus-1/services` before
//! `/usr/share/dbus-1/services`. Nautilus' file is in the system one. The
//! two file managers are this test binary itself, run again by the bus as
//! one of the two helper tests below: each owns the name, writes every call
//! it gets to a log, and stays until the test tells it to go. No file the
//! test wrote is ever executed, nothing reads the person's own bus, and the
//! person's `~/.local/share/dbus-1` is never touched.

mod common;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use better_core::ComponentManifest;
use common::*;
use defaults_core::removal::{
    PutBack, RemovalState, Standing, finish_one, put_back, session_context, take_again,
};
use defaults_core::{
    ComponentReadiness, Confirmations, DefaultsEngine, IntegrationState, Selection, SystemContext,
};
use defaults_platform::AdapterSet;
use defaults_platform::bus_activation::{
    Activation, BusActivationAdapter, ZbusSession, remove_own_files,
};
use defaults_store::SnapshotStore;

const NAME: &str = "org.freedesktop.FileManager1";
const PATH: &str = "/org/freedesktop/FileManager1";
const MANIFEST: &str = include_str!("../../../components/manifests/better-files.yaml");
/// Set in the bus's activation environment: only a helper the bus started
/// does anything.
const HELPER: &str = "BETTER_OS_TEST_BUS_HELPER";
const LOG: &str = "BETTER_OS_TEST_BUS_LOG";

/// How this binary is run by the bus as `helper`. The bus gives an
/// activated program no usable standard output, which the test harness
/// writes to, so it goes to `/dev/null`.
fn exec_line(helper: &str) -> String {
    format!(
        "/bin/sh -c 'exec {} --exact {helper} --test-threads=1 >/dev/null 2>&1'",
        std::env::current_exe().unwrap().display()
    )
}

/// The `Exec=` the adapter writes for Better Files here: the helper that
/// stands in for it.
fn test_exec(component: &str, name: &str) -> Option<Activation> {
    (component == "better-files" && name == NAME).then(|| Activation {
        exec: exec_line("helper_better_files"),
        program: std::env::current_exe().unwrap(),
    })
}

struct PrivateBus {
    child: Child,
    address: String,
    directory: tempfile::TempDir,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let directory = tempfile::tempdir().ok()?;
        let user = directory.path().join("user/dbus-1/services");
        let system = directory.path().join("system/dbus-1/services");
        std::fs::create_dir_all(&user).ok()?;
        std::fs::create_dir_all(&system).ok()?;
        std::fs::write(
            system.join(format!("{NAME}.service")),
            format!(
                "[D-BUS Service]\nName={NAME}\nExec={}\n",
                exec_line("helper_nautilus")
            ),
        )
        .ok()?;
        let config = directory.path().join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:dir={}</listen>\
                 <auth>EXTERNAL</auth><servicedir>{}</servicedir><servicedir>{}</servicedir>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
                directory.path().display(),
                user.display(),
                system.display()
            ),
        )
        .ok()?;
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(directory.path().join("bus.err")).ok()?)
            .spawn()
            .ok()?;
        let mut address = String::new();
        if let Some(stdout) = child.stdout.take() {
            let _ = BufReader::new(stdout).read_line(&mut address);
        }
        let bus = Self {
            child,
            address: address.trim().to_string(),
            directory,
        };
        if bus.address.is_empty() {
            return None;
        }
        let connection = bus.connection();
        let proxy = zbus::blocking::fdo::DBusProxy::new(&connection).ok()?;
        proxy
            .update_activation_environment(std::collections::HashMap::from([
                (HELPER, "1"),
                (LOG, bus.log().to_str()?),
            ]))
            .ok()?;
        Some(bus)
    }

    fn connection(&self) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .unwrap()
    }

    fn user_services(&self) -> PathBuf {
        self.directory.path().join("user/dbus-1/services")
    }

    fn log(&self) -> PathBuf {
        self.directory.path().join("calls.log")
    }

    /// "Show in folder" for one file, as another application sends it; then
    /// which file manager got it.
    fn show_item(&self, uri: &str) -> String {
        let before = self.calls().len();
        self.connection()
            .call_method(
                Some(NAME),
                PATH,
                Some(NAME),
                "ShowItems",
                &(vec![uri.to_string()], String::new()),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "ShowItems was not answered: {error}\n{}",
                    std::fs::read_to_string(self.directory.path().join("bus.err"))
                        .unwrap_or_default()
                )
            });
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let calls = self.calls();
            if calls.len() > before {
                return calls[before].clone();
            }
            assert!(Instant::now() < deadline, "no file manager logged the call");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Tells the helper that owns the name to leave, and waits until the
    /// name is free again.
    fn quit_owner(&self) {
        std::fs::write(self.directory.path().join("quit"), "").unwrap();
        let connection = self.connection();
        let proxy = zbus::blocking::fdo::DBusProxy::new(&connection).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while proxy
            .name_has_owner(NAME.try_into().unwrap())
            .unwrap_or(false)
        {
            assert!(Instant::now() < deadline, "the owner did not leave");
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_file(self.directory.path().join("quit"));
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = std::fs::write(self.directory.path().join("quit"), "");
        std::thread::sleep(Duration::from_millis(100));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Better Defaults over the real declaration and the adapter on this bus.
struct Run {
    catalog: better_core::ComponentCatalog,
    store: SnapshotStore,
    adapters: AdapterSet,
}

impl Run {
    fn new(bus: &PrivateBus, snapshots: &Path) -> Self {
        let manifest = ComponentManifest::parse_yaml(MANIFEST).unwrap();
        let mut adapters = AdapterSet::in_memory();
        adapters.insert(Box::new(BusActivationAdapter::new(
            bus.user_services(),
            Box::new(ZbusSession::at(bus.address.clone())),
            test_exec,
        )));
        Self {
            catalog: catalog(vec![manifest]),
            store: SnapshotStore::at_path(snapshots),
            adapters,
        }
    }

    fn engine(&self) -> DefaultsEngine<'_> {
        DefaultsEngine::new(&self.catalog, SystemContext::new("zorin", "zorin:GNOME"))
            .with_readiness(component("better-files"), ComponentReadiness::ready())
    }

    fn state(&self) -> IntegrationState {
        let history = self.store.history().unwrap();
        let report = self
            .engine()
            .inspect(&Selection::All, &self.adapters, &history);
        report
            .component(&component("better-files"))
            .unwrap()
            .integrations
            .iter()
            .find(|status| status.integration.as_str() == "file-manager-service")
            .unwrap()
            .state
            .clone()
    }

    fn apply(&mut self) {
        let history = self.store.history().unwrap();
        let plan = self.engine().plan_apply(
            &Selection::All,
            &self.adapters,
            &history,
            &Confirmations::none(),
        );
        let catalog = &self.catalog;
        DefaultsEngine::new(catalog, SystemContext::new("zorin", "zorin:GNOME"))
            .with_readiness(component("better-files"), ComponentReadiness::ready())
            .execute(&plan, &mut self.adapters, &self.store)
            .unwrap();
    }

    fn restore(&mut self) {
        let history = self.store.history().unwrap();
        let plan = self.engine().plan_restore(
            &Selection::All,
            &self.adapters,
            &history,
            &Confirmations::none(),
        );
        let catalog = &self.catalog;
        DefaultsEngine::new(catalog, SystemContext::new("zorin", "zorin:GNOME"))
            .with_readiness(component("better-files"), ComponentReadiness::ready())
            .execute(&plan, &mut self.adapters, &self.store)
            .unwrap();
    }
}

/// A private bus, or, where `dbus-daemon` cannot be started, a skip on a
/// developer's machine and a failure in CI (`CI` is set there, and the `rust`
/// job installs the `dbus` package): the routing must be proved somewhere.
macro_rules! bus_or_skip {
    () => {
        match PrivateBus::start() {
            Some(bus) => bus,
            None if std::env::var_os("CI").is_some() => {
                panic!("CI has no dbus-daemon to start a private bus with")
            }
            None => {
                eprintln!("skipped: no dbus-daemon to start a private bus with");
                return;
            }
        }
    };
}

#[test]
fn use_better_os_defaults_makes_show_in_folder_reach_better_files_and_restore_reaches_nautilus() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(&bus, snapshots.path());

    // Before: the system's file starts Nautilus.
    assert_eq!(
        bus.show_item("file:///tmp/a.txt"),
        "nautilus file:///tmp/a.txt"
    );
    bus.quit_owner();
    assert_eq!(run.state(), IntegrationState::NotDefault);

    run.apply();
    assert_eq!(run.state(), IntegrationState::Default);
    assert_eq!(
        bus.show_item("file:///tmp/b.txt"),
        "better-files file:///tmp/b.txt"
    );
    // While it runs, Better Files is still the default.
    assert_eq!(run.state(), IntegrationState::Default);
    bus.quit_owner();

    run.restore();
    assert!(
        !bus.user_services().join(format!("{NAME}.service")).exists(),
        "the activation file is gone"
    );
    assert_eq!(
        bus.show_item("file:///tmp/c.txt"),
        "nautilus file:///tmp/c.txt"
    );
    bus.quit_owner();
}

#[test]
fn a_running_nautilus_keeps_the_name_until_it_leaves_and_is_never_stopped() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(&bus, snapshots.path());

    // Nautilus is running (it answered a call and stays).
    assert_eq!(
        bus.show_item("file:///tmp/a.txt"),
        "nautilus file:///tmp/a.txt"
    );
    run.apply();
    // The file is written; the running owner is left alone and keeps
    // answering.
    assert_eq!(
        std::fs::read_to_string(bus.user_services().join(format!("{NAME}.service"))).unwrap(),
        format!(
            "[D-BUS Service]\nName={NAME}\nExec={}\n",
            exec_line("helper_better_files")
        )
    );
    assert_eq!(
        bus.show_item("file:///tmp/b.txt"),
        "nautilus file:///tmp/b.txt"
    );
    // Once it leaves on its own, the next call starts Better Files.
    bus.quit_owner();
    assert_eq!(
        bus.show_item("file:///tmp/c.txt"),
        "better-files file:///tmp/c.txt"
    );
    bus.quit_owner();
}

#[test]
fn a_foreign_user_file_is_put_back_exactly() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    // Someone else's file, pointing at Nautilus by another path.
    let theirs = format!(
        "[D-BUS Service]\nName={NAME}\nExec={}\n# the person's own\n",
        exec_line("helper_nautilus")
    );
    let file = bus.user_services().join(format!("{NAME}.service"));
    std::fs::write(&file, &theirs).unwrap();
    let mut run = Run::new(&bus, snapshots.path());
    run.apply();
    assert_eq!(
        bus.show_item("file:///tmp/a.txt"),
        "better-files file:///tmp/a.txt"
    );
    bus.quit_owner();
    run.restore();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), theirs);
    assert_eq!(
        bus.show_item("file:///tmp/b.txt"),
        "nautilus file:///tmp/b.txt"
    );
    bus.quit_owner();
}

/// Uninstalling Better Files runs its before-removal program, which is this
/// restore: the activation file goes, and Nautilus answers again.
#[test]
fn removal_takes_the_activation_file_away_and_nautilus_answers_again() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(&bus, snapshots.path());
    run.apply();
    assert_eq!(
        bus.show_item("file:///tmp/a.txt"),
        "better-files file:///tmp/a.txt"
    );
    bus.quit_owner();
    let outcome = defaults_core::removal::restore_component(
        ComponentManifest::parse_yaml(MANIFEST).unwrap(),
        SystemContext::new("zorin", "zorin:GNOME"),
        &mut run.adapters,
        &run.store,
    )
    .unwrap();
    assert!(!outcome.has_failures(), "{outcome:?}");
    assert!(!bus.user_services().join(format!("{NAME}.service")).exists());
    assert_eq!(
        bus.show_item("file:///tmp/b.txt"),
        "nautilus file:///tmp/b.txt"
    );
    bus.quit_owner();
}

fn session() -> Option<SystemContext> {
    Some(SystemContext::new("zorin", "zorin:GNOME"))
}

/// The before-removal program's two halves (ticket 129): a removal the
/// daemon then refused, or Better Files turned off and on again, leaves it
/// the default exactly as before.
#[test]
fn put_back_then_take_again_leaves_better_files_the_default() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let state_dir = tempfile::tempdir().unwrap();
    let state = RemovalState::at(state_dir.path(), "better-files");
    let mut run = Run::new(&bus, snapshots.path());
    run.apply();

    let manifest = || ComponentManifest::parse_yaml(MANIFEST).unwrap();
    assert_eq!(
        put_back(
            manifest(),
            session(),
            &mut run.adapters,
            &run.store,
            &state,
            Vec::new
        ),
        PutBack::Done
    );
    assert_eq!(
        bus.show_item("file:///tmp/a.txt"),
        "nautilus file:///tmp/a.txt"
    );
    bus.quit_owner();

    assert_eq!(
        take_again(manifest(), session(), &mut run.adapters, &run.store, &state),
        PutBack::Done
    );
    assert_eq!(
        bus.show_item("file:///tmp/b.txt"),
        "better-files file:///tmp/b.txt"
    );
    bus.quit_owner();
}

/// No session to put anything back in (a removal run from outside the
/// person's desktop): nothing is touched, the program says so with exit 4,
/// and Better Manager finishes it at its next start.
#[test]
fn a_put_back_with_no_session_is_deferred_not_reported_done() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let state_dir = tempfile::tempdir().unwrap();
    let state = RemovalState::at(state_dir.path(), "better-files");
    let mut run = Run::new(&bus, snapshots.path());
    run.apply();
    let outcome = put_back(
        ComponentManifest::parse_yaml(MANIFEST).unwrap(),
        session_context("zorin".into(), String::new(), true),
        &mut run.adapters,
        &run.store,
        &state,
        Vec::new,
    );
    assert_eq!(outcome, PutBack::Deferred);
    assert!(state.is_pending());
    assert!(bus.user_services().join(format!("{NAME}.service")).exists());
    assert!(session_context("zorin".into(), "zorin:GNOME".into(), false).is_none());
}

/// No record to restore from (the snapshots were lost, or another account
/// applied it): the activation file is removed only when it is byte for
/// byte Better OS's.
/// Second review, finding 1: Better Files turned off with no session (SSH,
/// a TTY, `better-manager-cli disable`) leaves a pending put-back, and
/// Better Manager's next start, seeing the record say "installed, off",
/// finishes it rather than throwing it away. Turned on again first, the
/// pending put-back is overtaken and dropped.
#[test]
fn turned_off_with_no_session_is_put_back_at_the_next_start() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let state_dir = tempfile::tempdir().unwrap();
    let state = RemovalState::at(state_dir.path(), "better-files");
    let mut run = Run::new(&bus, snapshots.path());
    run.apply();
    let manifest = || ComponentManifest::parse_yaml(MANIFEST).unwrap();
    assert_eq!(
        put_back(
            manifest(),
            None,
            &mut run.adapters,
            &run.store,
            &state,
            Vec::new
        ),
        PutBack::Deferred
    );
    assert!(bus.user_services().join(format!("{NAME}.service")).exists());

    // The start: installed, off.
    let off = Standing {
        installed: true,
        enabled: false,
    };
    assert!(finish_one(
        manifest(),
        off,
        session(),
        &mut run.adapters,
        &run.store,
        &state,
        Vec::new
    ));
    assert!(!state.is_pending());
    assert!(!bus.user_services().join(format!("{NAME}.service")).exists());
    assert_eq!(
        bus.show_item("file:///tmp/a.txt"),
        "nautilus file:///tmp/a.txt"
    );
    bus.quit_owner();

    // On again before a start: the pending put-back is dropped, nothing put
    // back.
    let on = Standing {
        installed: true,
        enabled: true,
    };
    run.apply();
    assert_eq!(
        put_back(
            manifest(),
            None,
            &mut run.adapters,
            &run.store,
            &state,
            Vec::new
        ),
        PutBack::Deferred
    );
    finish_one(
        manifest(),
        on,
        session(),
        &mut run.adapters,
        &run.store,
        &state,
        Vec::new,
    );
    assert!(!state.is_pending());
    assert!(bus.user_services().join(format!("{NAME}.service")).exists());
}

/// Second review, finding 2: turned on with no session, what was given back
/// is remembered (and marked pending) until applying it again succeeds.
#[test]
fn turned_on_with_no_session_keeps_what_to_take_again() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let state_dir = tempfile::tempdir().unwrap();
    let state = RemovalState::at(state_dir.path(), "better-files");
    let mut run = Run::new(&bus, snapshots.path());
    run.apply();
    let manifest = || ComponentManifest::parse_yaml(MANIFEST).unwrap();
    assert_eq!(
        put_back(
            manifest(),
            session(),
            &mut run.adapters,
            &run.store,
            &state,
            Vec::new
        ),
        PutBack::Done
    );
    assert_eq!(
        take_again(manifest(), None, &mut run.adapters, &run.store, &state),
        PutBack::Deferred
    );
    assert!(state.is_pending());
    assert!(!bus.user_services().join(format!("{NAME}.service")).exists());
    // The next start in the session, the record saying on: taken again.
    let on = Standing {
        installed: true,
        enabled: true,
    };
    assert!(finish_one(
        manifest(),
        on,
        session(),
        &mut run.adapters,
        &run.store,
        &state,
        Vec::new
    ));
    assert!(!state.is_pending());
    assert_eq!(
        bus.show_item("file:///tmp/b.txt"),
        "better-files file:///tmp/b.txt"
    );
    bus.quit_owner();
}

#[test]
fn with_no_record_only_better_os_s_own_file_is_taken_away() {
    let bus = bus_or_skip!();
    let services = bus.user_services();
    let file = services.join(format!("{NAME}.service"));
    let ours = format!(
        "[D-BUS Service]\nName={NAME}\nExec={}\n",
        exec_line("helper_better_files")
    );
    std::fs::write(&file, &ours).unwrap();
    let session_bus = ZbusSession::at(bus.address.clone());
    let removed = remove_own_files(&services, "better-files", test_exec, &session_bus);
    assert_eq!(removed, [NAME]);
    assert_eq!(
        bus.show_item("file:///tmp/a.txt"),
        "nautilus file:///tmp/a.txt"
    );
    bus.quit_owner();

    std::fs::create_dir_all(&services).unwrap();
    std::fs::write(&file, format!("{ours}# mine\n")).unwrap();
    assert!(remove_own_files(&services, "better-files", test_exec, &session_bus).is_empty());
    assert!(file.exists(), "a file that is not exactly ours stays");
}

/// Ticket 129, review: after "Use Better OS defaults" another program writes
/// its own activation file in Better Files' place. That is a change someone
/// else made, not a change waiting for sign-out, and Restore (and the
/// removal program) leave it alone without the person confirming.
#[test]
fn a_file_another_program_wrote_after_apply_is_never_touched_by_restore() {
    let bus = bus_or_skip!();
    let snapshots = tempfile::tempdir().unwrap();
    let mut run = Run::new(&bus, snapshots.path());
    run.apply();
    let file = bus.user_services().join(format!("{NAME}.service"));
    let theirs = format!(
        "[D-BUS Service]\nName={NAME}\nExec={}\n# another file manager\n",
        exec_line("helper_nautilus")
    );
    std::fs::write(&file, &theirs).unwrap();
    assert!(matches!(
        run.state(),
        IntegrationState::ChangedExternally { .. }
    ));
    run.restore();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), theirs);
}

// --- The two file managers the bus starts --------------------------------

struct Recorder {
    role: &'static str,
    log: PathBuf,
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl Recorder {
    fn show_items(&self, uris: Vec<String>, _startup_id: String) {
        use std::io::Write as _;
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .unwrap();
        for uri in uris {
            writeln!(log, "{} {uri}", self.role).unwrap();
        }
    }
}

/// Owns the name on the bus that started it and records each call until
/// the test leaves a `quit` file beside the log.
fn be_a_file_manager(role: &'static str) {
    if std::env::var_os(HELPER).is_none() {
        return;
    }
    let log = PathBuf::from(std::env::var_os(LOG).unwrap());
    let address = std::env::var("DBUS_STARTER_ADDRESS").unwrap();
    let connection = zbus::blocking::connection::Builder::address(address.as_str())
        .unwrap()
        .serve_at(
            PATH,
            Recorder {
                role,
                log: log.clone(),
            },
        )
        .unwrap()
        .name(NAME)
        .unwrap()
        .build()
        .unwrap();
    let quit = log.with_file_name("quit");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !quit.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(connection);
    std::process::exit(0);
}

#[test]
fn helper_nautilus() {
    be_a_file_manager("nautilus");
}

#[test]
fn helper_better_files() {
    be_a_file_manager("better-files");
}
