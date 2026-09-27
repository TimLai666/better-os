//! What the service pushes to its clients when it ends a session on its own,
//! observed on a private session bus.
//!
//! The bus is one this test started and will kill, the inhibitor is a fake, and
//! the battery is a file in a captured `/sys` tree, so nothing here touches the
//! developer's own session, rules, or history.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use awake_ipc::{AwakeEvent, AwakeRequest, EventBody, RequestBody, WireEnd};
use awake_service::backend::{FakeInhibitorBackend, FixedClock};
use awake_service::{AwakeDbusService, AwakeEngine, INTERFACE_NAME, OBJECT_PATH};
use zbus::export::futures_core::Stream;
use zbus::object_server::SignalEmitter;

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

    let address: zbus::Address = bus.address.parse().unwrap();
    let connection = zbus::connection::Builder::address(address)
        .unwrap()
        .name(SERVICE_NAME)
        .unwrap()
        .serve_at(OBJECT_PATH, AwakeDbusService::new(engine.clone()))
        .unwrap()
        .build()
        .await
        .unwrap();

    Service {
        connection,
        engine,
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
    awake_service::service::tick_and_announce(&service.engine, &service.emitter()).await;

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
    awake_service::service::tick_and_announce(&service.engine, &service.emitter()).await;
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
    awake_service::service::tick_and_announce(&service.engine, &service.emitter()).await;

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
    awake_service::service::tick_and_announce(&service.engine, &service.emitter()).await;

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
    awake_service::service::tick_and_announce(&service.engine, &service.emitter()).await;
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
    awake_service::service::tick_and_announce(&service.engine, &service.emitter()).await;

    let status = status_in(next_event(&mut stream, ARRIVES).await);
    assert!(status.sessions.is_empty());
    assert_eq!(status.rule_summary.refused, 1);
    assert_eq!(next_event(&mut stream, QUIET).await, None);

    service.clock.advance(60);
    awake_service::service::tick_and_announce(&service.engine, &service.emitter()).await;
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
    awake_service::service::tick_and_announce(&service.engine, &service.emitter()).await;

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
