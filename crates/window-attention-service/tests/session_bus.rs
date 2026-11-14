//! The service against a fake extension on a private session bus: the wire
//! shapes of the signal, the method calls, the launcher mark, and the status
//! method, with nothing near the developer's own session.

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use window_attention_core::bus::{
    ADAPTER_BUS_NAME, ADAPTER_INTERFACE, ADAPTER_OBJECT_PATH, SERVICE_BUS_NAME,
    SERVICE_OBJECT_PATH, WINDOW_ATTENTION_ENDED_SIGNAL, WINDOW_ATTENTION_SIGNAL,
};
use window_attention_core::store::Store;
use window_attention_core::{AppId, Choice, Choices};
use window_attention_service::Relay;
use window_attention_service::bus::{
    LAUNCHER_ENTRY_INTERFACE, ServeError, query_status, serve, serve_until,
};
use zbus::blocking::{Connection, MessageIterator};

struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        use std::io::{BufRead, BufReader};
        let mut address = String::new();
        BufReader::new(child.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(Self { child, address })
    }

    fn connect(&self) -> Connection {
        zbus::blocking::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .unwrap()
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

macro_rules! bus_or_skip {
    () => {
        match PrivateBus::start() {
            Some(bus) => bus,
            None => {
                eprintln!("skipping: dbus-daemon is not available in this environment");
                return;
            }
        }
    };
}

/// The extension's relay, recording what it was asked.
struct FakeAdapter {
    calls: Arc<Mutex<Vec<String>>>,
}

#[zbus::interface(name = "org.betteros.ShellAdapter1.WindowAttention")]
impl FakeAdapter {
    fn suppress_built_in_attention(&self, suppress: bool) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("suppress {suppress}"));
    }
    fn show_attention_banner(&self, window: u64) {
        self.calls.lock().unwrap().push(format!("banner {window}"));
    }
    fn activate_window(&self, window: u64) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("activate {window}"));
    }
    fn clear_attention(&self, window: u64) {
        self.calls.lock().unwrap().push(format!("clear {window}"));
    }
    fn capabilities(&self) -> String {
        "{}".to_string()
    }
}

fn wait_for(calls: &Arc<Mutex<Vec<String>>>, count: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let now = calls.lock().unwrap().clone();
        if now.len() >= count || Instant::now() > deadline {
            return now;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start_adapter(bus: &PrivateBus) -> (Connection, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let connection = bus.connect();
    connection
        .object_server()
        .at(
            ADAPTER_OBJECT_PATH,
            FakeAdapter {
                calls: calls.clone(),
            },
        )
        .unwrap();
    connection.request_name(ADAPTER_BUS_NAME).unwrap();
    (connection, calls)
}

fn emit(adapter: &Connection, window: u64, app_id: &str) {
    adapter
        .emit_signal(
            None::<&str>,
            ADAPTER_OBJECT_PATH,
            ADAPTER_INTERFACE,
            WINDOW_ATTENTION_SIGNAL,
            &(window, app_id, "Claude", 0_u32, false, false),
        )
        .unwrap();
}

fn wait_for_name(connection: &Connection, name: &str) {
    let proxy = zbus::blocking::fdo::DBusProxy::new(connection).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !proxy.name_has_owner(name.try_into().unwrap()).unwrap() {
        assert!(Instant::now() < deadline, "{name} never appeared");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_service_takes_over_answers_each_request_and_marks_the_taskbar() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let mut choices = Choices::default();
    choices.set(
        AppId::new("com.anthropic.Claude.desktop").unwrap(),
        "Claude",
        Choice::BringForward,
    );
    store.save_choices(&choices).unwrap();

    let (adapter, calls) = start_adapter(&bus);
    let watcher = bus.connect();
    let launcher_rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(LAUNCHER_ENTRY_INTERFACE)
        .unwrap()
        .member("Update")
        .unwrap()
        .build();
    let launcher_messages = MessageIterator::for_match_rule(launcher_rule, &watcher, None).unwrap();
    // Read on a thread and waited for with a deadline, so a regression fails
    // the test instead of hanging it.
    let (forward, launcher) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for message in launcher_messages.flatten() {
            if forward.send(message).is_err() {
                break;
            }
        }
    });
    let next_update = || {
        let message = launcher
            .recv_timeout(Duration::from_secs(10))
            .expect("a launcher update within 10 s");
        message
            .body()
            .deserialize::<(
                String,
                std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
            )>()
            .unwrap()
    };

    let service = bus.connect();
    let relay_store = store.clone();
    std::thread::spawn(move || serve(service, Relay::new(relay_store), |_| {}));

    assert_eq!(wait_for(&calls, 1), ["suppress true"]);
    // The service records the takeover once the extension's reply is back.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut status = query_status(&watcher).unwrap();
    while !status.1 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        status = query_status(&watcher).unwrap();
    }
    assert_eq!((status.0, status.1, status.2), (true, true, 1));

    // Claude has its own choice; another application follows the default.
    emit(&adapter, 11, "com.anthropic.Claude.desktop");
    assert_eq!(wait_for(&calls, 2)[1], "activate 11");
    emit(&adapter, 12, "org.gnome.Nautilus.desktop");
    let (uri, properties) = next_update();
    assert_eq!(uri, "application://org.gnome.Nautilus.desktop");
    assert!(bool::try_from(&properties["urgent"]).unwrap());

    // A signal from anyone but the extension is not a request: neither one
    // broadcast, nor one sent straight to the service, which skips the
    // bus's match rules.
    let impostor = bus.connect();
    emit(&impostor, 13, "com.anthropic.Claude.desktop");
    let service_unique = zbus::blocking::fdo::DBusProxy::new(&watcher)
        .unwrap()
        .get_name_owner(SERVICE_BUS_NAME.try_into().unwrap())
        .unwrap()
        .to_string();
    impostor
        .emit_signal(
            Some(service_unique.as_str()),
            ADAPTER_OBJECT_PATH,
            ADAPTER_INTERFACE,
            WINDOW_ATTENTION_SIGNAL,
            &(
                14_u64,
                "com.anthropic.Claude.desktop",
                "Claude",
                0_u32,
                false,
                false,
            ),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let answered = calls.lock().unwrap().clone();
    assert_eq!(answered.len(), 2, "{answered:?}");

    // The window that was marked stops asking: its mark is taken back.
    adapter
        .emit_signal(
            None::<&str>,
            ADAPTER_OBJECT_PATH,
            ADAPTER_INTERFACE,
            WINDOW_ATTENTION_ENDED_SIGNAL,
            &(12_u64, 0_u32),
        )
        .unwrap();
    let (uri, properties) = next_update();
    assert_eq!(uri, "application://org.gnome.Nautilus.desktop");
    assert!(!bool::try_from(&properties["urgent"]).unwrap());

    // A mark the saved choices no longer ask for is taken back when Better
    // Manager says the choices changed.
    emit(&adapter, 15, "org.gnome.Nautilus.desktop");
    let _marked = next_update();
    choices.set(
        AppId::new("org.gnome.Nautilus.desktop").unwrap(),
        "Files",
        Choice::BannerAndMark,
    );
    store.save_choices(&choices).unwrap();
    watcher
        .call_method(
            Some(SERVICE_BUS_NAME),
            SERVICE_OBJECT_PATH,
            Some("org.betteros.WindowAttention1"),
            "ChoicesChanged",
            &(),
        )
        .unwrap();
    let (_, properties) = next_update();
    assert!(!bool::try_from(&properties["urgent"]).unwrap());

    // Both applications are recorded for the page.
    let deadline = Instant::now() + Duration::from_secs(5);
    while store.load_seen().unwrap().apps.len() < 2 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A shell restart drops the extension and brings it back; the service takes
/// over again without being restarted.
#[test]
fn the_service_takes_over_again_when_the_extension_comes_back() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let service = bus.connect();
    let probe = bus.connect();
    std::thread::spawn(move || serve(service, Relay::new(Store::new(directory.path())), |_| {}));
    wait_for_name(&probe, SERVICE_BUS_NAME);
    let status = query_status(&probe).unwrap();
    assert_eq!((status.0, status.1), (false, false));

    let (adapter, calls) = start_adapter(&bus);
    assert_eq!(wait_for(&calls, 1), ["suppress true"]);
    drop(adapter);
    let (_adapter, calls) = start_adapter(&bus);
    assert_eq!(wait_for(&calls, 1), ["suppress true"]);
}

#[test]
fn a_second_instance_leaves_the_first_one_alone() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let first = bus.connect();
    let probe = bus.connect();
    let store = Store::new(directory.path());
    let first_store = store.clone();
    std::thread::spawn(move || serve(first, Relay::new(first_store), |_| {}));
    wait_for_name(&probe, SERVICE_BUS_NAME);
    let second = serve(bus.connect(), Relay::new(store), |_| {});
    assert!(matches!(second, Err(ServeError::AlreadyRunning)));
    // The first still answers.
    assert!(
        probe
            .call_method(
                Some(SERVICE_BUS_NAME),
                SERVICE_OBJECT_PATH,
                Some("org.betteros.WindowAttention1"),
                "Status",
                &(),
            )
            .is_ok()
    );
}

/// Asked to stop, the service takes back every taskbar mark it holds before
/// it ends, so no icon stays marked by a service that is gone.
#[test]
fn stopping_takes_back_every_mark_first() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let (adapter, calls) = start_adapter(&bus);
    let watcher = bus.connect();
    let launcher_rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(LAUNCHER_ENTRY_INTERFACE)
        .unwrap()
        .member("Update")
        .unwrap()
        .build();
    let launcher_messages = MessageIterator::for_match_rule(launcher_rule, &watcher, None).unwrap();
    let (forward, launcher) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for message in launcher_messages.flatten() {
            if forward.send(message).is_err() {
                break;
            }
        }
    });
    let next_urgent = || {
        let message = launcher
            .recv_timeout(Duration::from_secs(10))
            .expect("a launcher update within 10 s");
        let (uri, properties) = message
            .body()
            .deserialize::<(
                String,
                std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
            )>()
            .unwrap();
        (uri, bool::try_from(&properties["urgent"]).unwrap())
    };

    let (stop, stopped) = std::sync::mpsc::channel();
    let service = bus.connect();
    let store = Store::new(directory.path());
    let serving =
        std::thread::spawn(move || serve_until(service, Relay::new(store), |_| {}, stopped));
    assert_eq!(wait_for(&calls, 1), ["suppress true"]);

    // The default is the taskbar mark, so this marks Files.
    emit(&adapter, 21, "org.gnome.Nautilus.desktop");
    assert_eq!(
        next_urgent(),
        ("application://org.gnome.Nautilus.desktop".to_string(), true)
    );

    stop.send(()).unwrap();
    assert_eq!(
        next_urgent(),
        (
            "application://org.gnome.Nautilus.desktop".to_string(),
            false
        )
    );
    assert!(serving.join().unwrap().is_ok());
}
