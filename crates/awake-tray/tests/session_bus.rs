//! Drives the tray and the service against each other over a private session
//! bus, the same shape as `manager-daemon`'s D-Bus test.
//!
//! Nothing here needs a desktop: the inhibitor backend is a fake, the watcher
//! is a fake, and the bus is one this test started and will kill. What is being
//! tested is the part that only exists on a bus — registration verification,
//! the dbusmenu wire surface, and a session outliving the client that started
//! it.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;

use awake_ipc::{AwakeRequest, RequestBody, WireEnd, WireIndicator};
use awake_service::backend::{FakeInhibitorBackend, FixedClock};
use awake_service::{AwakeDbusService, AwakeEngine, OBJECT_PATH};
use awake_store::JsonStore;
use awake_tray::client::ServiceClient;
use awake_tray::controller::TrayController;
use awake_tray::dbusmenu::DbusMenu;
use awake_tray::item::StatusNotifierItem;
use awake_tray::labels::Locale;
use awake_tray::sni::{ITEM_PATH, MENU_PATH, TrayAvailability, register_and_verify};
use serde::Deserialize;
use zbus::zvariant::{OwnedValue, Type};

const SERVICE_NAME: &str = "org.betteros.Awake1Test";
const NOW: u64 = 1_700_000_000;

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

/// A StatusNotifierWatcher that records what it was asked to register and can
/// be told to forget it, which is what a watcher that accepts and then drops an
/// item looks like from the tray's side.
struct FakeWatcher {
    registered: Arc<Mutex<Vec<String>>>,
    remember: bool,
    host_present: bool,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl FakeWatcher {
    async fn register_status_notifier_item(&self, service: &str) {
        if self.remember {
            self.registered.lock().unwrap().push(service.to_string());
        }
    }

    #[zbus(property)]
    async fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registered.lock().unwrap().clone()
    }

    #[zbus(property)]
    async fn is_status_notifier_host_registered(&self) -> bool {
        self.host_present
    }
}

async fn serve_watcher(
    bus: &PrivateBus,
    remember: bool,
    host_present: bool,
) -> zbus::Result<zbus::Connection> {
    let address: zbus::Address = bus.address.parse()?;
    zbus::connection::Builder::address(address)?
        .name("org.kde.StatusNotifierWatcher")?
        .serve_at(
            "/StatusNotifierWatcher",
            FakeWatcher {
                registered: Arc::new(Mutex::new(Vec::new())),
                remember,
                host_present,
            },
        )?
        .build()
        .await
}

struct Service {
    _connection: zbus::Connection,
    _directory: tempfile::TempDir,
    engine: Arc<AwakeEngine<FakeInhibitorBackend>>,
}

async fn serve_service(bus: &PrivateBus) -> zbus::Result<Service> {
    let directory = tempfile::tempdir().unwrap();
    let store = JsonStore::at_path(directory.path().join("state.json"));
    let engine = Arc::new(
        AwakeEngine::start(
            FakeInhibitorBackend::logind_shaped(),
            store,
            Arc::new(FixedClock::at(NOW)),
        )
        .await,
    );

    let address: zbus::Address = bus.address.parse()?;
    let connection = zbus::connection::Builder::address(address)?
        .name(SERVICE_NAME)?
        .serve_at(OBJECT_PATH, AwakeDbusService::new(engine.clone()))?
        .build()
        .await?;

    Ok(Service {
        _connection: connection,
        _directory: directory,
        engine,
    })
}

async fn client(bus: &PrivateBus) -> ServiceClient {
    ServiceClient::with_destination(bus.connect().await.unwrap(), SERVICE_NAME.to_string())
        .await
        .unwrap()
}

fn start(end: WireEnd) -> AwakeRequest {
    AwakeRequest::new(RequestBody::StartSession {
        reason: "Android Studio build is running".to_string(),
        policy: awake_core::SessionPolicy::quick_default(),
        battery_stop_percent: Some(20),
        end,
        security_confirmed: false,
    })
}

#[tokio::test]
async fn the_protocol_version_is_readable_before_anything_is_attempted() {
    let bus = bus_or_skip!();
    let _service = serve_service(&bus).await.unwrap();
    let client = client(&bus).await;

    assert_eq!(
        client.protocol_version().await.unwrap(),
        awake_ipc::PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn a_malformed_request_is_refused_as_an_answer_not_as_a_transport_error() {
    let bus = bus_or_skip!();
    let _service = serve_service(&bus).await.unwrap();
    let connection = bus.connect().await.unwrap();

    let reply = connection
        .call_method(
            Some(SERVICE_NAME),
            OBJECT_PATH,
            Some("org.betteros.Awake1"),
            "Request",
            &("{ not json",),
        )
        .await
        .expect("a bad document must still produce a reply");
    let document: String = reply.body().deserialize().unwrap();
    let response = awake_ipc::AwakeResponse::from_json(&document).unwrap();

    assert!(matches!(
        response.body,
        awake_ipc::ResponseBody::Rejected { .. }
    ));
}

#[tokio::test]
async fn a_session_outlives_the_client_that_started_it() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();

    {
        let first = client(&bus).await;
        let status = first.send(start(WireEnd::Indefinite)).await.unwrap();
        assert_eq!(status.indicator, WireIndicator::ActiveManual);
    }
    // The tray is gone: its connection was dropped with its client.
    assert!(service.engine.holds_inhibitor().await);

    let restarted = client(&bus).await;
    let status = restarted.status().await.unwrap();
    assert_eq!(status.indicator, WireIndicator::ActiveManual);
    assert_eq!(status.sessions.len(), 1);
    assert_eq!(status.sessions[0].reason, "Android Studio build is running");
    assert_eq!(
        service.engine.backend().held_count(),
        1,
        "the inhibitor was never released, because the tray never held it"
    );
}

#[tokio::test]
async fn a_watcher_that_lists_the_item_and_a_host_means_the_icon_is_really_there() {
    let bus = bus_or_skip!();
    let _watcher = serve_watcher(&bus, true, true).await.unwrap();
    let connection = bus.connect().await.unwrap();
    let name = connection.unique_name().unwrap().to_string();

    assert_eq!(
        register_and_verify(&connection, &name).await,
        TrayAvailability::Registered
    );
}

#[tokio::test]
async fn a_watcher_that_forgets_the_item_never_reports_the_icon_as_visible() {
    let bus = bus_or_skip!();
    let _watcher = serve_watcher(&bus, false, true).await.unwrap();
    let connection = bus.connect().await.unwrap();
    let name = connection.unique_name().unwrap().to_string();

    assert_eq!(
        register_and_verify(&connection, &name).await,
        TrayAvailability::NotListed
    );
}

#[tokio::test]
async fn a_watcher_with_no_host_is_registered_but_not_showing() {
    let bus = bus_or_skip!();
    let _watcher = serve_watcher(&bus, true, false).await.unwrap();
    let connection = bus.connect().await.unwrap();
    let name = connection.unique_name().unwrap().to_string();

    let availability = register_and_verify(&connection, &name).await;
    assert_eq!(availability, TrayAvailability::NoHost);
    assert!(!availability.is_visible());
}

#[tokio::test]
async fn no_watcher_at_all_is_reported_rather_than_failing_silently() {
    let bus = bus_or_skip!();
    let connection = bus.connect().await.unwrap();
    let name = connection.unique_name().unwrap().to_string();

    let availability = register_and_verify(&connection, &name).await;
    assert_eq!(availability, TrayAvailability::NoWatcher);
    assert!(availability.remedy_key().is_some());
}

/// `(ia{sv}av)`, read back the way a panel reads it.
#[derive(Debug, Deserialize, Type)]
struct Node(i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

async fn serve_tray(bus: &PrivateBus) -> (zbus::Connection, Arc<TrayController>) {
    let client = client(bus).await;
    let status = client.status().await.unwrap();
    let controller = Arc::new(TrayController::new(client, Locale::EnUs, status));

    let connection = bus.connect().await.unwrap();
    connection
        .object_server()
        .at(ITEM_PATH, StatusNotifierItem::new(controller.clone()))
        .await
        .unwrap();
    connection
        .object_server()
        .at(MENU_PATH, DbusMenu::new(controller.clone()))
        .await
        .unwrap();
    controller.attach(connection.clone()).await;
    (connection, controller)
}

async fn get_layout(connection: &zbus::Connection, destination: &str) -> (u32, Node) {
    let reply = connection
        .call_method(
            Some(destination),
            MENU_PATH,
            Some("com.canonical.dbusmenu"),
            "GetLayout",
            &(0i32, -1i32, Vec::<String>::new()),
        )
        .await
        .unwrap();
    reply.body().deserialize().unwrap()
}

/// Pulls the `label` out of one item's property dictionary, the way a panel
/// does when it draws the entry.
fn label_of(properties: &zbus::zvariant::Value<'_>) -> Option<String> {
    let zbus::zvariant::Value::Dict(properties) = properties else {
        return None;
    };
    properties.iter().find_map(|(name, value)| {
        let name = name.downcast_ref::<&str>().ok()?;
        if name != "label" {
            return None;
        }
        value
            .downcast_ref::<&str>()
            .ok()
            .map(|label| label.to_string())
    })
}

/// Every label in the layout, top level and submenu children alike.
fn labels_of(value: &zbus::zvariant::Value<'_>, labels: &mut Vec<String>) {
    // Children arrive as variants, because the layout's child array is `av`.
    if let zbus::zvariant::Value::Value(inner) = value {
        labels_of(inner, labels);
        return;
    }
    let zbus::zvariant::Value::Structure(item) = value else {
        return;
    };
    let fields = item.fields();
    if let Some(properties) = fields.get(1)
        && let Some(label) = label_of(properties)
    {
        labels.push(label);
    }
    if let Some(zbus::zvariant::Value::Array(children)) = fields.get(2) {
        for child in children.iter() {
            labels_of(child, labels);
        }
    }
}

fn menu_labels(node: &Node) -> Vec<String> {
    let mut labels = Vec::new();
    for child in &node.2 {
        labels_of(child, &mut labels);
    }
    labels
}

#[tokio::test]
async fn a_panel_reading_the_menu_gets_the_inactive_layout() {
    let bus = bus_or_skip!();
    let _service = serve_service(&bus).await.unwrap();
    let (tray, _controller) = serve_tray(&bus).await;
    let tray_name = tray.unique_name().unwrap().to_string();

    let reader = bus.connect().await.unwrap();
    let (revision, layout) = get_layout(&reader, &tray_name).await;

    assert!(revision >= 1);
    assert_eq!(layout.0, 0, "the dbusmenu root is always id 0");
    assert!(
        layout.1.contains_key("children-display"),
        "the root must declare that it has a submenu to draw"
    );
    let labels = menu_labels(&layout);
    assert!(
        labels.contains(&"Start a session".to_string()),
        "{labels:?}"
    );
    assert!(labels.contains(&"Indefinitely".to_string()), "{labels:?}");
    assert!(
        labels.contains(&"Quit Better Awake".to_string()),
        "{labels:?}"
    );
}

#[tokio::test]
async fn clicking_a_preset_starts_a_session_and_clicking_end_ends_it() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    let (tray, controller) = serve_tray(&bus).await;
    let tray_name = tray.unique_name().unwrap().to_string();
    let reader = bus.connect().await.unwrap();

    let two_hours = controller.menu().await.find_by_label("2 hours").unwrap().id;
    reader
        .call_method(
            Some(tray_name.as_str()),
            MENU_PATH,
            Some("com.canonical.dbusmenu"),
            "Event",
            &(
                two_hours,
                "clicked",
                zbus::zvariant::Value::from(0i32),
                0u32,
            ),
        )
        .await
        .unwrap();

    assert!(service.engine.holds_inhibitor().await);
    let status = controller.status().await;
    assert_eq!(status.indicator, WireIndicator::ActiveManual);
    assert_eq!(status.sessions[0].reason, "Started from the tray");

    // The menu has become the active layout, and End session is now in it.
    let end = controller
        .menu()
        .await
        .find_by_label("End session")
        .unwrap()
        .id;
    reader
        .call_method(
            Some(tray_name.as_str()),
            MENU_PATH,
            Some("com.canonical.dbusmenu"),
            "Event",
            &(end, "clicked", zbus::zvariant::Value::from(0i32), 0u32),
        )
        .await
        .unwrap();

    assert!(!service.engine.holds_inhibitor().await);
    assert_eq!(controller.status().await.indicator, WireIndicator::Inactive);
}

#[tokio::test]
async fn a_restarted_tray_shows_the_session_the_service_still_owns() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();

    {
        let (_tray, controller) = serve_tray(&bus).await;
        let indefinitely = controller
            .menu()
            .await
            .find_by_label("Indefinitely")
            .unwrap()
            .id;
        controller.activate(indefinitely).await;
        assert!(service.engine.holds_inhibitor().await);
    }

    // A whole new tray process would do exactly this: connect and ask.
    let (_tray, controller) = serve_tray(&bus).await;
    let labels = controller.menu().await;
    let labels = labels.labels();
    assert!(
        labels.contains(&"Keeping this computer awake"),
        "{labels:?}"
    );
    assert!(service.engine.holds_inhibitor().await);
}

#[tokio::test]
async fn the_item_reports_the_icon_and_tooltip_for_the_state_it_is_in() {
    let bus = bus_or_skip!();
    let _service = serve_service(&bus).await.unwrap();
    let (tray, controller) = serve_tray(&bus).await;
    let tray_name = tray.unique_name().unwrap().to_string();
    let reader = bus.connect().await.unwrap();

    let properties = zbus::fdo::PropertiesProxy::builder(&reader)
        .destination(tray_name.clone())
        .unwrap()
        .path(ITEM_PATH)
        .unwrap()
        .build()
        .await
        .unwrap();
    let interface = || {
        zbus::names::InterfaceName::try_from("org.kde.StatusNotifierItem")
            .expect("a constant interface name")
    };

    let icon = properties.get(interface(), "IconName").await.unwrap();
    assert_eq!(
        String::try_from(icon.try_clone().unwrap()).unwrap(),
        "better-awake-inactive"
    );

    let indefinitely = controller
        .menu()
        .await
        .find_by_label("Indefinitely")
        .unwrap()
        .id;
    controller.activate(indefinitely).await;

    let icon = properties.get(interface(), "IconName").await.unwrap();
    assert_eq!(
        String::try_from(icon.try_clone().unwrap()).unwrap(),
        "better-awake-active"
    );
}
