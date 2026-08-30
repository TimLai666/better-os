//! The typed client, over a private session bus.
//!
//! Ticket 31 proved the service's surface with a hand-built proxy. This proves
//! the client Better Files actually uses, because a proxy in a test says
//! nothing about the one in production. Same private `dbus-daemon`, same fakes,
//! no real device.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use storage_core::{DeviceStateKind, PERFORMANCE_RISK_KEYS, RemovalPolicy};
use storage_platform::fake::{FakeDeviceControl, FakeFlush, FakeOpenUse, FakeWriteback, usb_stick};
use storage_service::client::{ClientError, StorageClient};
use storage_service::coordinator::Clock;
use storage_service::service::{OBJECT_PATH, StorageService, publish_updates};
use storage_service::{PROTOCOL_VERSION, PreferenceStore, StorageCoordinator};
use tokio::sync::Mutex;

const OBJECT: &str = "/org/freedesktop/UDisks2/block_devices/sdb1";
const TEST_NAME: &str = "org.betteros.Storage1ClientTest";

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

    fn connection_builder(&self) -> zbus::Result<zbus::connection::Builder<'static>> {
        let address: zbus::Address = self.address.parse()?;
        zbus::connection::Builder::address(address)
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Fixture {
    root: std::path::PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "better-os-storage-client-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn serve(bus: &PrivateBus, fixture: &Fixture) -> zbus::Result<zbus::Connection> {
    let control = FakeDeviceControl::new([usb_stick(OBJECT, "/dev/sdb1", "A1B2-C3D4")]);
    let coordinator = StorageCoordinator::new(
        control,
        Arc::new(FakeFlush::default()),
        Arc::new(FakeWriteback::idle()),
        Arc::new(FakeOpenUse::idle()),
        PreferenceStore::at_path(fixture.root.join("storage-preferences.json")),
        Clock::manual(),
    )
    .expect("a coordinator");
    let updates = coordinator.subscribe();
    coordinator.clock().clone().advance(1);
    let coordinator = Arc::new(Mutex::new(coordinator));
    coordinator
        .lock()
        .await
        .refresh_inventory()
        .await
        .expect("an inventory");

    let connection = bus
        .connection_builder()?
        .name(TEST_NAME)?
        .serve_at(OBJECT_PATH, StorageService::new(coordinator.clone()))?
        .build()
        .await?;
    tokio::spawn(publish_updates::<FakeDeviceControl>(
        connection.clone(),
        updates,
    ));
    Ok(connection)
}

async fn client_for(bus: &PrivateBus) -> StorageClient {
    let connection = bus
        .connection_builder()
        .unwrap()
        .build()
        .await
        .expect("a client connection");
    StorageClient::with_destination(connection, TEST_NAME.to_string())
        .await
        .expect("a client")
}

macro_rules! bus_or_skip {
    () => {
        match PrivateBus::start() {
            Some(bus) => bus,
            None => {
                eprintln!("skipping: no dbus-daemon available");
                return;
            }
        }
    };
}

#[tokio::test]
async fn the_client_reads_devices_and_their_states_through_the_service() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("list");
    let _service = serve(&bus, &fixture).await.expect("a served interface");
    let client = client_for(&bus).await;

    assert_eq!(client.protocol_version().await.unwrap(), PROTOCOL_VERSION);
    client.verify().await.expect("a matching protocol version");

    let devices = client.list_devices().await.expect("a device list");
    assert_eq!(devices.len(), 1);
    let device = &devices[0];
    assert_eq!(device.object_path, OBJECT);
    assert_eq!(device.device_path, "/dev/sdb1");
    assert_eq!(device.policy, RemovalPolicy::DirectRemoval);
    // Detected and not mounted. `docs/storage-safety-signals.md` states the
    // rule this exercises: an unmounted volume is ready without a flush,
    // because no filesystem is left owing the device anything and no tracked
    // operation is in flight.
    assert_eq!(device.mount_point, None);
    assert_eq!(device.state.kind(), DeviceStateKind::ReadyToUnplug);
    assert!(device.state.permits_direct_removal());
}

#[tokio::test]
async fn mounting_through_the_client_returns_the_mount_point() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("mount");
    let _service = serve(&bus, &fixture).await.expect("a served interface");
    let client = client_for(&bus).await;

    let mount_point = client.mount(OBJECT).await.expect("a mount point");
    assert!(
        mount_point.starts_with('/'),
        "an absolute mount point, got {mount_point}"
    );
    let devices = client.list_devices().await.unwrap();
    assert_eq!(
        devices[0].mount_point.as_deref(),
        Some(mount_point.as_str())
    );
}

#[tokio::test]
async fn ejecting_reports_what_actually_happened() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("eject");
    let _service = serve(&bus, &fixture).await.expect("a served interface");
    let client = client_for(&bus).await;

    client.mount(OBJECT).await.expect("a mount point");
    let report = client.eject(OBJECT).await.expect("an eject report");
    assert_eq!(report.object_path, OBJECT);
    assert!(report.unmounted);
    assert_eq!(report.protocol_version, PROTOCOL_VERSION);
}

#[tokio::test]
async fn a_policy_change_without_acknowledged_risks_is_refused_not_silently_dropped() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("policy");
    let _service = serve(&bus, &fixture).await.expect("a served interface");
    let client = client_for(&bus).await;

    let refusal = client
        .set_policy(OBJECT, RemovalPolicy::Performance, Vec::new())
        .await
        .expect_err("performance mode needs the risks acknowledged");
    assert!(
        matches!(refusal, ClientError::Rejected(_)),
        "a refusal is a refusal, not a dead service: {refusal:?}"
    );

    client
        .set_policy(
            OBJECT,
            RemovalPolicy::Performance,
            PERFORMANCE_RISK_KEYS
                .iter()
                .map(|key| (*key).to_string())
                .collect(),
        )
        .await
        .expect("acknowledged risks are accepted");
    let devices = client.list_devices().await.unwrap();
    assert_eq!(devices[0].policy, RemovalPolicy::Performance);
    assert_eq!(devices[0].state.kind(), DeviceStateKind::PerformanceMode);
}

#[tokio::test]
async fn an_operation_notice_keeps_the_device_out_of_ready_to_unplug() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("operations");
    let _service = serve(&bus, &fixture).await.expect("a served interface");
    let client = client_for(&bus).await;

    client.mount(OBJECT).await.expect("a mount point");
    client
        .notify_operation_started(OBJECT, "job-1")
        .await
        .expect("the notice is accepted");
    let devices = client.list_devices().await.unwrap();
    assert_eq!(
        devices[0].state.kind(),
        DeviceStateKind::Writing,
        "a tracked operation is a write, whatever the other signals say"
    );

    client
        .notify_operation_completed(OBJECT, "job-1")
        .await
        .expect("the completion is accepted");
    let devices = client.list_devices().await.unwrap();
    assert_eq!(
        devices[0].state.kind(),
        DeviceStateKind::ReadyToUnplug,
        "the flush the completion triggered is what earns the claim"
    );
}

#[tokio::test]
async fn a_client_with_no_service_behind_it_says_so_rather_than_answering() {
    let bus = bus_or_skip!();
    // Nothing is served on this bus at all.
    let connection = bus
        .connection_builder()
        .unwrap()
        .build()
        .await
        .expect("a client connection");
    let client = StorageClient::with_destination(connection, TEST_NAME.to_string())
        .await
        .expect("building a proxy for an absent name succeeds");

    let error = client
        .protocol_version()
        .await
        .expect_err("but calling it does not");
    assert!(
        matches!(error, ClientError::ServiceUnavailable(_)),
        "an absent service is unavailable, not a protocol problem: {error:?}"
    );
    assert!(
        client.list_devices().await.is_err(),
        "and no device list is invented"
    );
}
