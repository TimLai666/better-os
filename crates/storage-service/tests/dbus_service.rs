//! Drives the storage service's D-Bus surface over a private session bus.
//!
//! The same shape as `manager-daemon`'s bus test: a private `dbus-daemon`, no
//! real devices behind it, and assertions about the surface itself — what the
//! documents look like, what a refused policy change does, and that a state
//! change reaches a client as a signal.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use storage_core::{DeviceStateKind, PERFORMANCE_RISK_KEYS, RemovalPolicy};
use storage_platform::fake::{FakeDeviceControl, FakeFlush, FakeOpenUse, FakeWriteback, usb_stick};
use storage_service::coordinator::Clock;
use storage_service::protocol::{DeviceListDocument, EjectReport, SetPolicyRequest};
use storage_service::service::{OBJECT_PATH, StorageService, publish_updates};
use storage_service::{PreferenceStore, StorageCoordinator};
use tokio::sync::Mutex;

const OBJECT: &str = "/org/freedesktop/UDisks2/block_devices/sdb1";
const TEST_NAME: &str = "org.betteros.Storage1Test";

/// A private session bus, so the test never touches the developer's own.
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
        let mut reader = BufReader::new(stdout);
        let mut address = String::new();
        reader.read_line(&mut address).ok()?;
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(Self { child, address })
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
            "better-os-storage-dbus-{label}-{}",
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

async fn serve(
    bus: &PrivateBus,
    fixture: &Fixture,
) -> zbus::Result<(
    zbus::Connection,
    Arc<Mutex<StorageCoordinator<FakeDeviceControl>>>,
)> {
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

    let address: zbus::Address = bus.address.parse()?;
    let connection = zbus::connection::Builder::address(address)?
        .name(TEST_NAME)?
        .serve_at(OBJECT_PATH, StorageService::new(coordinator.clone()))?
        .build()
        .await?;
    tokio::spawn(publish_updates::<FakeDeviceControl>(
        connection.clone(),
        updates,
    ));
    Ok((connection, coordinator))
}

async fn client(bus: &PrivateBus) -> zbus::Result<zbus::Connection> {
    let address: zbus::Address = bus.address.parse()?;
    zbus::connection::Builder::address(address)?.build().await
}

async fn call<B, R>(connection: &zbus::Connection, method: &str, body: &B) -> zbus::Result<R>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
{
    let reply = connection
        .call_method(
            Some(TEST_NAME),
            OBJECT_PATH,
            Some("org.betteros.Storage1"),
            method,
            body,
        )
        .await?;
    reply.body().deserialize()
}

/// Skips rather than failing where no session bus binary exists.
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

#[tokio::test]
async fn a_client_lists_devices_and_gets_a_versioned_document() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("list");
    let (_service, _coordinator) = serve(&bus, &fixture).await.unwrap();
    let client = client(&bus).await.unwrap();

    let document: String = call(&client, "ListDevices", &()).await.unwrap();
    let listed = DeviceListDocument::from_json(&document).unwrap();
    assert_eq!(listed.protocol_version, storage_service::PROTOCOL_VERSION);
    assert_eq!(listed.devices.len(), 1);
    assert_eq!(listed.devices[0].policy, RemovalPolicy::DirectRemoval);
}

#[tokio::test]
async fn mounting_over_the_bus_reaches_ready_and_ejecting_reports_what_happened() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("mount");
    let (_service, _coordinator) = serve(&bus, &fixture).await.unwrap();
    let client = client(&bus).await.unwrap();

    let mount_point: String = call(&client, "Mount", &(OBJECT)).await.unwrap();
    assert!(mount_point.starts_with("/run/media/"));

    let document: String = call(&client, "ListDevices", &()).await.unwrap();
    let listed = DeviceListDocument::from_json(&document).unwrap();
    assert_eq!(
        listed.devices[0].state.kind(),
        DeviceStateKind::ReadyToUnplug
    );

    let document: String = call(&client, "Eject", &(OBJECT)).await.unwrap();
    let report = EjectReport::from_json(&document).unwrap();
    assert!(report.unmounted);
    assert!(report.powered_off);
}

#[tokio::test]
async fn a_policy_change_without_every_risk_acknowledged_is_refused_over_the_bus() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("policy");
    let (_service, _coordinator) = serve(&bus, &fixture).await.unwrap();
    let client = client(&bus).await.unwrap();

    let request = SetPolicyRequest::performance(OBJECT, Vec::new())
        .to_json()
        .unwrap();
    let refused: zbus::Result<()> = call(&client, "SetPolicy", &(request.as_str())).await;
    assert!(refused.is_err(), "an unacknowledged opt-in was accepted");

    let request = SetPolicyRequest::performance(
        OBJECT,
        PERFORMANCE_RISK_KEYS
            .iter()
            .map(|key| key.to_string())
            .collect(),
    )
    .to_json()
    .unwrap();
    call::<_, ()>(&client, "SetPolicy", &(request.as_str()))
        .await
        .expect("a complete acknowledgement is accepted");

    let document: String = call(&client, "ListDevices", &()).await.unwrap();
    let listed = DeviceListDocument::from_json(&document).unwrap();
    assert_eq!(listed.devices[0].policy, RemovalPolicy::Performance);
}

#[tokio::test]
async fn a_malformed_document_is_refused_before_it_reaches_the_coordinator() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("malformed");
    let (_service, _coordinator) = serve(&bus, &fixture).await.unwrap();
    let client = client(&bus).await.unwrap();

    let refused: zbus::Result<()> = call(&client, "SetPolicy", &("not a document")).await;
    assert!(refused.is_err());
}

#[tokio::test]
async fn the_service_publishes_state_changes_as_signals() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("signals");
    let (_service, _coordinator) = serve(&bus, &fixture).await.unwrap();
    let client = client(&bus).await.unwrap();

    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.betteros.Storage1")
        .unwrap()
        .member("DeviceStateChanged")
        .unwrap()
        .build();
    let mut stream = zbus::MessageStream::for_match_rule(rule, &client, None)
        .await
        .unwrap();

    let _: String = call(&client, "Mount", &(OBJECT)).await.unwrap();

    use futures_util::StreamExt;
    let signal = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("a signal arrived")
        .expect("a message")
        .expect("a valid message");
    let (object_path, state_json): (String, String) = signal.body().deserialize().unwrap();
    assert_eq!(object_path, OBJECT);
    assert!(state_json.contains("\"object_path\""));
}

#[tokio::test]
async fn the_protocol_version_is_readable_as_a_property() {
    let bus = bus_or_skip!();
    let fixture = Fixture::new("version");
    let (_service, _coordinator) = serve(&bus, &fixture).await.unwrap();
    let client = client(&bus).await.unwrap();

    let properties = zbus::fdo::PropertiesProxy::builder(&client)
        .destination(TEST_NAME)
        .unwrap()
        .path(OBJECT_PATH)
        .unwrap()
        .build()
        .await
        .unwrap();
    let value = properties
        .get(
            "org.betteros.Storage1".try_into().unwrap(),
            "ProtocolVersion",
        )
        .await
        .unwrap();
    let version: u32 = value.try_into().unwrap();
    assert_eq!(version, storage_service::PROTOCOL_VERSION);
}
