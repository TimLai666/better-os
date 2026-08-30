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
    assert_eq!(status.collectors.len(), 6);
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

#[tokio::test]
async fn an_incident_marked_by_one_client_is_visible_to_the_next() {
    let bus = bus_or_skip!();
    let directory = tempfile::tempdir().unwrap();
    let engine = MonitorEngine::start(config(directory.path().to_path_buf())).unwrap();
    engine.tick().await.unwrap();
    engine.tick().await.unwrap();
    let _served = serve(&bus, engine).await.expect("a served object");

    let marked = {
        let first = client(&bus).await;
        first
            .mark(Some("the system was just slow".into()), 300, 120, None)
            .await
            .expect("an incident")
    };
    assert_eq!(marked.incident.id, 1);
    assert_eq!(
        marked.incident.note.as_deref(),
        Some("the system was just slow")
    );

    let second = client(&bus).await;
    let incidents = second.incidents().await.expect("the incident list");
    assert_eq!(incidents.incidents.len(), 1);
    assert_eq!(incidents.incidents[0].id, 1);

    let window = second.incident_window(1).await.expect("the window");
    assert_eq!(window.incident.id, 1);
    assert!(!window.incident.snapshot.collectors.is_empty());
}
