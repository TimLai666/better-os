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

use awake_ipc::{
    AwakeEvent, AwakeRequest, EventBody, RequestBody, StatusDocument, WireEnd, WireIndicator,
};
use awake_service::backend::{FakeInhibitorBackend, FixedClock};
use awake_service::notify::LowBatteryNotifier;
use awake_service::{AwakeDbusService, AwakeEngine, OBJECT_PATH};
use awake_tray::client::{LowBatteryStop, ServiceClient};
use awake_tray::controller::TrayController;
use awake_tray::dbusmenu::DbusMenu;
use awake_tray::item::StatusNotifierItem;
use awake_tray::labels::{Locale, LocaleLabels};
use awake_tray::notify::{DesktopNotifier, NotifyError, claim_notifications, handle_event};
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
    connection: zbus::Connection,
    directory: tempfile::TempDir,
    engine: Arc<AwakeEngine<FakeInhibitorBackend>>,
    notifier: LowBatteryNotifier,
    clock: Arc<FixedClock>,
}

impl Service {
    /// Puts a battery at `percent` in the service's own `/sys` tree.
    fn set_battery(&self, percent: u8) {
        let battery = self.directory.path().join("sys/class/power_supply/BAT1");
        std::fs::create_dir_all(&battery).unwrap();
        std::fs::write(battery.join("type"), "Battery\n").unwrap();
        std::fs::write(battery.join("capacity"), format!("{percent}\n")).unwrap();
    }

    /// Plugs the charger in or out in the service's own `/sys` tree.
    fn set_on_ac(&self, online: bool) {
        let supply = self.directory.path().join("sys/class/power_supply/ACAD");
        std::fs::create_dir_all(&supply).unwrap();
        std::fs::write(supply.join("type"), "Mains\n").unwrap();
        std::fs::write(supply.join("online"), if online { "1\n" } else { "0\n" }).unwrap();
    }

    /// What the fake inhibitor holds, as `systemd-inhibit --list` would show it.
    fn held(&self) -> Vec<String> {
        self.engine
            .backend()
            .held_requests()
            .iter()
            .map(|request| request.what_argument())
            .collect()
    }

    /// One tick through the path the service binary runs.
    async fn tick(&self) {
        let emitter =
            zbus::object_server::SignalEmitter::new(&self.connection, OBJECT_PATH).unwrap();
        awake_service::service::tick_and_announce(&self.engine, &emitter, Some(&self.notifier))
            .await;
    }
}

async fn serve_service(bus: &PrivateBus) -> zbus::Result<Service> {
    // Every file the service keeps, and the `/proc` and `/sys` its providers
    // read, are in this directory, so no test touches the developer's own
    // History, rules, or battery.
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(FixedClock::at(NOW));
    let engine = Arc::new(
        AwakeEngine::start_in(
            FakeInhibitorBackend::logind_shaped(),
            directory.path(),
            clock.clone(),
        )
        .await,
    );
    // The service's own notifier is pointed at the fake notification service,
    // like the tray's, so neither can reach the desktop's.
    let notifier = LowBatteryNotifier::new(tokio::runtime::Handle::current(), Locale::EnUs)
        .with_destination(NOTIFICATIONS_NAME.to_string());

    let address: zbus::Address = bus.address.parse()?;
    let connection = zbus::connection::Builder::address(address)?
        .name(SERVICE_NAME)?
        .serve_at(
            OBJECT_PATH,
            AwakeDbusService::new(engine.clone()).with_notifier(notifier.clone()),
        )?
        .build()
        .await?;

    Ok(Service {
        connection,
        directory,
        engine,
        notifier,
        clock,
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
async fn the_test_service_keeps_its_history_in_its_own_directory() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    let client = client(&bus).await;

    client.send(start(WireEnd::Indefinite)).await.unwrap();
    client
        .send(AwakeRequest::new(RequestBody::EndManualSession))
        .await
        .unwrap();

    assert!(
        service
            .directory
            .path()
            .join(awake_store::history::HISTORY_FILE_NAME)
            .exists(),
        "the session this test ran must be recorded in the test's own directory"
    );
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

    // Served from the builder, as `main.rs` does: objects added to a built
    // connection start zbus's object server lazily, and a call arriving before
    // it has subscribed is dropped without a reply.
    let address: zbus::Address = bus.address.parse().unwrap();
    let connection = zbus::connection::Builder::address(address)
        .unwrap()
        .serve_at(ITEM_PATH, StatusNotifierItem::new(controller.clone()))
        .unwrap()
        .serve_at(MENU_PATH, DbusMenu::new(controller.clone()))
        .unwrap()
        .build()
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
        "better-awake-inactive-symbolic"
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
        "better-awake-active-symbolic"
    );
}

// ---- Desktop notifications -----------------------------------------------
//
// The notification service is a fake under a test-only name on the private
// bus. Nothing here can reach the developer's own notification daemon: the
// bus is not theirs, and the name is not one any daemon answers to.

const NOTIFICATIONS_NAME: &str = "org.betteros.NotificationsTest";

/// One `Notify` call as the fake received it.
#[derive(Clone, Debug)]
struct Raised {
    sender: String,
    signature: String,
    app_name: String,
    replaces_id: u32,
    summary: String,
    body: String,
    expire_timeout: i32,
}

#[derive(Clone, Copy)]
enum Behaviour {
    Accept,
    Refuse,
    NeverAnswer,
}

struct FakeNotifications {
    raised: Arc<Mutex<Vec<Raised>>>,
    behaviour: Behaviour,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl FakeNotifications {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        _hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> zbus::fdo::Result<u32> {
        match self.behaviour {
            Behaviour::Accept => {}
            Behaviour::Refuse => {
                return Err(zbus::fdo::Error::Failed(
                    "notifications are off".to_string(),
                ));
            }
            Behaviour::NeverAnswer => std::future::pending::<()>().await,
        }
        let mut raised = self.raised.lock().unwrap();
        raised.push(Raised {
            sender: header
                .sender()
                .map(|sender| sender.to_string())
                .unwrap_or_default(),
            signature: header.signature().to_string_no_parens(),
            app_name,
            replaces_id,
            summary,
            body,
            expire_timeout,
        });
        Ok(raised.len() as u32)
    }
}

async fn serve_notifications(
    bus: &PrivateBus,
    behaviour: Behaviour,
) -> (zbus::Connection, Arc<Mutex<Vec<Raised>>>) {
    let raised = Arc::new(Mutex::new(Vec::new()));
    let address: zbus::Address = bus.address.parse().unwrap();
    let connection = zbus::connection::Builder::address(address)
        .unwrap()
        .name(NOTIFICATIONS_NAME)
        .unwrap()
        .serve_at(
            "/org/freedesktop/Notifications",
            FakeNotifications {
                raised: raised.clone(),
                behaviour,
            },
        )
        .unwrap()
        .build()
        .await
        .unwrap();
    (connection, raised)
}

async fn notifier(bus: &PrivateBus, locale: Locale) -> DesktopNotifier {
    DesktopNotifier::with_destination(
        &bus.connect().await.unwrap(),
        NOTIFICATIONS_NAME.to_string(),
        locale,
    )
    .await
    .unwrap()
}

fn session_ended(
    cause: &str,
    battery_stop_percent: Option<u8>,
    battery_percent: Option<u8>,
) -> String {
    AwakeEvent::new(EventBody::SessionEnded {
        session_id: 7,
        cause: cause.to_string(),
        battery_stop_percent,
        battery_percent,
        lid_limit_minutes: None,
    })
    .to_json()
    .unwrap()
}

fn low_battery_stop() -> String {
    session_ended("battery_threshold", Some(20), Some(19))
}

fn status_event() -> (String, StatusDocument) {
    let status = StatusDocument {
        indicator: WireIndicator::Inactive,
        effective_policy: awake_core::SessionPolicy::default(),
        unmet_policy: Vec::new(),
        battery_stop_percent: None,
        sessions: Vec::new(),
        reasons: Vec::new(),
        backend: awake_ipc::WireBackend {
            name: "logind".to_string(),
            available: true,
            capabilities: awake_core::BackendCapabilities::NONE,
            detail: None,
        },
        attention: None,
        interrupted_previous_session: None,
        reduced_security_confirmed: false,
        active_rules: Vec::new(),
        rule_summary: awake_ipc::WireRuleSummary::default(),
        rules_suppression: None,
        conflicts: Vec::new(),
        providers: Vec::new(),
        battery_protection: awake_ipc::WireBatteryProtection::default(),
        lid: awake_ipc::WireLidProtection::default(),
        now_unix_seconds: NOW,
    };
    let document = AwakeEvent::new(EventBody::StatusChanged(Box::new(status.clone())))
        .to_json()
        .unwrap();
    (document, status)
}

#[tokio::test]
async fn a_low_battery_stop_raises_one_notification_naming_the_threshold_it_crossed() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;

    for locale in [Locale::EnUs, Locale::ZhTw] {
        raised.lock().unwrap().clear();
        let notifier = notifier(&bus, locale).await;

        assert_eq!(
            handle_event(&low_battery_stop(), Some(&notifier)).await,
            None
        );

        let raised = raised.lock().unwrap().clone();
        assert_eq!(
            raised.len(),
            1,
            "{}: one stop, one notification",
            locale.tag()
        );
        let notification = &raised[0];
        assert_eq!(
            notification.signature, "susssasa{sv}i",
            "the call must match the Notify signature in the freedesktop specification"
        );
        assert_eq!(notification.app_name, locale.labels().application_name);
        assert_eq!(notification.replaces_id, 0);
        assert_eq!(notification.expire_timeout, -1);
        assert!(
            notification.summary.contains("20%"),
            "{}: the threshold must be named: {}",
            locale.tag(),
            notification.summary
        );
        assert!(
            notification.body.contains("19%"),
            "{}: the reading must be named: {}",
            locale.tag(),
            notification.body
        );
    }
}

#[tokio::test]
async fn the_notification_is_worded_in_the_trays_own_locale() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;

    handle_event(
        &low_battery_stop(),
        Some(&notifier(&bus, Locale::EnUs).await),
    )
    .await;
    handle_event(
        &low_battery_stop(),
        Some(&notifier(&bus, Locale::ZhTw).await),
    )
    .await;

    let raised = raised.lock().unwrap().clone();
    assert_eq!(raised.len(), 2);
    assert_ne!(raised[0].summary, raised[1].summary);
    assert!(raised[0].summary.is_ascii());
    assert!(
        !raised[1].summary.is_ascii(),
        "zh-TW is not the English wording"
    );
}

#[tokio::test]
async fn a_session_that_ended_for_any_other_reason_raises_no_notification() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let notifier = notifier(&bus, Locale::EnUs).await;

    for cause in [
        "expired",
        "trigger_cleared",
        "rules_suppressed",
        "service_shutdown",
        "user_request",
    ] {
        handle_event(&session_ended(cause, Some(20), None), Some(&notifier)).await;
    }

    assert!(raised.lock().unwrap().is_empty());
}

#[tokio::test]
async fn no_notification_daemon_is_reported_and_the_tray_keeps_following_the_service() {
    let bus = bus_or_skip!();
    // Nothing owns the notification name on this bus.
    let notifier = notifier(&bus, Locale::EnUs).await;

    assert!(matches!(
        notifier
            .low_battery_stop(&LowBatteryStop {
                session_id: 7,
                threshold_percent: 20,
                percent: 19,
            })
            .await,
        Err(NotifyError::Call(_))
    ));

    assert_eq!(
        handle_event(&low_battery_stop(), Some(&notifier)).await,
        None
    );
    let (document, status) = status_event();
    assert_eq!(
        handle_event(&document, Some(&notifier)).await,
        Some(status),
        "a failed notification must not stop the next status from reaching the menu"
    );
}

#[tokio::test]
async fn a_tray_with_no_notifier_at_all_still_follows_the_service() {
    let (document, status) = status_event();
    assert_eq!(handle_event(&low_battery_stop(), None).await, None);
    assert_eq!(handle_event(&document, None).await, Some(status));
}

#[tokio::test]
async fn a_notification_daemon_that_refuses_is_reported_and_the_tray_keeps_going() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Refuse).await;
    let notifier = notifier(&bus, Locale::EnUs).await;

    assert!(matches!(
        notifier
            .low_battery_stop(&LowBatteryStop {
                session_id: 7,
                threshold_percent: 20,
                percent: 19,
            })
            .await,
        Err(NotifyError::Call(_))
    ));
    assert!(raised.lock().unwrap().is_empty());

    let (document, status) = status_event();
    assert_eq!(
        handle_event(&low_battery_stop(), Some(&notifier)).await,
        None
    );
    assert_eq!(handle_event(&document, Some(&notifier)).await, Some(status));
}

#[tokio::test]
async fn a_notification_daemon_that_never_answers_is_given_up_on() {
    let bus = bus_or_skip!();
    let (_daemon, _raised) = serve_notifications(&bus, Behaviour::NeverAnswer).await;
    let notifier = notifier(&bus, Locale::EnUs)
        .await
        .with_timeout(std::time::Duration::from_millis(200));

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        notifier.low_battery_stop(&LowBatteryStop {
            session_id: 7,
            threshold_percent: 20,
            percent: 19,
        }),
    )
    .await
    .expect("the notifier must give up rather than hold the tray's event loop");
    assert_eq!(outcome, Err(NotifyError::TimedOut));
}

// ---- Exactly one notification, whether or not a tray is running -----------

/// A tray's event loop as `main.rs` runs it: subscribe to the service, then
/// claim the notifications, then handle every event. Returns the tray's
/// connection, which owns the tray's name for as long as it is kept.
async fn follow_as_a_tray(bus: &PrivateBus) -> zbus::Connection {
    use zbus::export::futures_core::Stream;

    let connection = bus.connect().await.unwrap();
    let client = ServiceClient::with_destination(connection.clone(), SERVICE_NAME.to_string())
        .await
        .unwrap();
    let stream = client.status_updates().await.unwrap();
    let notifier = DesktopNotifier::with_destination(
        &connection,
        NOTIFICATIONS_NAME.to_string(),
        Locale::EnUs,
    )
    .await
    .unwrap();
    let notifier = claim_notifications(&connection, Some(notifier))
        .await
        .expect("the only tray on the bus gets the name");
    tokio::spawn(async move {
        let mut stream = std::pin::pin!(stream);
        while let Some(signal) =
            std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await
        {
            let Ok(args) = signal.args() else { continue };
            handle_event(args.event_json(), Some(&notifier)).await;
        }
    });
    connection
}

async fn settle(raised: &Arc<Mutex<Vec<Raised>>>, expected: usize) -> Vec<Raised> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while raised.lock().unwrap().len() < expected && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    raised.lock().unwrap().clone()
}

#[tokio::test]
async fn with_a_tray_running_only_the_tray_raises_the_low_battery_notification() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let service = serve_service(&bus).await.unwrap();
    let tray = follow_as_a_tray(&bus).await;

    service.set_battery(80);
    service.engine.handle(start(WireEnd::Indefinite)).await;
    service.set_battery(9);
    service.clock.advance(60);
    service.tick().await;

    let raised = settle(&raised, 1).await;
    assert_eq!(raised.len(), 1, "exactly one notification for one stop");
    assert_eq!(
        raised[0].sender,
        tray.unique_name().unwrap().to_string(),
        "the tray raised it, not the service"
    );
    assert!(raised[0].summary.contains("20%"));
    assert!(raised[0].body.contains("9%"));
}

#[tokio::test]
async fn without_a_tray_the_service_raises_the_low_battery_notification() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let service = serve_service(&bus).await.unwrap();

    service.set_battery(80);
    service.engine.handle(start(WireEnd::Indefinite)).await;
    service.set_battery(9);
    service.clock.advance(60);
    service.tick().await;

    let raised = settle(&raised, 1).await;
    assert_eq!(raised.len(), 1, "exactly one notification for one stop");
    assert_eq!(
        raised[0].sender,
        service.connection.unique_name().unwrap().to_string()
    );
    // Worded the same way the tray words it.
    assert_eq!(raised[0].app_name, Locale::EnUs.labels().application_name);
}

#[tokio::test]
async fn a_second_tray_leaves_the_notifications_to_the_first() {
    let bus = bus_or_skip!();
    let first = bus.connect().await.unwrap();
    let second = bus.connect().await.unwrap();

    assert!(
        claim_notifications(&first, Some(notifier(&bus, Locale::EnUs).await))
            .await
            .is_some()
    );
    assert!(
        claim_notifications(&second, Some(notifier(&bus, Locale::EnUs).await))
            .await
            .is_none(),
        "two trays notifying would be two notifications for one stop"
    );
}

#[tokio::test]
async fn a_tray_with_no_notifier_does_not_claim_the_notifications() {
    let bus = bus_or_skip!();
    let tray = bus.connect().await.unwrap();

    assert!(claim_notifications(&tray, None).await.is_none());
    let bus_proxy = zbus::fdo::DBusProxy::new(&tray).await.unwrap();
    assert!(
        !bus_proxy
            .name_has_owner(awake_ipc::notification::TRAY_BUS_NAME.try_into().unwrap())
            .await
            .unwrap(),
        "a tray that cannot notify must leave the service to do it"
    );
}

// ---- The AppIndicator host, replayed -----------------------------------------
//
// GNOME's AppIndicator extension (`ubuntu/gnome-shell-extension-appindicator`,
// and the `zorin-appindicator@zorinos.com` fork Zorin ships, whose
// `dbusMenu.js` is the same in every part used here) keeps every menu item it
// has seen in a cache keyed by id. `HostMenu` is that cache, call for call:
//
// - a layout read is always `GetLayout(0, -1, ["type", "children-display"])`;
// - an id it has not seen is created from that reply and then filled with
//   `GetGroupProperties([ids], [])`;
// - an id it has seen gets only the `type` and `children-display` that reply
//   carried, and nothing else is read again unless an `ItemsPropertiesUpdated`
//   arrived while the menu was closed (`_doLayoutUpdate`);
// - `LayoutUpdated` and `ItemsPropertiesUpdated` that arrive while the menu is
//   closed are only remembered, and acted on when it next opens;
// - opening the menu sends `Event(0, "opened")` and `AboutToShow(0)`, and
//   re-reads the layout if that answers true; closing it sends
//   `Event(0, "closed")`;
// - unfolding a submenu sends `Event(id, "opened")` and `AboutToShow(id)`,
//   folding it `Event(id, "closed")`;
// - a row it is told `type` or `children-display` for, by a layout read or by
//   `ItemsPropertiesUpdated`, changed or not, is destroyed and drawn again
//   (`_onPropertyChanged` calls `_replaceSelf`); an unfolded submenu that is
//   drawn again is folded, and says nothing about it;
// - a click is `Event(id, "clicked", <i 0>, timestamp)`.
//
// Owner's machine, 2026-11-13: the tray reused ids 1..N for every layout, so
// this cache kept the not-awake labels on the awake layout's ids. These tests
// fail against that tray.

/// Whether the extension rebuilds a row it is sent these properties for.
fn rebuilds(properties: &HashMap<String, OwnedValue>) -> bool {
    properties.contains_key("type") || properties.contains_key("children-display")
}

/// One child of a layout node, which arrives as a variant holding `(ia{sv}av)`.
fn child_node(child: &OwnedValue) -> Node {
    let value: zbus::zvariant::Value<'_> = child.try_clone().unwrap().into();
    let value = match value {
        zbus::zvariant::Value::Value(inner) => *inner,
        other => other,
    };
    let zbus::zvariant::Value::Structure(structure) = value else {
        panic!("a layout child is not a structure");
    };
    let mut fields = structure.into_fields().into_iter();
    let id = i32::try_from(fields.next().unwrap()).unwrap();
    let properties = HashMap::<String, OwnedValue>::try_from(fields.next().unwrap()).unwrap();
    let children = Vec::<OwnedValue>::try_from(fields.next().unwrap()).unwrap();
    Node(id, properties, children)
}

#[derive(Debug, Default)]
struct HostItem {
    properties: HashMap<String, OwnedValue>,
    children: Vec<i32>,
}

/// What the panel draws for one item.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    depth: usize,
    id: i32,
    label: String,
    kind: &'static str,
    enabled: bool,
    children: usize,
}

struct HostMenu {
    connection: zbus::Connection,
    tray: String,
    items: HashMap<i32, HostItem>,
    signals: zbus::MessageStream,
    active: bool,
    layout_update_required: bool,
    items_update_required: bool,
    /// The submenu unfolded now, if any.
    unfolded: Option<i32>,
    /// How many times an unfolded submenu was folded up by a rebuild.
    collapses: usize,
}

fn string_property(properties: &HashMap<String, OwnedValue>, name: &str) -> Option<String> {
    properties
        .get(name)
        .and_then(|value| value.downcast_ref::<&str>().ok().map(str::to_string))
}

impl HostMenu {
    async fn attach(bus: &PrivateBus, tray: &zbus::Connection) -> Self {
        let connection = bus.connect().await.unwrap();
        let tray = tray.unique_name().unwrap().to_string();
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(tray.as_str())
            .unwrap()
            .path(MENU_PATH)
            .unwrap()
            .build();
        let signals = zbus::MessageStream::for_match_rule(rule, &connection, None)
            .await
            .unwrap();
        let mut items = HashMap::new();
        items.insert(0, HostItem::default());
        let mut host = Self {
            connection,
            tray,
            items,
            signals,
            active: false,
            layout_update_required: false,
            items_update_required: false,
            unfolded: None,
            collapses: 0,
        };
        // `initAsync` reads the layout once, closed.
        host.layout_update().await;
        host
    }

    async fn call<B, R>(&self, method: &str, body: &B) -> R
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        self.connection
            .call_method(
                Some(self.tray.as_str()),
                MENU_PATH,
                Some("com.canonical.dbusmenu"),
                method,
                body,
            )
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap()
    }

    async fn layout_update(&mut self) {
        let (_revision, root): (u32, Node) = self
            .call(
                "GetLayout",
                &(
                    0i32,
                    -1i32,
                    vec!["type".to_string(), "children-display".to_string()],
                ),
            )
            .await;
        let mut requested = Vec::new();
        self.merge(&root, &mut requested);
        self.request_properties(requested).await;
        self.collect_garbage();
        self.layout_update_required = false;
        self.items_update_required = false;
    }

    /// `_doLayoutUpdate`.
    fn merge(&mut self, node: &Node, requested: &mut Vec<i32>) {
        let children: Vec<Node> = node.2.iter().map(child_node).collect();
        for child in &children {
            self.merge(child, requested);
        }
        let ids: Vec<i32> = children.iter().map(|child| child.0).collect();
        if self.items.contains_key(&node.0) && rebuilds(&node.1) {
            self.rebuild(node.0);
        }
        match self.items.get_mut(&node.0) {
            Some(item) => {
                for (name, value) in &node.1 {
                    item.properties
                        .insert(name.clone(), value.try_clone().unwrap());
                }
                item.children = ids;
                if !self.items_update_required {
                    return;
                }
            }
            None => {
                let mut properties = HashMap::new();
                for (name, value) in &node.1 {
                    properties.insert(name.clone(), value.try_clone().unwrap());
                }
                self.items.insert(
                    node.0,
                    HostItem {
                        properties,
                        children: ids,
                    },
                );
            }
        }
        requested.push(node.0);
    }

    /// `_requestProperties`: every property, and the item is reset first.
    async fn request_properties(&mut self, ids: Vec<i32>) {
        if ids.is_empty() {
            return;
        }
        let reply: Vec<(i32, HashMap<String, OwnedValue>)> = self
            .call("GetGroupProperties", &(ids, Vec::<String>::new()))
            .await;
        for (id, properties) in reply {
            if let Some(item) = self.items.get_mut(&id) {
                item.properties = properties;
            }
        }
    }

    fn collect_garbage(&mut self) {
        let mut reachable = std::collections::HashSet::new();
        let mut queue = vec![0];
        while let Some(id) = queue.pop() {
            if reachable.insert(id)
                && let Some(item) = self.items.get(&id)
            {
                queue.extend(item.children.iter().copied());
            }
        }
        self.items.retain(|id, _| reachable.contains(id));
        if self.unfolded.is_some_and(|id| !reachable.contains(&id)) {
            self.rebuild(self.unfolded.unwrap());
        }
    }

    /// A row destroyed and drawn again: an unfolded submenu folds up.
    fn rebuild(&mut self, id: i32) {
        if self.unfolded == Some(id) {
            self.unfolded = None;
            self.collapses += 1;
        }
    }

    async fn event(&self, id: i32, event: &str) {
        let _: () = self
            .call(
                "Event",
                &(id, event, zbus::zvariant::Value::from(0i32), 0u32),
            )
            .await;
    }

    /// Unfolds the submenu with this label.
    async fn unfold(&mut self, label: &str) {
        let item = self.find(label);
        assert_eq!(item.kind, "submenu", "{label:?} is not a submenu");
        self.unfold_id(item.id).await;
    }

    async fn unfold_id(&mut self, id: i32) {
        if let Some(open) = self.unfolded.take() {
            self.event(open, "closed").await;
        }
        self.event(id, "opened").await;
        let _: (bool,) = self.call("AboutToShow", &(id,)).await;
        self.unfolded = Some(id);
    }

    /// Folds the unfolded submenu up.
    async fn fold(&mut self) {
        if let Some(open) = self.unfolded.take() {
            self.event(open, "closed").await;
        }
    }

    /// Handles every signal the tray has sent so far, as `_onSignal` does.
    async fn pump(&mut self) {
        use zbus::export::futures_core::Stream;
        loop {
            let next = tokio::time::timeout(
                std::time::Duration::from_millis(150),
                std::future::poll_fn(|context| {
                    std::pin::Pin::new(&mut self.signals).poll_next(context)
                }),
            )
            .await;
            let Ok(Some(Ok(message))) = next else { return };
            let header = message.header();
            let member = header.member().map(|member| member.to_string());
            let root_has_children = self
                .items
                .get(&0)
                .is_some_and(|root| !root.children.is_empty());
            match member.as_deref() {
                Some("LayoutUpdated") => {
                    if !self.active && root_has_children {
                        self.layout_update_required = true;
                    } else {
                        self.layout_update().await;
                    }
                }
                Some("ItemsPropertiesUpdated") => {
                    if !self.active && root_has_children {
                        self.items_update_required = true;
                        continue;
                    }
                    type Updated = Vec<(i32, HashMap<String, OwnedValue>)>;
                    type Removed = Vec<(i32, Vec<String>)>;
                    let (updated, removed): (Updated, Removed) =
                        message.body().deserialize().unwrap();
                    for (id, properties) in updated {
                        if self.items.contains_key(&id) && rebuilds(&properties) {
                            self.rebuild(id);
                        }
                        if let Some(item) = self.items.get_mut(&id) {
                            item.properties.extend(properties);
                        }
                    }
                    for (id, names) in removed {
                        if let Some(item) = self.items.get_mut(&id) {
                            for name in names {
                                item.properties.remove(&name);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// The panel icon clicked: `active` first, then `AboutToShow(0)`.
    async fn open(&mut self) {
        self.pump().await;
        self.active = true;
        if self.layout_update_required {
            self.layout_update().await;
        } else if self.items_update_required {
            let ids: Vec<i32> = self.items.keys().copied().collect();
            self.request_properties(ids).await;
            self.items_update_required = false;
        }
        self.event(0, "opened").await;
        let changed: (bool,) = self.call("AboutToShow", &(0i32,)).await;
        if changed.0 {
            self.layout_update().await;
        }
        self.pump().await;
    }

    async fn close(&mut self) {
        self.fold().await;
        self.active = false;
        self.event(0, "closed").await;
        self.pump().await;
    }

    /// What the panel shows, depth-first.
    fn drawn(&self) -> Vec<Drawn> {
        let mut drawn = Vec::new();
        self.draw(0, 0, &mut drawn);
        drawn
    }

    fn draw(&self, id: i32, depth: usize, drawn: &mut Vec<Drawn>) {
        let Some(item) = self.items.get(&id) else {
            return;
        };
        for child in &item.children {
            let Some(entry) = self.items.get(child) else {
                continue;
            };
            let properties = &entry.properties;
            let kind =
                if string_property(properties, "children-display").as_deref() == Some("submenu") {
                    "submenu"
                } else if string_property(properties, "type").as_deref() == Some("separator") {
                    "separator"
                } else {
                    "item"
                };
            let enabled = properties
                .get("enabled")
                .map(|value| bool::try_from(value).unwrap())
                .unwrap_or(true);
            drawn.push(Drawn {
                depth,
                id: *child,
                label: string_property(properties, "label")
                    .unwrap_or_default()
                    .replacen('_', "", 1),
                kind,
                enabled,
                children: entry.children.len(),
            });
            if kind == "submenu" {
                self.draw(*child, depth + 1, drawn);
            }
        }
    }

    fn find(&self, label: &str) -> Drawn {
        self.drawn()
            .into_iter()
            .find(|item| item.label == label)
            .unwrap_or_else(|| panic!("the panel shows no {label:?}: {:#?}", self.drawn()))
    }

    fn shows(&self, label: &str) -> bool {
        self.drawn().iter().any(|item| item.label == label)
    }

    /// Unfolds a submenu, then clicks an item, the way a person does it.
    async fn click(&mut self, label: &str) {
        let item = self.find(label);
        // The submenu holding it, if any, is unfolded first.
        let drawn = self.drawn();
        if let Some(position) = drawn.iter().position(|entry| entry.id == item.id)
            && item.depth > 0
            && let Some(parent) = drawn[..position]
                .iter()
                .rev()
                .find(|entry| entry.depth == item.depth - 1 && entry.kind == "submenu")
        {
            let parent = parent.id;
            self.unfold_id(parent).await;
        }
        self.event(item.id, "clicked").await;
        // A click closes the menu.
        self.close().await;
    }

    /// Every way the drawn menu can be broken, checked at once.
    fn assert_well_formed(&self) {
        for item in self.drawn() {
            match item.kind {
                "separator" => {}
                "submenu" => {
                    assert!(!item.label.is_empty(), "a submenu with no label: {item:?}");
                    assert!(item.enabled, "a disabled submenu arrow: {item:?}");
                    assert!(item.children > 0, "a submenu that opens nothing: {item:?}");
                }
                _ => assert!(!item.label.is_empty(), "an item with no label: {item:?}"),
            }
        }
    }
}

/// The menu as the tray means it, read with every property. What the host
/// draws must be exactly this.
async fn intended(bus: &PrivateBus, tray: &zbus::Connection) -> Vec<(usize, String)> {
    fn walk(node: &Node, depth: usize, out: &mut Vec<(usize, String)>) {
        for child in &node.2 {
            let child = child_node(child);
            out.push((
                depth,
                string_property(&child.1, "label").unwrap_or_default(),
            ));
            walk(&child, depth + 1, out);
        }
    }
    let reader = bus.connect().await.unwrap();
    let (_, root) = get_layout(&reader, tray.unique_name().unwrap().as_str()).await;
    let mut out = Vec::new();
    walk(&root, 0, &mut out);
    out
}

fn as_drawn_labels(host: &HostMenu) -> Vec<(usize, String)> {
    host.drawn()
        .into_iter()
        .map(|item| (item.depth, item.label))
        .collect()
}

async fn serve_following_tray(bus: &PrivateBus) -> (zbus::Connection, Arc<TrayController>) {
    let (tray, controller) = serve_tray(bus).await;
    tokio::spawn(controller.clone().follow_service(tray.clone(), None));
    // The subscription is in place before anything is asked of the service.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    (tray, controller)
}

#[tokio::test]
async fn the_panel_shows_the_awake_menu_after_a_session_starts_from_it() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    let (tray, _controller) = serve_following_tray(&bus).await;
    let mut host = HostMenu::attach(&bus, &tray).await;

    host.open().await;
    assert!(host.shows("Not keeping this computer awake"));
    host.assert_well_formed();
    host.click("Indefinitely").await;
    assert!(service.engine.holds_inhibitor().await);

    host.open().await;
    assert_eq!(as_drawn_labels(&host), intended(&bus, &tray).await);
    host.assert_well_formed();
    assert!(host.shows("Keeping this computer awake"));
    for stale in [
        "Not keeping this computer awake",
        "Start a session",
        "Until…",
    ] {
        assert!(!host.shows(stale), "the panel still shows {stale:?}");
    }
    // Nothing to extend in a session with no end, so no arrow for it.
    assert!(!host.shows("Extend session"));
    assert!(host.shows("End session"));
}

#[tokio::test]
async fn every_item_of_both_menus_does_what_it_says_through_the_panel() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    let (tray, controller) = serve_following_tray(&bus).await;
    let mut host = HostMenu::attach(&bus, &tray).await;

    // Not awake: a length starts a session of that length.
    host.open().await;
    host.click("30 minutes").await;
    let status = controller.status().await;
    assert_eq!(status.sessions.len(), 1);
    assert_eq!(status.sessions[0].end, WireEnd::Duration { seconds: 1_800 });

    // Awake: a length changes the running session, from now, and never
    // starts a second one or reports an error.
    host.open().await;
    host.assert_well_formed();
    assert_eq!(as_drawn_labels(&host), intended(&bus, &tray).await);
    let change = host.find("Change length");
    assert_eq!(change.kind, "submenu");
    host.click("2 hours").await;
    let status = controller.status().await;
    assert_eq!(status.sessions.len(), 1, "{:?}", status.sessions);
    assert_eq!(status.sessions[0].session_id, 1);
    assert_eq!(
        status.sessions[0].end,
        WireEnd::UntilUnixSeconds {
            unix_seconds: NOW + 7_200
        }
    );

    // A timed session can be extended, and the extension is an open submenu.
    host.open().await;
    host.assert_well_formed();
    let extend = host.find("Extend session");
    assert!(extend.enabled && extend.kind == "submenu" && extend.children == 3);
    host.click("15 min").await;
    assert_eq!(
        controller.status().await.sessions[0].end,
        WireEnd::UntilUnixSeconds {
            unix_seconds: NOW + 7_200 + 900
        }
    );

    // The quick options change what the next session asks for.
    host.open().await;
    host.click("End session").await;
    assert!(!service.engine.holds_inhibitor().await);
    host.open().await;
    assert_eq!(as_drawn_labels(&host), intended(&bus, &tray).await);
    host.assert_well_formed();
    assert!(host.shows("Not keeping this computer awake"));
    host.click("Allow display to turn off").await;
    assert!(!controller.options().await.allow_display_off);
    host.open().await;
    let toggle = host.find("Allow display to turn off");
    assert_eq!(
        host.items[&toggle.id].properties["toggle-state"]
            .downcast_ref::<i32>()
            .unwrap(),
        0,
        "the panel shows the checkmark the tray holds"
    );
    host.close().await;
}

#[tokio::test]
async fn the_open_menu_follows_a_session_started_somewhere_else() {
    let bus = bus_or_skip!();
    let _service = serve_service(&bus).await.unwrap();
    let (tray, _controller) = serve_following_tray(&bus).await;
    let mut host = HostMenu::attach(&bus, &tray).await;

    host.open().await;
    assert!(host.shows("Start a session"));
    // The settings window starts one while the panel menu is open.
    client(&bus)
        .await
        .send(start(WireEnd::Indefinite))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    host.pump().await;
    assert_eq!(as_drawn_labels(&host), intended(&bus, &tray).await);
    assert!(host.shows("Reason: Android Studio build is running"));
    host.assert_well_formed();
}

#[tokio::test]
async fn starting_from_a_stale_menu_changes_the_running_session_instead_of_failing() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    // Not following: this tray still believes nothing is running.
    let (_tray, controller) = serve_tray(&bus).await;
    service.engine.handle(start(WireEnd::Indefinite)).await;
    assert_eq!(controller.indicator().await, WireIndicator::Inactive);

    let fifteen = controller
        .menu()
        .await
        .find_by_label("15 minutes")
        .unwrap()
        .id;
    assert_eq!(
        controller.activate(fifteen).await,
        awake_tray::Activation::Applied
    );
    let status = controller.status().await;
    assert_eq!(status.sessions.len(), 1);
    assert_eq!(
        status.sessions[0].reason, "Android Studio build is running",
        "the running session is changed, not replaced by the tray's"
    );
    assert_eq!(
        status.sessions[0].end,
        WireEnd::UntilUnixSeconds {
            unix_seconds: NOW + 900
        }
    );
}

#[tokio::test]
async fn a_refused_action_is_said_in_a_notification() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let service = serve_service(&bus).await.unwrap();
    let (_tray, controller) = serve_tray(&bus).await;
    controller
        .attach_notifier(notifier(&bus, Locale::EnUs).await)
        .await;

    let indefinitely = controller
        .menu()
        .await
        .find_by_label("Indefinitely")
        .unwrap()
        .id;
    controller.activate(indefinitely).await;
    let end = controller
        .menu()
        .await
        .find_by_label("End session")
        .unwrap()
        .id;
    // Ended elsewhere; this tray does not know.
    service
        .engine
        .handle(AwakeRequest::new(RequestBody::EndManualSession))
        .await;

    assert!(matches!(
        controller.activate(end).await,
        awake_tray::Activation::Failed(_)
    ));
    let raised = settle(&raised, 1).await;
    assert_eq!(raised.len(), 1);
    let labels = Locale::EnUs.labels();
    assert_eq!(raised[0].summary, labels.action_failed);
    assert_eq!(raised[0].body, labels.failed_no_manual_session);
    // And the menu is the truth again.
    assert_eq!(controller.indicator().await, WireIndicator::Inactive);
}

#[tokio::test]
async fn the_tray_follows_the_service_across_a_restart() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    let (_tray, controller) = serve_following_tray(&bus).await;
    service.engine.handle(start(WireEnd::Indefinite)).await;
    controller.refresh().await.unwrap();
    assert_eq!(controller.indicator().await, WireIndicator::ActiveManual);

    // The service stops and a new one takes the name, holding nothing.
    drop(service);
    let _restarted = serve_service(&bus).await.unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while controller.indicator().await != WireIndicator::Inactive
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        controller.indicator().await,
        WireIndicator::Inactive,
        "a new service is read as soon as it is on the bus"
    );

    // And its signals reach the tray: the subscription followed the name.
    client(&bus)
        .await
        .send(start(WireEnd::Indefinite))
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while controller.indicator().await != WireIndicator::ActiveManual
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(controller.indicator().await, WireIndicator::ActiveManual);
}

#[tokio::test]
async fn a_watcher_that_appears_after_the_tray_gets_the_item_registered() {
    let bus = bus_or_skip!();
    let tray = bus.connect().await.unwrap();
    let name = tray.unique_name().unwrap().to_string();
    assert_eq!(
        register_and_verify(&tray, &name).await,
        TrayAvailability::NoWatcher
    );

    let reports = Arc::new(Mutex::new(Vec::new()));
    let sink = reports.clone();
    tokio::spawn(awake_tray::sni::keep_registered(
        tray.clone(),
        name.clone(),
        move |availability| sink.lock().unwrap().push(availability),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let _watcher = serve_watcher(&bus, true, true).await.unwrap();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while reports.lock().unwrap().is_empty() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        reports.lock().unwrap().as_slice(),
        &[TrayAvailability::Registered]
    );
}

// ---- A service that never answers -------------------------------------------

const SILENT_NAME: &str = "org.betteros.Awake1Silent";

/// Owns a name and the interface, and does not reply to a request until the
/// test lets it: what a lost reply or a wedged handler looks like from the
/// tray. The test releases it before the bus goes, so no handler is left
/// parked inside zbus when the connection is torn down.
struct SilentService {
    release: Arc<tokio::sync::Semaphore>,
}

#[zbus::interface(name = "org.betteros.Awake1")]
impl SilentService {
    async fn request(&self, _request_json: String) -> zbus::fdo::Result<String> {
        let _permit = self.release.acquire().await;
        Err(zbus::fdo::Error::Failed("released".to_string()))
    }
}

struct Silent {
    _connection: zbus::Connection,
    release: Arc<tokio::sync::Semaphore>,
}

impl Drop for Silent {
    fn drop(&mut self) {
        self.release
            .add_permits(tokio::sync::Semaphore::MAX_PERMITS / 2);
    }
}

/// A connection whose calls give up after 300 ms, as the tray's give up after
/// `CALL_TIMEOUT`.
async fn impatient(bus: &PrivateBus) -> zbus::Connection {
    let address: zbus::Address = bus.address.parse().unwrap();
    zbus::connection::Builder::address(address)
        .unwrap()
        .method_timeout(std::time::Duration::from_millis(300))
        .build()
        .await
        .unwrap()
}

async fn serve_silent(bus: &PrivateBus) -> Silent {
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let address: zbus::Address = bus.address.parse().unwrap();
    let connection = zbus::connection::Builder::address(address)
        .unwrap()
        .name(SILENT_NAME)
        .unwrap()
        .serve_at(
            OBJECT_PATH,
            SilentService {
                release: release.clone(),
            },
        )
        .unwrap()
        .build()
        .await
        .unwrap();
    Silent {
        _connection: connection,
        release,
    }
}

#[tokio::test]
async fn a_service_that_never_answers_is_reported_unavailable_instead_of_hanging() {
    let bus = bus_or_skip!();
    let _silent = serve_silent(&bus).await;
    let client = ServiceClient::with_destination(impatient(&bus).await, SILENT_NAME.into())
        .await
        .unwrap();

    let started = std::time::Instant::now();
    let reply = client.status().await;
    assert!(
        matches!(reply, Err(awake_tray::ClientError::ServiceUnavailable(_))),
        "{reply:?}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

#[tokio::test]
async fn a_click_the_service_never_answers_fails_and_is_said() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let _service = serve_service(&bus).await.unwrap();
    let status = client(&bus).await.status().await.unwrap();
    let _silent = serve_silent(&bus).await;
    let silent = ServiceClient::with_destination(impatient(&bus).await, SILENT_NAME.into())
        .await
        .unwrap();
    let controller = TrayController::new(silent, Locale::EnUs, status);
    controller
        .attach_notifier(notifier(&bus, Locale::EnUs).await)
        .await;

    let fifteen = controller
        .menu()
        .await
        .find_by_label("15 minutes")
        .unwrap()
        .id;
    let started = std::time::Instant::now();
    assert!(matches!(
        controller.activate(fifteen).await,
        awake_tray::Activation::Failed(awake_tray::ClientError::ServiceUnavailable(_))
    ));
    // The send and the refresh after it each wait out the timeout, no more.
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    let raised = settle(&raised, 1).await;
    assert_eq!(raised.len(), 1);
    assert_eq!(
        raised[0].body,
        Locale::EnUs.labels().failed_service_unavailable
    );
}

// ---- Review follow-ups -------------------------------------------------------

#[tokio::test]
async fn racing_rebuilds_reach_the_host_in_the_order_they_were_made() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    let (tray, controller) = serve_tray(&bus).await;
    let reader = bus.connect().await.unwrap();
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(tray.unique_name().unwrap().as_str())
        .unwrap()
        .member("LayoutUpdated")
        .unwrap()
        .build();
    let mut signals = zbus::MessageStream::for_match_rule(rule, &reader, None)
        .await
        .unwrap();

    let not_awake = client(&bus).await.status().await.unwrap();
    service.engine.handle(start(WireEnd::Indefinite)).await;
    let awake = client(&bus).await.status().await.unwrap();

    for _ in 0..20 {
        tokio::join!(
            controller.set_status(awake.clone()),
            controller.set_status(not_awake.clone()),
        );
    }

    use zbus::export::futures_core::Stream;
    let mut revisions = Vec::new();
    while let Ok(Some(Ok(message))) = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        std::future::poll_fn(|context| std::pin::Pin::new(&mut signals).poll_next(context)),
    )
    .await
    {
        let (revision, _parent): (u32, i32) = message.body().deserialize().unwrap();
        revisions.push(revision);
    }
    assert!(!revisions.is_empty());
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "out of order: {revisions:?}"
    );
    assert_eq!(revisions.last().copied(), Some(controller.revision().await));
}

#[tokio::test]
async fn changing_a_session_that_just_ended_starts_one_instead() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    // Not following, so this tray keeps showing the session after it ends.
    let (_tray, controller) = serve_tray(&bus).await;
    service.engine.handle(start(WireEnd::Indefinite)).await;
    controller.refresh().await.unwrap();
    let ended = controller.status().await.sessions[0].session_id;
    service
        .engine
        .handle(AwakeRequest::new(RequestBody::EndManualSession))
        .await;

    // "30 minutes" under "Change length" in the menu this tray still shows.
    let menu = controller.menu().await;
    let change = menu.find_by_label("Change length").unwrap();
    let thirty = change
        .children
        .iter()
        .find(|item| item.label == "30 minutes")
        .unwrap()
        .id;
    assert_eq!(
        controller.activate(thirty).await,
        awake_tray::Activation::Applied
    );
    let status = controller.status().await;
    assert_eq!(status.sessions.len(), 1);
    assert_ne!(status.sessions[0].session_id, ended, "a new session");
    assert_eq!(status.sessions[0].end, WireEnd::Duration { seconds: 1_800 });
}

#[tokio::test]
async fn a_new_tray_gets_the_name_an_exiting_one_was_still_holding() {
    let bus = bus_or_skip!();
    let old = bus.connect().await.unwrap();
    assert!(
        claim_notifications(&old, Some(notifier(&bus, Locale::EnUs).await))
            .await
            .is_some()
    );

    let new = bus.connect().await.unwrap();
    let notifier = notifier(&bus, Locale::EnUs).await;
    let claim = tokio::spawn(async move {
        awake_tray::notify::claim_notifications_within(
            &new,
            Some(notifier),
            10,
            std::time::Duration::from_millis(100),
        )
        .await
        .is_some()
    });
    // The old tray finishes exiting a moment later.
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    drop(old);
    assert!(claim.await.unwrap(), "the replacing tray takes the name");
}

// ---- Also when the lid is closed (ticket 109) ---------------------------------

async fn click(reader: &zbus::Connection, tray: &str, id: i32) {
    reader
        .call_method(
            Some(tray),
            MENU_PATH,
            Some("com.canonical.dbusmenu"),
            "Event",
            &(id, "clicked", zbus::zvariant::Value::from(0i32), 0u32),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_session_started_from_the_tray_holds_the_lid_unless_the_option_is_unticked() {
    let bus = bus_or_skip!();
    let service = serve_service(&bus).await.unwrap();
    let (tray, controller) = serve_tray(&bus).await;
    let tray_name = tray.unique_name().unwrap().to_string();
    let reader = bus.connect().await.unwrap();

    let two_hours = controller.menu().await.find_by_label("2 hours").unwrap().id;
    click(&reader, &tray_name, two_hours).await;
    assert_eq!(service.held(), vec!["sleep:idle:handle-lid-switch"]);
    assert!(
        controller
            .status()
            .await
            .effective_policy
            .prevent_lid_suspend
    );

    let end = controller
        .menu()
        .await
        .find_by_label("End session")
        .unwrap()
        .id;
    click(&reader, &tray_name, end).await;
    assert!(
        service.held().is_empty(),
        "ending the session releases the lid"
    );

    let lid = controller
        .menu()
        .await
        .find_by_label("Also when the lid is closed")
        .unwrap()
        .id;
    click(&reader, &tray_name, lid).await;
    assert!(!controller.options().await.keep_awake_lid_closed);
    let two_hours = controller.menu().await.find_by_label("2 hours").unwrap().id;
    click(&reader, &tray_name, two_hours).await;
    assert_eq!(service.held(), vec!["sleep:idle"]);
}

#[tokio::test]
async fn with_a_tray_running_the_tray_says_why_the_closed_lid_ended_a_session() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let service = serve_service(&bus).await.unwrap();
    let tray = follow_as_a_tray(&bus).await;

    service.set_on_ac(true);
    service.set_battery(80);
    service
        .engine
        .handle(AwakeRequest::new(RequestBody::StartSession {
            reason: "Download".to_string(),
            policy: awake_core::SessionPolicy::manual_default(),
            battery_stop_percent: Some(20),
            end: WireEnd::Indefinite,
            security_confirmed: false,
        }))
        .await;
    service.engine.backend().set_lid_closed(Some(true));
    service.set_on_ac(false);
    service.clock.advance(60);
    service.tick().await;
    service.clock.advance(2 * 60 * 60);
    service.tick().await;

    let raised = settle(&raised, 1).await;
    assert_eq!(raised.len(), 1, "exactly one notification for one stop");
    assert_eq!(raised[0].sender, tray.unique_name().unwrap().to_string());
    assert_eq!(
        raised[0].summary,
        "Stopped keeping awake: lid closed on battery for 2 hours"
    );
    assert!(service.held().is_empty());
}

// ---- A service from before protocol version 3 --------------------------------

/// A service as 0.2.12 answers: it parses the request first, with
/// `deny_unknown_fields` everywhere, so a version 3 field or request comes back
/// as malformed; only a document it could parse has its version checked. It
/// answers in version 2's shape. The engine behind it is the current one, its
/// answers cut down to that shape.
struct LegacyCore {
    engine: Arc<AwakeEngine<FakeInhibitorBackend>>,
    received: Arc<Mutex<Vec<String>>>,
}

impl LegacyCore {
    async fn request(&self, request_json: &str) -> String {
        self.received.lock().unwrap().push(request_json.to_string());
        let reject = |key: String| {
            awake_ipc::AwakeResponse::rejected(key)
                .to_json_for(awake_ipc::LEGACY_PROTOCOL_VERSION)
                .unwrap()
        };
        for unknown in ["prevent_lid_suspend", "lid_switch", "set_lid_battery_limit"] {
            if request_json.contains(unknown) {
                return reject(format!(
                    "awake.ipc.error.malformed:unknown field `{unknown}`, expected one of"
                ));
            }
        }
        let value: serde_json::Value = serde_json::from_str(request_json).unwrap();
        let version = value["protocol_version"].as_u64().unwrap() as u32;
        if version != awake_ipc::LEGACY_PROTOCOL_VERSION {
            return reject(format!("awake.ipc.error.protocol_version:{version}:2"));
        }
        let request = AwakeRequest::from_json(request_json).unwrap();
        self.engine
            .handle(request)
            .await
            .to_json_for(awake_ipc::LEGACY_PROTOCOL_VERSION)
            .unwrap()
    }
}

/// 0.2.12's interface, with its `ProtocolVersion` property.
struct LegacyService(LegacyCore);

#[zbus::interface(name = "org.betteros.Awake1")]
impl LegacyService {
    async fn request(&self, request_json: &str) -> String {
        self.0.request(request_json).await
    }

    #[zbus(property)]
    async fn protocol_version(&self) -> u32 {
        awake_ipc::LEGACY_PROTOCOL_VERSION
    }
}

/// The same service with the property unreadable, so the client has only the
/// refusal to go on.
struct LegacyServiceWithoutProperty(LegacyCore);

#[zbus::interface(name = "org.betteros.Awake1")]
impl LegacyServiceWithoutProperty {
    async fn request(&self, request_json: &str) -> String {
        self.0.request(request_json).await
    }
}

const LEGACY_SERVICE_NAME: &str = "org.betteros.Awake1LegacyTest";

async fn legacy_engine() -> (tempfile::TempDir, Arc<AwakeEngine<FakeInhibitorBackend>>) {
    let directory = tempfile::tempdir().unwrap();
    let engine = Arc::new(
        AwakeEngine::start_in(
            FakeInhibitorBackend::logind_shaped(),
            directory.path(),
            Arc::new(FixedClock::at(NOW)),
        )
        .await,
    );
    (directory, engine)
}

/// Drives a new tray's client through what a person does against an old
/// service.
async fn drive_against_an_old_service(
    bus: &PrivateBus,
    engine: Arc<AwakeEngine<FakeInhibitorBackend>>,
) {
    let client = ServiceClient::with_destination(
        bus.connect().await.unwrap(),
        LEGACY_SERVICE_NAME.to_string(),
    )
    .await
    .unwrap();

    // A start with the lid option ticked is the first request: the one a
    // service that parses first would choke on.
    let started = client
        .send(awake_tray::client::start_request(
            "從系統匣開始的工作階段",
            WireEnd::Indefinite,
            awake_tray::QuickOptions::default(),
            false,
        ))
        .await
        .unwrap();
    assert!(client.speaks_legacy());
    assert_eq!(started.indicator, WireIndicator::ActiveManual);
    assert!(!started.backend.capabilities.lid_switch);
    assert!(engine.holds_inhibitor().await);

    let menu = awake_tray::build_menu(
        &client.status().await.unwrap(),
        awake_tray::QuickOptions::default(),
        Locale::ZhTw,
        awake_tray::UtcOffset::UTC,
        false,
    );
    let settings = menu.find_by_label("目前的設定").unwrap();
    assert!(
        settings
            .children
            .iter()
            .any(|item| item.label == "闔上螢幕時睡眠: 目前無法使用")
    );

    // Changing the length, as the menu does.
    let session_id = started.sessions[0].session_id;
    client
        .send(AwakeRequest::new(RequestBody::ChangeSession {
            session_id,
            reason: "從系統匣開始的工作階段".to_string(),
            policy: awake_tray::QuickOptions::default().policy(),
            battery_stop_percent: Some(20),
            end: WireEnd::Duration { seconds: 900 },
            security_confirmed: false,
        }))
        .await
        .unwrap();

    assert_eq!(
        client
            .send(AwakeRequest::new(RequestBody::SetLidBatteryLimit {
                limit: awake_core::LidBatteryLimit::Never,
            }))
            .await,
        Err(awake_tray::client::ClientError::Rejected(
            "awake.ipc.error.needs_newer_service".to_string()
        ))
    );

    // After the service restarts as a newer one, the tray asks again.
    client.reset_protocol();
    assert!(!client.speaks_legacy());
}

#[tokio::test]
async fn a_new_tray_reads_the_old_services_version_and_never_sends_it_what_it_cannot_parse() {
    let bus = bus_or_skip!();
    let (_directory, engine) = legacy_engine().await;
    let received = Arc::new(Mutex::new(Vec::new()));
    let _service =
        zbus::connection::Builder::address(bus.address.parse::<zbus::Address>().unwrap())
            .unwrap()
            .name(LEGACY_SERVICE_NAME)
            .unwrap()
            .serve_at(
                OBJECT_PATH,
                LegacyService(LegacyCore {
                    engine: engine.clone(),
                    received: received.clone(),
                }),
            )
            .unwrap()
            .build()
            .await
            .unwrap();

    drive_against_an_old_service(&bus, engine).await;

    let received = received.lock().unwrap().clone();
    assert_eq!(received.len(), 3, "start, status, change: {received:?}");
    for document in &received {
        assert!(document.contains(r#""protocol_version":2"#), "{document}");
        assert!(!document.contains("lid"), "{document}");
    }
}

#[tokio::test]
async fn a_new_tray_falls_back_when_the_old_service_says_a_field_is_unknown() {
    let bus = bus_or_skip!();
    let (_directory, engine) = legacy_engine().await;
    let received = Arc::new(Mutex::new(Vec::new()));
    let _service =
        zbus::connection::Builder::address(bus.address.parse::<zbus::Address>().unwrap())
            .unwrap()
            .name(LEGACY_SERVICE_NAME)
            .unwrap()
            .serve_at(
                OBJECT_PATH,
                LegacyServiceWithoutProperty(LegacyCore {
                    engine: engine.clone(),
                    received: received.clone(),
                }),
            )
            .unwrap()
            .build()
            .await
            .unwrap();

    drive_against_an_old_service(&bus, engine).await;

    let received = received.lock().unwrap().clone();
    assert!(
        received[0].contains("prevent_lid_suspend"),
        "the first start went out as version 3: {}",
        received[0]
    );
    assert_eq!(
        received.len(),
        4,
        "refused once, then three in the old version"
    );
    for document in &received[1..] {
        assert!(document.contains(r#""protocol_version":2"#), "{document}");
        assert!(!document.contains("lid"), "{document}");
    }
}

#[tokio::test]
async fn after_the_old_service_restarts_as_a_new_one_the_tray_speaks_version_3_again() {
    let bus = bus_or_skip!();
    let (_old_directory, old_engine) = legacy_engine().await;
    let old = zbus::connection::Builder::address(bus.address.parse::<zbus::Address>().unwrap())
        .unwrap()
        .name(LEGACY_SERVICE_NAME)
        .unwrap()
        .serve_at(
            OBJECT_PATH,
            LegacyService(LegacyCore {
                engine: old_engine,
                received: Arc::new(Mutex::new(Vec::new())),
            }),
        )
        .unwrap()
        .build()
        .await
        .unwrap();
    let client = ServiceClient::with_destination(
        bus.connect().await.unwrap(),
        LEGACY_SERVICE_NAME.to_string(),
    )
    .await
    .unwrap();
    assert!(
        !client
            .status()
            .await
            .unwrap()
            .backend
            .capabilities
            .lid_switch
    );
    assert!(client.speaks_legacy());

    // An update restarts the service as the current one under the same name.
    drop(old);
    let (_directory, engine) = legacy_engine().await;
    let _new = zbus::connection::Builder::address(bus.address.parse::<zbus::Address>().unwrap())
        .unwrap()
        .name(LEGACY_SERVICE_NAME)
        .unwrap()
        .serve_at(OBJECT_PATH, AwakeDbusService::new(engine.clone()))
        .unwrap()
        .build()
        .await
        .unwrap();
    // What the tray's loop does when the name gets a new owner.
    client.reset_protocol();

    let started = client
        .send(awake_tray::client::start_request(
            "從系統匣開始的工作階段",
            WireEnd::Indefinite,
            awake_tray::QuickOptions::default(),
            false,
        ))
        .await
        .unwrap();
    assert!(!client.speaks_legacy(), "the new service says 3");
    assert!(started.backend.capabilities.lid_switch);
    assert!(started.effective_policy.prevent_lid_suspend);
    assert_eq!(
        engine
            .backend()
            .held_requests()
            .iter()
            .map(|request| request.what_argument())
            .collect::<Vec<_>>(),
        vec!["sleep:idle:handle-lid-switch"]
    );
}

// ---- Ticket 112: an unfolded submenu stays unfolded --------------------------

/// The panel showing the awake menu of a session with an end, so it has an
/// "Extend session" submenu, with the menu open.
async fn open_on_a_timed_session(
    bus: &PrivateBus,
) -> (Service, zbus::Connection, Arc<TrayController>, HostMenu) {
    let service = serve_service(bus).await.unwrap();
    let (tray, controller) = serve_following_tray(bus).await;
    client(bus)
        .await
        .send(start(WireEnd::Duration { seconds: 3_600 }))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let mut host = HostMenu::attach(bus, &tray).await;
    host.open().await;
    assert!(host.shows("Extend session"), "{:#?}", host.drawn());
    (service, tray, controller, host)
}

/// The session ends somewhere else (the window, or its end time), which
/// changes the menu's layout.
async fn end_elsewhere(bus: &PrivateBus) {
    let end = awake_tray::client::menu_request(awake_tray::MenuAction::EndSession).unwrap();
    client(bus).await.send(end).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
}

#[tokio::test]
async fn an_unfolded_submenu_stays_unfolded_while_the_session_ends_and_the_layout_follows_it() {
    let bus = bus_or_skip!();
    let (_service, tray, controller, mut host) = open_on_a_timed_session(&bus).await;
    host.unfold("Extend session").await;
    let extend = host.unfolded.unwrap();

    end_elsewhere(&bus).await;
    host.pump().await;
    assert_eq!(host.unfolded, Some(extend), "the submenu folded up");
    assert_eq!(host.collapses, 0);
    // What would extend the ended session is greyed out where it is.
    let fifteen = host.find("15 min");
    assert!(!fifteen.enabled, "{fifteen:?}");
    // A click that raced the greying is refused in words, not dropped.
    assert_eq!(
        controller.activate(fifteen.id).await,
        awake_tray::Activation::Failed(awake_tray::ClientError::Rejected(
            awake_tray::controller::NO_LONGER_APPLIES.to_string()
        ))
    );

    // Folded: after the settle time the new layout goes out on its own.
    host.fold().await;
    tokio::time::sleep(awake_tray::controller::SUBMENU_SETTLE * 2).await;
    host.pump().await;
    assert!(host.shows("Start a session"), "{:#?}", host.drawn());
    assert_eq!(as_drawn_labels(&host), intended(&bus, &tray).await);
    host.assert_well_formed();
}

#[tokio::test]
async fn the_next_submenu_unfolded_right_after_the_first_is_not_folded_by_the_held_layout() {
    let bus = bus_or_skip!();
    let (_service, _tray, _controller, mut host) = open_on_a_timed_session(&bus).await;
    host.unfold("Extend session").await;
    end_elsewhere(&bus).await;
    host.pump().await;

    // GNOME folds one submenu just before it unfolds the next.
    host.unfold("Current settings").await;
    let settings = host.unfolded.unwrap();
    tokio::time::sleep(awake_tray::controller::SUBMENU_SETTLE * 2).await;
    host.pump().await;
    assert_eq!(host.unfolded, Some(settings));
    assert_eq!(host.collapses, 0);
    assert!(
        !host.shows("Start a session"),
        "the layout went out under it"
    );
}

#[tokio::test]
async fn opening_the_menu_again_sends_a_layout_held_for_a_row_that_never_said_it_closed() {
    let bus = bus_or_skip!();
    let (_service, tray, _controller, mut host) = open_on_a_timed_session(&bus).await;
    host.unfold("Extend session").await;
    end_elsewhere(&bus).await;
    host.pump().await;
    assert!(!host.shows("Start a session"));

    // The row went away without a `closed` (a destroyed row says nothing),
    // and the panel asks before showing the menu again.
    host.unfolded = None;
    host.active = false;
    let changed: (bool,) = host.call("AboutToShow", &(0i32,)).await;
    assert!(changed.0, "the held layout was not sent");
    host.active = true;
    host.layout_update().await;
    assert!(host.shows("Start a session"), "{:#?}", host.drawn());
    assert_eq!(as_drawn_labels(&host), intended(&bus, &tray).await);
}
