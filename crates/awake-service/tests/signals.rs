//! What the service pushes to its clients when it ends a session on its own,
//! and the low-battery notification it raises when no tray is there to, observed
//! on a private session bus.
//!
//! The bus is one this test started and will kill, the inhibitor is a fake, the
//! battery is a file in a captured `/sys` tree, and the notification service is a
//! fake under a test-only name, so nothing here touches the developer's own
//! session, rules, history, or notifications.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use awake_ipc::notification::{Locale, TRAY_BUS_NAME, low_battery_notification};
use awake_ipc::{AwakeEvent, AwakeRequest, EventBody, RequestBody, WireEnd};
use awake_service::backend::{FakeInhibitorBackend, FixedClock};
use awake_service::notify::LowBatteryNotifier;
use awake_service::{AwakeDbusService, AwakeEngine, INTERFACE_NAME, OBJECT_PATH};
use zbus::export::futures_core::Stream;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

const SERVICE_NAME: &str = "org.betteros.Awake1SignalTest";
const NOW: u64 = 1_700_000_000;

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

    async fn connect(&self) -> zbus::Connection {
        let address: zbus::Address = self.address.parse().unwrap();
        zbus::connection::Builder::address(address)
            .unwrap()
            .build()
            .await
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

struct Service {
    connection: zbus::Connection,
    engine: Arc<AwakeEngine<FakeInhibitorBackend>>,
    notifier: LowBatteryNotifier,
    clock: Arc<FixedClock>,
    roots: awake_platform::Roots,
    _directory: tempfile::TempDir,
}

impl Service {
    fn set_battery(&self, percent: u8) {
        std::fs::write(
            self.roots.sys_path("class/power_supply/BAT1/capacity"),
            format!("{percent}\n"),
        )
        .unwrap();
    }

    fn set_on_ac(&self, online: bool) {
        std::fs::write(
            self.roots.sys_path("class/power_supply/ACAD/online"),
            if online { "1\n" } else { "0\n" },
        )
        .unwrap();
    }

    fn emitter(&self) -> SignalEmitter<'_> {
        SignalEmitter::new(&self.connection, OBJECT_PATH).unwrap()
    }

    /// One tick through the path `main.rs` runs.
    async fn tick(&self) -> Vec<awake_service::EndedSession> {
        awake_service::service::tick_and_announce(
            &self.engine,
            &self.emitter(),
            Some(&self.notifier),
        )
        .await
    }
}

/// A laptop on its charger at 80%, with every file the service keeps in a
/// temporary directory.
async fn serve(bus: &PrivateBus) -> Service {
    let directory = tempfile::tempdir().unwrap();
    let sys = directory.path().join("sys");
    std::fs::create_dir_all(directory.path().join("proc")).unwrap();
    awake_platform::power::write_supply(&sys, "ACAD", &[("type", "Mains"), ("online", "1")]);
    awake_platform::power::write_supply(&sys, "BAT1", &[("type", "Battery"), ("capacity", "80")]);
    let roots = awake_platform::Roots::at(directory.path());

    let clock = Arc::new(FixedClock::at(NOW));
    let engine = Arc::new(
        AwakeEngine::start_in(
            FakeInhibitorBackend::logind_shaped(),
            directory.path(),
            clock.clone(),
        )
        .await,
    );

    // The service's own notifier, pointed at the fake notification service, so
    // no test can reach the desktop's.
    let notifier = LowBatteryNotifier::new(tokio::runtime::Handle::current(), Locale::EnUs)
        .with_destination(NOTIFICATIONS_NAME.to_string())
        .with_timeout(Duration::from_millis(500));

    let address: zbus::Address = bus.address.parse().unwrap();
    let connection = zbus::connection::Builder::address(address)
        .unwrap()
        .name(SERVICE_NAME)
        .unwrap()
        .serve_at(
            OBJECT_PATH,
            AwakeDbusService::new(engine.clone()).with_notifier(notifier.clone()),
        )
        .unwrap()
        .build()
        .await
        .unwrap();

    Service {
        connection,
        engine,
        notifier,
        clock,
        roots,
        _directory: directory,
    }
}

/// Every event the service pushes, in order.
async fn events(bus: &PrivateBus) -> (zbus::Connection, zbus::MessageStream) {
    let connection = bus.connect().await;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(INTERFACE_NAME)
        .unwrap()
        .member("StatusChanged")
        .unwrap()
        .build();
    let stream = zbus::MessageStream::for_match_rule(rule, &connection, None)
        .await
        .unwrap();
    (connection, stream)
}

/// The next pushed event, or `None` when nothing arrives in time.
async fn next_event(stream: &mut zbus::MessageStream, wait: Duration) -> Option<AwakeEvent> {
    let mut stream = std::pin::Pin::new(stream);
    let message = tokio::time::timeout(
        wait,
        std::future::poll_fn(|context| stream.as_mut().poll_next(context)),
    )
    .await
    .ok()??
    .ok()?;
    let document: String = message.body().deserialize().ok()?;
    Some(AwakeEvent::from_json(&document).unwrap())
}

fn start_request() -> AwakeRequest {
    AwakeRequest::new(RequestBody::StartSession {
        reason: "Android Studio build is running".to_string(),
        policy: awake_core::SessionPolicy::quick_default(),
        battery_stop_percent: Some(20),
        end: WireEnd::Indefinite,
        security_confirmed: false,
    })
}

fn is_status(event: &AwakeEvent) -> bool {
    matches!(event.body, EventBody::StatusChanged(_))
}

const ARRIVES: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(300);

#[tokio::test]
async fn a_low_battery_stop_on_a_tick_is_pushed_once_and_then_the_status_follows() {
    let bus = bus_or_skip!();
    let service = serve(&bus).await;
    // Started on the engine directly, so the only signals on the bus are the
    // ones the tick sends.
    service.engine.handle(start_request()).await;
    let session_id = service.engine.status().await.sessions[0].session_id;
    let (_listener, mut stream) = events(&bus).await;

    service.set_battery(9);
    service.clock.advance(60);
    service.tick().await;

    assert_eq!(
        next_event(&mut stream, ARRIVES)
            .await
            .map(|event| event.body),
        Some(EventBody::SessionEnded {
            session_id,
            cause: "battery_threshold".to_string(),
            battery_stop_percent: Some(20),
            battery_percent: Some(9),
        })
    );
    let status = next_event(&mut stream, ARRIVES)
        .await
        .expect("a status follows the end, so the menu stops showing the session");
    match status.body {
        EventBody::StatusChanged(status) => assert!(status.sessions.is_empty()),
        other => panic!("expected a status, got {other:?}"),
    }

    service.clock.advance(60);
    service.tick().await;
    assert_eq!(
        next_event(&mut stream, QUIET).await,
        None,
        "a tick that ended nothing pushes nothing"
    );
}

#[tokio::test]
async fn a_session_that_expires_on_a_tick_is_pushed_with_its_cause() {
    let bus = bus_or_skip!();
    let service = serve(&bus).await;
    service
        .engine
        .handle(AwakeRequest::new(RequestBody::StartSession {
            reason: "Short build".to_string(),
            policy: awake_core::SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
            end: WireEnd::Duration { seconds: 900 },
            security_confirmed: false,
        }))
        .await;
    let session_id = service.engine.status().await.sessions[0].session_id;
    let (_listener, mut stream) = events(&bus).await;

    service.clock.advance(900);
    service.tick().await;

    assert_eq!(
        next_event(&mut stream, ARRIVES)
            .await
            .map(|event| event.body),
        Some(EventBody::SessionEnded {
            session_id,
            cause: "expired".to_string(),
            battery_stop_percent: Some(20),
            battery_percent: None,
        })
    );
}

#[tokio::test]
async fn a_battery_stop_found_while_answering_a_request_is_pushed_before_the_status() {
    let bus = bus_or_skip!();
    let service = serve(&bus).await;
    service.engine.handle(start_request()).await;
    let session_id = service.engine.status().await.sessions[0].session_id;
    let (listener, mut stream) = events(&bus).await;
    service.set_battery(9);

    let rule = awake_core::Rule::new(
        awake_core::RuleId(0),
        awake_core::Reason::new("Charging").unwrap(),
        awake_core::Combine::All,
        [
            awake_core::ConditionGroup::one(awake_core::Condition::AcPower { connected: true })
                .unwrap(),
        ],
    )
    .unwrap();
    let request = AwakeRequest::new(RequestBody::CreateRule {
        rule: Box::new(rule),
    })
    .to_json()
    .unwrap();
    listener
        .call_method(
            Some(SERVICE_NAME),
            OBJECT_PATH,
            Some(INTERFACE_NAME),
            "Request",
            &(request.as_str(),),
        )
        .await
        .unwrap();

    let first = next_event(&mut stream, ARRIVES).await.unwrap();
    assert_eq!(
        first.body,
        EventBody::SessionEnded {
            session_id,
            cause: "battery_threshold".to_string(),
            battery_stop_percent: Some(20),
            battery_percent: Some(9),
        }
    );
    assert!(is_status(&next_event(&mut stream, ARRIVES).await.unwrap()));
    assert_eq!(next_event(&mut stream, QUIET).await, None);
}

#[tokio::test]
async fn ending_a_session_on_request_pushes_the_status_and_no_session_ended() {
    let bus = bus_or_skip!();
    let service = serve(&bus).await;
    service.engine.handle(start_request()).await;
    let (listener, mut stream) = events(&bus).await;

    let request = AwakeRequest::new(RequestBody::EndManualSession)
        .to_json()
        .unwrap();
    listener
        .call_method(
            Some(SERVICE_NAME),
            OBJECT_PATH,
            Some(INTERFACE_NAME),
            "Request",
            &(request.as_str(),),
        )
        .await
        .unwrap();

    assert!(is_status(&next_event(&mut stream, ARRIVES).await.unwrap()));
    assert_eq!(next_event(&mut stream, QUIET).await, None);
}

#[tokio::test]
async fn a_shutdown_pushes_every_session_it_ends() {
    let bus = bus_or_skip!();
    let service = serve(&bus).await;
    service.engine.handle(start_request()).await;
    let session_id = service.engine.status().await.sessions[0].session_id;
    let (_listener, mut stream) = events(&bus).await;

    awake_service::service::shutdown_and_announce(&service.engine, &service.emitter()).await;

    assert_eq!(
        next_event(&mut stream, ARRIVES)
            .await
            .map(|event| event.body),
        Some(EventBody::SessionEnded {
            session_id,
            cause: "service_shutdown".to_string(),
            battery_stop_percent: Some(20),
            battery_percent: None,
        })
    );
    assert!(!service.engine.holds_inhibitor().await);
}

/// A rule that matches while the charger is plugged in.
fn on_ac_rule() -> AwakeRequest {
    AwakeRequest::new(RequestBody::CreateRule {
        rule: Box::new(
            awake_core::Rule::new(
                awake_core::RuleId(0),
                awake_core::Reason::new("Charging").unwrap(),
                awake_core::Combine::All,
                [
                    awake_core::ConditionGroup::one(awake_core::Condition::AcPower {
                        connected: true,
                    })
                    .unwrap(),
                ],
            )
            .unwrap(),
        ),
    })
}

fn status_in(event: Option<AwakeEvent>) -> awake_ipc::StatusDocument {
    match event.map(|event| event.body) {
        Some(EventBody::StatusChanged(status)) => *status,
        other => panic!("expected a status, got {other:?}"),
    }
}

#[tokio::test]
async fn a_rule_that_starts_on_a_tick_pushes_one_status() {
    let bus = bus_or_skip!();
    let service = serve(&bus).await;
    // The charger is out when the rule is written, so the rule waits for a tick.
    service.set_on_ac(false);
    service.engine.handle(on_ac_rule()).await;
    assert!(service.engine.status().await.sessions.is_empty());
    let (_listener, mut stream) = events(&bus).await;

    service.set_on_ac(true);
    service.clock.advance(60);
    service.tick().await;

    let status = status_in(next_event(&mut stream, ARRIVES).await);
    assert_eq!(status.sessions.len(), 1);
    assert_eq!(
        status.sessions[0].origin,
        awake_core::SessionOrigin::Trigger
    );
    assert_eq!(
        next_event(&mut stream, QUIET).await,
        None,
        "one status for the start"
    );

    service.clock.advance(60);
    service.tick().await;
    assert_eq!(
        next_event(&mut stream, QUIET).await,
        None,
        "a tick that leaves the rule's session as it was pushes nothing"
    );
}

#[tokio::test]
async fn a_rule_the_battery_holds_back_on_a_tick_pushes_one_status() {
    let bus = bus_or_skip!();
    let service = serve(&bus).await;
    service.set_on_ac(false);
    service.set_battery(9);
    service.engine.handle(on_ac_rule()).await;
    assert_eq!(service.engine.status().await.rule_summary.refused, 0);
    let (_listener, mut stream) = events(&bus).await;

    // The rule now matches, and the battery is below the threshold it carries,
    // so it is refused rather than started.
    service.set_on_ac(true);
    service.clock.advance(60);
    service.tick().await;

    let status = status_in(next_event(&mut stream, ARRIVES).await);
    assert!(status.sessions.is_empty());
    assert_eq!(status.rule_summary.refused, 1);
    assert_eq!(next_event(&mut stream, QUIET).await, None);

    service.clock.advance(60);
    service.tick().await;
    assert_eq!(
        next_event(&mut stream, QUIET).await,
        None,
        "a rule still held back is not a change"
    );
}

#[tokio::test]
async fn a_tick_that_ends_and_starts_sessions_pushes_the_ends_first_and_one_status_after() {
    let bus = bus_or_skip!();
    let service = serve(&bus).await;
    service.set_on_ac(false);
    service.engine.handle(on_ac_rule()).await;
    service
        .engine
        .handle(AwakeRequest::new(RequestBody::StartSession {
            reason: "Short build".to_string(),
            policy: awake_core::SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
            end: WireEnd::Duration { seconds: 900 },
            security_confirmed: false,
        }))
        .await;
    let manual = service.engine.status().await.sessions[0].session_id;
    let (_listener, mut stream) = events(&bus).await;

    // On one tick the manual session expires and the rule takes hold.
    service.set_on_ac(true);
    service.clock.advance(900);
    service.tick().await;

    assert_eq!(
        next_event(&mut stream, ARRIVES)
            .await
            .map(|event| event.body),
        Some(EventBody::SessionEnded {
            session_id: manual,
            cause: "expired".to_string(),
            battery_stop_percent: Some(20),
            battery_percent: None,
        })
    );
    let status = status_in(next_event(&mut stream, ARRIVES).await);
    assert_eq!(status.sessions.len(), 1);
    assert_ne!(status.sessions[0].session_id, manual);
    assert_eq!(
        status.sessions[0].origin,
        awake_core::SessionOrigin::Trigger
    );
    assert_eq!(
        next_event(&mut stream, QUIET).await,
        None,
        "one status after the ends, not one per change"
    );
}

// ---- The notification the service raises when no tray is running ----------
//
// The notification service is a fake under a test-only name on the private bus.
// Nothing here can reach the developer's own notification daemon: the bus is
// not theirs, and the name is not one any daemon answers to.

const NOTIFICATIONS_NAME: &str = "org.betteros.NotificationsTest";

/// One `Notify` call as the fake received it.
#[derive(Clone, Debug)]
struct Raised {
    sender: String,
    app_name: String,
    summary: String,
    body: String,
}

#[derive(Clone, Copy)]
enum Behaviour {
    Accept,
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
        _replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        _hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        if let Behaviour::NeverAnswer = self.behaviour {
            std::future::pending::<()>().await;
        }
        let mut raised = self.raised.lock().unwrap();
        raised.push(Raised {
            sender: header
                .sender()
                .map(|sender| sender.to_string())
                .unwrap_or_default(),
            app_name,
            summary,
            body,
        });
        raised.len() as u32
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

/// What the fake has received once `expected` calls have arrived, or the wait
/// ran out, and then a quiet moment passed with nothing more arriving.
async fn settle(raised: &Arc<Mutex<Vec<Raised>>>, expected: usize) -> Vec<Raised> {
    let deadline = tokio::time::Instant::now() + ARRIVES;
    while raised.lock().unwrap().len() < expected && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(QUIET).await;
    raised.lock().unwrap().clone()
}

/// A tray, as the service sees one: a connection owning the tray's name.
async fn tray_on(bus: &PrivateBus) -> zbus::Connection {
    let tray = bus.connect().await;
    tray.request_name(TRAY_BUS_NAME).await.unwrap();
    tray
}

/// Starts a manual session and flattens the battery under it, ready for a tick.
async fn a_session_on_a_flat_battery(service: &Service) -> u64 {
    service.engine.handle(start_request()).await;
    let session_id = service.engine.status().await.sessions[0].session_id;
    service.set_battery(9);
    service.clock.advance(60);
    session_id
}

#[tokio::test]
async fn with_no_tray_the_service_raises_the_low_battery_notification_itself() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let service = serve(&bus).await;
    let session_id = a_session_on_a_flat_battery(&service).await;
    let (_listener, mut stream) = events(&bus).await;

    service.tick().await;

    let raised = settle(&raised, 1).await;
    assert_eq!(raised.len(), 1, "one stop, one notification");
    let expected = low_battery_notification(Locale::EnUs, 20, 9);
    assert_eq!(raised[0].app_name, expected.application_name);
    assert_eq!(raised[0].summary, expected.summary);
    assert_eq!(raised[0].body, expected.body);
    assert_eq!(
        raised[0].sender,
        service.connection.unique_name().unwrap().to_string(),
        "the service raised it"
    );

    // The clients are still told, in the same order as before.
    assert!(matches!(
        next_event(&mut stream, ARRIVES).await.map(|event| event.body),
        Some(EventBody::SessionEnded { session_id: id, .. }) if id == session_id
    ));
    assert!(is_status(&next_event(&mut stream, ARRIVES).await.unwrap()));
}

#[tokio::test]
async fn with_a_tray_on_the_bus_the_service_leaves_the_notification_to_it() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let service = serve(&bus).await;
    let _tray = tray_on(&bus).await;
    let session_id = a_session_on_a_flat_battery(&service).await;
    let (_listener, mut stream) = events(&bus).await;

    service.tick().await;

    assert!(
        settle(&raised, 0).await.is_empty(),
        "the tray raises it, so the service must not"
    );
    assert!(matches!(
        next_event(&mut stream, ARRIVES).await.map(|event| event.body),
        Some(EventBody::SessionEnded { session_id: id, .. }) if id == session_id
    ));
}

#[tokio::test]
async fn a_tray_that_has_quit_no_longer_stops_the_service_from_notifying() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let service = serve(&bus).await;
    let tray = tray_on(&bus).await;
    drop(tray);

    // The bus drops the name when the tray's connection closes.
    let bus_proxy = zbus::fdo::DBusProxy::new(&bus.connect().await)
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + ARRIVES;
    while bus_proxy
        .name_has_owner(TRAY_BUS_NAME.try_into().unwrap())
        .await
        .unwrap()
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the tray's name was never released"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    a_session_on_a_flat_battery(&service).await;
    service.tick().await;

    assert_eq!(settle(&raised, 1).await.len(), 1);
}

#[tokio::test]
async fn a_battery_stop_found_while_answering_a_request_is_notified_by_the_service() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::Accept).await;
    let service = serve(&bus).await;
    service.engine.handle(start_request()).await;
    service.set_battery(9);
    let caller = bus.connect().await;

    // A rule edit re-reads the battery and finds the stop inside the D-Bus
    // handler. The reply must still come back: the notification is raised
    // beside the handler, not inside it.
    let request = on_ac_rule().to_json().unwrap();
    tokio::time::timeout(
        ARRIVES,
        caller.call_method(
            Some(SERVICE_NAME),
            OBJECT_PATH,
            Some(INTERFACE_NAME),
            "Request",
            &(request.as_str(),),
        ),
    )
    .await
    .expect("the request must be answered")
    .unwrap();

    let raised = settle(&raised, 1).await;
    assert_eq!(raised.len(), 1);
    assert_eq!(
        raised[0].summary,
        low_battery_notification(Locale::EnUs, 20, 9).summary
    );
}

#[tokio::test]
async fn no_notification_service_changes_nothing_but_the_notification() {
    let bus = bus_or_skip!();
    // Nothing owns the notification name on this bus.
    let service = serve(&bus).await;
    let session_id = a_session_on_a_flat_battery(&service).await;
    let (_listener, mut stream) = events(&bus).await;

    let ended = service.tick().await;

    assert_eq!(ended.len(), 1);
    assert!(matches!(
        next_event(&mut stream, ARRIVES).await.map(|event| event.body),
        Some(EventBody::SessionEnded { session_id: id, .. }) if id == session_id
    ));
    assert!(is_status(&next_event(&mut stream, ARRIVES).await.unwrap()));
    assert!(service.engine.status().await.sessions.is_empty());
}

#[tokio::test]
async fn a_notification_service_that_never_answers_does_not_hold_up_the_tick() {
    let bus = bus_or_skip!();
    let (_daemon, raised) = serve_notifications(&bus, Behaviour::NeverAnswer).await;
    let service = serve(&bus).await;
    a_session_on_a_flat_battery(&service).await;
    let (_listener, mut stream) = events(&bus).await;

    let ended = tokio::time::timeout(Duration::from_secs(2), service.tick())
        .await
        .expect("the tick must not wait for the notification service");

    assert_eq!(ended.len(), 1);
    assert!(next_event(&mut stream, ARRIVES).await.is_some());
    assert!(is_status(&next_event(&mut stream, ARRIVES).await.unwrap()));
    assert!(raised.lock().unwrap().is_empty());
}
