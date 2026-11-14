//! The service and a client, against each other over a real session bus.
//!
//! Everything else about the engine is provable without a bus, so what is
//! tested here is only the part that is not: that the interface is actually
//! served, that a JSON document survives the round trip, that a rejection
//! comes back as a document rather than as a bus error, and — the milestone —
//! that a client can connect over the bus, disconnect, and leave collection
//! running behind it.
//!
//! The bus is a private one this test starts and kills, the same shape
//! `awake-tray` and `manager-daemon` use, so the developer's own session is
//! never touched. Where `dbus-daemon` is not installed the test says it is
//! skipping rather than passing quietly.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use monitor_collectors_linux::Roots;
use monitor_ipc::{MonitorRequest, RequestBody};
use monitor_service::{
    AuditSources, ComponentVersions, MonitorClient, MonitorDbusService, MonitorEngine, OBJECT_PATH,
    ServiceConfig, SessionFacts,
};
use monitor_store::RetentionPolicy;

/// A name of our own, because the well-known one belongs to the developer's
/// real Better Monitor when this test runs on a workstation.
const SERVICE_NAME: &str = "org.betteros.Monitor1Test";

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
        let stdout = child.stdout.take()?;
        let mut address = String::new();
        BufReader::new(stdout).read_line(&mut address).ok()?;
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(Self { child, address })
    }

    async fn connect(&self) -> zbus::Result<zbus::Connection> {
        let address: zbus::Address = self.address.parse()?;
        zbus::connection::Builder::address(address)?.build().await
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

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("monitor-collectors-linux")
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn config(store_root: PathBuf) -> ServiceConfig {
    let mut config = ServiceConfig::at(
        store_root,
        Roots::at(fixture("snapshot-a")),
        AuditSources {
            roots: Roots::at(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests")
                    .join("fixtures")
                    .join("machine"),
            ),
            session: SessionFacts::default(),
            components: ComponentVersions::none(),
        },
    );
    config.sample_interval = Duration::from_millis(10);
    config.retention = RetentionPolicy {
        resolution_seconds: 0,
        ..RetentionPolicy::default()
    };
    config
}

/// Serve the engine on the private bus under the test name.
async fn serve(
    bus: &PrivateBus,
    engine: std::sync::Arc<MonitorEngine>,
) -> zbus::Result<zbus::Connection> {
    let address: zbus::Address = bus.address.parse()?;
    zbus::connection::Builder::address(address)?
        .name(SERVICE_NAME)?
        .serve_at(OBJECT_PATH, MonitorDbusService::new(engine))?
        .build()
        .await
}

async fn client(bus: &PrivateBus) -> MonitorClient {
    MonitorClient::with_destination(
        bus.connect().await.expect("a client connection"),
        SERVICE_NAME.to_string(),
    )
    .await
    .expect("a proxy for the served object")
}

#[tokio::test]
async fn a_client_reads_the_service_over_the_session_bus() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    engine.tick().await.unwrap();
    let _served = serve(&bus, engine.clone()).await.expect("a served object");

    let client = client(&bus).await;
    assert_eq!(
        client.protocol_version().await.unwrap(),
        monitor_ipc::PROTOCOL_VERSION
    );

    let status = client.status(false).await.expect("a status document");
    assert!(status.recording);
    assert_eq!(status.rounds_collected, 1);
    assert_eq!(
        status.collectors.len(),
        monitor_collectors_linux::LinuxCollectors::collector_names().len()
    );
}

#[tokio::test]
async fn collection_continues_after_a_bus_client_disconnects() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    let _served = serve(&bus, engine.clone()).await.expect("a served object");
    let sampling = monitor_service::spawn_sampling(engine.clone());

    // A window opens, reads, and closes. The connection is dropped, which is
    // what closing the window actually does to the bus.
    let rounds_while_connected = {
        let client = client(&bus).await;
        let status = client.status(true).await.expect("a status document");
        assert!(status.latest_round.is_some());
        client.rounds_collected().await.expect("the round counter")
    };

    tokio::time::sleep(Duration::from_millis(300)).await;

    // A second window opens much later and finds the machine was observed the
    // whole time it was gone.
    let later = client(&bus).await;
    let status = later.status(false).await.expect("a status document");
    sampling.abort();

    assert!(
        status.rounds_collected > rounds_while_connected,
        "collection stopped with the client: {rounds_while_connected} then {}",
        status.rounds_collected
    );
    assert!(
        status.store.samples > 0,
        "nothing was written while no client was connected"
    );
    let history = later
        .history(0, u64::MAX, 1_000)
        .await
        .expect("a history document");
    assert!(history.slice.samples.len() as u64 >= status.store.samples.min(1));
    assert!(
        history.slice.gaps.is_empty(),
        "the timeline broke while nobody was connected: {:?}",
        history.slice.gaps
    );
}

#[tokio::test]
async fn a_refused_request_comes_back_as_a_document_not_a_bus_error() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    let _served = serve(&bus, engine).await.expect("a served object");

    let client = client(&bus).await;
    let error = client
        .send(MonitorRequest {
            protocol_version: monitor_ipc::PROTOCOL_VERSION,
            body: RequestBody::QueryIncidentWindow { incident_id: 404 },
        })
        .await
        .expect_err("an unknown incident is refused");
    assert!(
        matches!(&error, monitor_service::ClientError::Rejected(key)
            if key.contains("monitor.ipc.error.unknown_incident")),
        "unexpected error: {error}"
    );

    // The service is still answering afterwards, which is the point of a
    // refusal being data rather than a transport failure.
    assert!(client.status(false).await.is_ok());
}

/// Copy a fixture so a test can rewrite its readings.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    for entry in std::fs::read_dir(from).expect("a readable fixture") {
        let entry = entry.expect("a readable entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            std::fs::create_dir_all(&target).expect("a writable directory");
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a copyable file");
        }
    }
}

#[tokio::test]
async fn a_slowdown_appears_without_any_client_asking_and_marking_is_refused() {
    let bus = bus_or_skip!();
    // The captured machine with memory stalled: fake readings, so nothing on
    // this computer has to be put under pressure.
    let machine = tempfile::tempdir().unwrap();
    copy_tree(&fixture("snapshot-a"), machine.path());
    std::fs::write(
        machine.path().join("proc/pressure/memory"),
        "some avg10=58.00 avg60=20.00 avg300=5.00 total=2746439\n\
         full avg10=33.00 avg60=10.00 avg300=2.00 total=2631951\n",
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut config = config(directory.path().to_path_buf());
    config.roots = Roots::at(machine.path());
    // Recognized at the first stalled round rather than after ten seconds,
    // so the test is short; the thresholds are the shipped ones.
    config.slowdowns.onset_seconds = 0;
    let engine = MonitorEngine::start(config).unwrap();
    let _served = serve(&bus, engine.clone()).await.expect("a served object");
    let sampling = monitor_service::spawn_sampling(engine.clone());

    // Nobody asks for anything: the service records it on its own.
    let watcher = client(&bus).await;
    let mut recorded = Vec::new();
    for _ in 0..100 {
        recorded = watcher
            .incidents()
            .await
            .expect("the incident list")
            .incidents;
        if !recorded.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(recorded.len(), 1, "one slowdown, one record");
    let detection = recorded[0].detection.as_ref().expect("a detected incident");
    assert_eq!(
        detection.leading().unwrap().kind,
        monitor_store::SlowdownKind::MemoryPressure
    );
    assert!(!detection.processes.is_empty());

    // A window from before ticket 115 still sends this. It is told no, and
    // nothing is recorded for it.
    let error = watcher
        .send(MonitorRequest::new(RequestBody::MarkIncident {
            note: Some("the system was just slow".into()),
            window_before_seconds: 300,
            window_after_seconds: 120,
            about_pid: None,
        }))
        .await
        .expect_err("marking by hand is refused");
    assert!(
        matches!(&error, monitor_service::ClientError::Rejected(key)
            if key == "monitor.ipc.error.manual_marking_removed"),
        "unexpected error: {error}"
    );
    let after = watcher.incidents().await.expect("the incident list");
    sampling.abort();
    assert_eq!(after.incidents.len(), 1, "the refusal recorded nothing");

    let window = watcher
        .incident_window(recorded[0].id)
        .await
        .expect("the window");
    assert!(!window.incident.snapshot.collectors.is_empty());
}

#[tokio::test]
async fn a_service_waiting_for_a_store_a_window_holds_is_on_the_bus_meanwhile() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    // The window's own engine holds the store.
    let window = monitor_store::HistoryStore::open(&root, RetentionPolicy::default()).unwrap();

    let connection = bus.connect().await.unwrap();
    let starting = tokio::spawn({
        let connection = connection.clone();
        let config = config(root.clone());
        async move {
            monitor_service::start_on_bus(
                &connection,
                SERVICE_NAME,
                config,
                monitor_service::StoreWait {
                    retry: Duration::from_millis(50),
                    attempts: 100,
                },
            )
            .await
            .map_err(|error| error.to_string())
        }
    });

    // The window sees the name taken, the way it watches for it, while the
    // store is still its own.
    let watcher = bus.connect().await.unwrap();
    let dbus = zbus::fdo::DBusProxy::new(&watcher).await.unwrap();
    let name = zbus::names::BusName::try_from(SERVICE_NAME).unwrap();
    let mut owned = false;
    for _ in 0..100 {
        if dbus.name_has_owner(name.clone()).await.unwrap() {
            owned = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        owned,
        "the service took its name while waiting for the store"
    );
    assert!(
        !starting.is_finished(),
        "and is still waiting for the store"
    );

    // The window hands over; the service opens the store and answers.
    drop(window);
    let engine = tokio::time::timeout(Duration::from_secs(5), starting)
        .await
        .expect("the service opened the store once it was free")
        .unwrap()
        .expect("a started engine");
    engine.tick().await.unwrap();
    let client = client(&bus).await;
    assert_eq!(
        client.protocol_version().await.unwrap(),
        monitor_ipc::PROTOCOL_VERSION
    );
}
