//! The whole gesture pipeline, end to end, over a private session bus.
//!
//! The house pattern from `storage-service` and `manager-daemon`: a private
//! `dbus-daemon`, nothing real behind it, and assertions about what actually
//! crossed the bus. What is different here is that both ends are real. The
//! service side is the shipped `GesturePipeline`, the shipped
//! `SessionBusShell`, and the shipped recognizer; only the extension is a fake,
//! and it is a fake that serves the same `org.betteros.TouchpadAdapter1`
//! interface the GJS one does and emits the same two signals.
//!
//! So a recorded gesture stream goes in as D-Bus signals and typed method calls
//! come out, and every threshold, cancellation, and cooldown decision between
//! them is made by the same code a real session would run.
//!
//! Nothing here touches the developer's own session, and nothing installs the
//! extension: the bus is private, the store directory is temporary, and the
//! test skips itself where `dbus-daemon` is not available.

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use touchpad_gesture_service::{GesturePipeline, StopReason};
use touchpad_gestures::{
    ConflictResolution, GNOME_46_GESTURES, GestureConfig, GestureStore, PresetPlan, RunState,
    SuppressionEvent, mac_style,
};
use touchpad_session::{
    GnomeShellAdapter, InputDeviceNode, InvocationOutcome, ServiceBus, ServiceBusError,
    SessionAdapter, SessionBusShell, SessionEvent, ShellEvents, ShellGestureEvent,
};
use zbus::interface;
use zbus::object_server::SignalEmitter;

const TEST_NAME: &str = "org.betteros.TouchpadAdapter1Test";
const OBJECT_PATH: &str = "/org/betteros/TouchpadAdapter1";

/// How long a read waits before the stream is treated as ended. Only the tests
/// bound the wait; the service blocks for as long as the session lasts.
const QUIET: std::time::Duration = std::time::Duration::from_millis(750);

// ── The private bus ─────────────────────────────────────────────────────────

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

// ── The fake extension ──────────────────────────────────────────────────────

/// Everything the extension was asked to do, in order.
#[derive(Clone, Debug, Default, PartialEq)]
struct ShellCalls {
    calls: Vec<String>,
    /// The unique name each `SuppressBuiltInGestures` came from.
    senders: Vec<String>,
}

/// The same interface the GJS extension serves, backed by a recording object
/// instead of GNOME Shell.
struct FakeExtension {
    recorded: Arc<Mutex<ShellCalls>>,
    /// Whether the method calls should fail, for the auto-disable path.
    failing: bool,
}

#[interface(name = "org.betteros.TouchpadAdapter1")]
impl FakeExtension {
    async fn show_overview(&self) -> Result<(), zbus::fdo::Error> {
        self.record("ShowOverview")
    }

    async fn show_desktop(&self) -> Result<(), zbus::fdo::Error> {
        self.record("ShowDesktop")
    }

    async fn switch_workspace(&self, direction: i32) -> Result<(), zbus::fdo::Error> {
        self.record(&format!("SwitchWorkspace({direction})"))
    }

    async fn suppress_built_in_gestures(
        &self,
        suppress: bool,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> Result<(), zbus::fdo::Error> {
        // Who asked, because the real extension takes this request only from
        // the gesture service's own name (ticket 131).
        if let Some(sender) = header.sender() {
            self.recorded
                .lock()
                .expect("recorded calls")
                .senders
                .push(sender.to_string());
        }
        self.record(&format!("SuppressBuiltInGestures({suppress})"))
    }

    async fn capabilities(&self) -> String {
        // Word for word what the GJS extension reports on GNOME 46.
        r#"{"protocol_version":2,"shell_version":"46.0","finger_count":true,
            "thumb_detection":false,"continuous_progress":true,
            "gesture_kinds":["swipe","pinch"],
            "actions":["overview","show-desktop","switch-workspace"],
            "unsupported_actions":["current-application-windows"],
            "built_in_trackers":2,
            "built_in_gestures_suppressed":false}"#
            .to_string()
    }

    #[zbus(signal)]
    async fn swipe_gesture(
        emitter: &SignalEmitter<'_>,
        phase: u32,
        fingers: u32,
        dx: f64,
        dy: f64,
        at_ms: u64,
        device: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn pinch_gesture(
        emitter: &SignalEmitter<'_>,
        phase: u32,
        fingers: u32,
        scale: f64,
        angle_delta: f64,
        at_ms: u64,
        device: &str,
    ) -> zbus::Result<()>;
}

impl FakeExtension {
    fn record(&self, call: &str) -> Result<(), zbus::fdo::Error> {
        if self.failing {
            return Err(zbus::fdo::Error::Failed("the shell did not answer".into()));
        }
        self.recorded
            .lock()
            .expect("recorded calls")
            .calls
            .push(call.to_string());
        Ok(())
    }
}

/// The extension end of the bus: a runtime, a connection, and the recorded
/// calls. Emitting a signal is a method on it, so a test writes a gesture
/// stream as a list rather than as bus plumbing.
struct Extension {
    runtime: tokio::runtime::Runtime,
    connection: zbus::Connection,
    recorded: Arc<Mutex<ShellCalls>>,
}

impl Extension {
    fn serve(bus: &PrivateBus, failing: bool) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("a runtime");
        let recorded = Arc::new(Mutex::new(ShellCalls::default()));
        let served = FakeExtension {
            recorded: recorded.clone(),
            failing,
        };
        let address: zbus::Address = bus.address.parse().expect("an address");
        let connection = runtime
            .block_on(async {
                zbus::connection::Builder::address(address)?
                    .name(TEST_NAME)?
                    .serve_at(OBJECT_PATH, served)?
                    .build()
                    .await
            })
            .expect("the fake extension owns its name");
        Self {
            runtime,
            connection,
            recorded,
        }
    }

    fn emitter(&self) -> SignalEmitter<'_> {
        SignalEmitter::new(&self.connection, OBJECT_PATH).expect("an emitter")
    }

    /// A swipe from a device the shell could not name, which is what most of
    /// these tests need: with one pad there is nothing to route.
    fn swipe(&self, phase: u32, fingers: u32, dx: f64, dy: f64, at_ms: u64) {
        self.swipe_on("", phase, fingers, dx, dy, at_ms);
    }

    fn swipe_on(&self, device: &str, phase: u32, fingers: u32, dx: f64, dy: f64, at_ms: u64) {
        self.runtime
            .block_on(FakeExtension::swipe_gesture(
                &self.emitter(),
                phase,
                fingers,
                dx,
                dy,
                at_ms,
                device,
            ))
            .expect("a swipe signal");
    }

    /// A swipe exactly as a protocol 1 extension emitted it: five arguments
    /// and no device. A shell keeps running the old extension until the next
    /// login, so the service has to read this shape too.
    fn protocol_1_swipe(&self, phase: u32, fingers: u32, dx: f64, dy: f64, at_ms: u64) {
        self.runtime
            .block_on(self.connection.emit_signal(
                None::<&str>,
                OBJECT_PATH,
                "org.betteros.TouchpadAdapter1",
                "SwipeGesture",
                &(phase, fingers, dx, dy, at_ms),
            ))
            .expect("a protocol 1 swipe signal");
    }

    fn pinch(&self, phase: u32, fingers: u32, scale: f64, angle: f64, at_ms: u64) {
        self.runtime
            .block_on(FakeExtension::pinch_gesture(
                &self.emitter(),
                phase,
                fingers,
                scale,
                angle,
                at_ms,
                "",
            ))
            .expect("a pinch signal");
    }

    fn calls(&self) -> Vec<String> {
        self.recorded.lock().expect("recorded calls").calls.clone()
    }

    fn senders(&self) -> Vec<String> {
        self.recorded
            .lock()
            .expect("recorded calls")
            .senders
            .clone()
    }

    /// The extension being disabled: it gives its name up.
    fn leave(&self) {
        self.runtime
            .block_on(self.connection.release_name(TEST_NAME))
            .expect("the fake extension gave its name up");
    }
}

// ── Recorded gesture streams ────────────────────────────────────────────────

/// One whole swipe under the shipped scales: 0.18 of the pad at 1000 pixels to
/// the pad.
const WHOLE_SWIPE: f64 = 180.0;

/// A four-finger swipe, emitted as the compositor would report it.
fn swipe(extension: &Extension, dx: f64, dy: f64, from_ms: u64, steps: u64) {
    extension.swipe(0, 4, 0.0, 0.0, from_ms);
    for step in 1..=steps {
        let phase = if step == steps { 2 } else { 1 };
        extension.swipe(
            phase,
            4,
            dx / steps as f64,
            dy / steps as f64,
            from_ms + step * 16,
        );
    }
}

/// A four-contact pinch to `scale`, which the recognizer reads as the launcher
/// gesture when it closes and Show Desktop when it opens.
fn pinch(extension: &Extension, scale: f64, from_ms: u64, steps: u64) {
    extension.pinch(0, 4, 1.0, 0.0, from_ms);
    for step in 1..=steps {
        let fraction = step as f64 / steps as f64;
        let phase = if step == steps { 2 } else { 1 };
        extension.pinch(
            phase,
            4,
            1.0 + (scale - 1.0) * fraction,
            0.0,
            from_ms + step * 16,
        );
    }
}

// ── The service end ─────────────────────────────────────────────────────────

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("a temporary directory"),
        }
    }

    fn store(&self) -> GestureStore {
        GestureStore::at(self.root.path().join("touchpad"))
    }

    /// The Mac-style preset, previewed and confirmed with every conflict
    /// resolved by taking the gesture from the desktop. This is the state a
    /// user reaches by pressing Apply, produced through the real plan rather
    /// than assembled by hand.
    fn applied_preset(&self, adapter: &mut dyn SessionAdapter) -> GestureConfig {
        let plan = PresetPlan::build(
            &GestureConfig::default(),
            &mac_style(),
            GNOME_46_GESTURES,
            adapter,
        );
        let resolutions = plan
            .conflicts
            .iter()
            .map(|conflict| (conflict.gesture.clone(), ConflictResolution::DisableBuiltIn))
            .collect();
        let approved = plan.approve(&resolutions, true).expect("an approved plan");
        let (config, _) = approved.apply(adapter);
        config
    }
}

fn shell(bus: &PrivateBus) -> SessionBusShell {
    SessionBusShell::connect_to(&bus.address, TEST_NAME)
        .expect("a connection to the fake extension")
}

fn adapter(bus: &PrivateBus) -> GnomeShellAdapter {
    GnomeShellAdapter::connect(Box::new(shell(bus))).expect("the extension answered Capabilities")
}

fn events(bus: &PrivateBus) -> impl ShellEvents {
    shell(bus)
        .events()
        .expect("a subscription to the extension's signals")
        .ending_after_quiet(QUIET)
}

// ── The tests ───────────────────────────────────────────────────────────────

#[test]
fn a_recorded_swipe_stream_reaches_the_shell_as_the_overview_action() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    swipe(&extension, 0.0, -WHOLE_SWIPE, 0, 8);
    assert_eq!(pipeline.run(&mut listening), StopReason::StreamEnded);

    assert!(
        extension.calls().contains(&"ShowOverview".to_string()),
        "{:?}",
        extension.calls()
    );
    let completed: Vec<&str> = pipeline
        .performed()
        .iter()
        .filter(|one| one.outcome == InvocationOutcome::Invoked)
        .map(|one| one.action)
        .collect();
    assert_eq!(completed, vec!["overview.show"]);
}

#[test]
fn every_phase_of_the_gesture_is_forwarded_and_only_the_last_one_acts() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    swipe(&extension, 0.0, -WHOLE_SWIPE, 0, 8);
    pipeline.run(&mut listening);

    let progress: Vec<f32> = pipeline
        .performed()
        .iter()
        .filter(|one| matches!(one.outcome, InvocationOutcome::Ignored { .. }))
        .map(|one| one.progress)
        .collect();
    assert!(
        progress.len() > 3,
        "the adapter saw no intermediate progress: {progress:?}"
    );
    assert!(
        progress.windows(2).all(|pair| pair[1] >= pair[0] - 1e-6),
        "{progress:?}"
    );
    // Continuous progress reached the adapter; exactly one call reached the
    // shell, because GNOME 46 has no way to drive its own transition from
    // outside and the adapter says so rather than pretending.
    assert_eq!(
        extension
            .calls()
            .iter()
            .filter(|call| *call == "ShowOverview")
            .count(),
        1
    );
}

#[test]
fn a_gesture_reversed_before_the_threshold_never_reaches_the_shell() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    // Up most of the way, then back down again, then the fingers lift.
    extension.swipe(0, 4, 0.0, 0.0, 0);
    for step in 1..=8 {
        extension.swipe(1, 4, 0.0, -WHOLE_SWIPE / 8.0, step * 16);
    }
    for step in 9..=16 {
        extension.swipe(1, 4, 0.0, WHOLE_SWIPE / 8.0, step * 16);
    }
    extension.swipe(2, 4, 0.0, 0.0, 17 * 16);
    pipeline.run(&mut listening);

    assert!(
        !extension.calls().contains(&"ShowOverview".to_string()),
        "a cancelled gesture reached the desktop: {:?}",
        extension.calls()
    );
}

#[test]
fn the_cooldown_swallows_a_second_gesture_made_immediately_after_the_first() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    swipe(&extension, 0.0, -WHOLE_SWIPE, 0, 8);
    // The cooldown is 350 ms and the second gesture starts 200 ms in.
    swipe(&extension, 0.0, -WHOLE_SWIPE, 200, 8);
    // And a third, well outside it.
    swipe(&extension, 0.0, -WHOLE_SWIPE, 2_000, 8);
    pipeline.run(&mut listening);

    assert_eq!(
        extension
            .calls()
            .iter()
            .filter(|call| *call == "ShowOverview")
            .count(),
        2,
        "{:?}",
        extension.calls()
    );
}

#[test]
fn a_four_contact_pinch_asks_the_launcher_and_never_the_shell() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    // The shell adapter alone: Better Launcher is not running on this private
    // bus, and the point of this test is which route the action takes, not
    // whether the launcher answered.
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    pinch(&extension, 0.5, 0, 8);
    pipeline.run(&mut listening);

    // The thumb the preset wants is invisible to a compositor, and the gesture
    // is still recognized: four contacts pinching is the launcher, because no
    // other four-contact pinch is configured.
    let launcher: Vec<&str> = pipeline
        .performed()
        .iter()
        .map(|one| one.action)
        .filter(|action| *action == "better-launcher.open")
        .collect();
    assert!(!launcher.is_empty(), "{:?}", pipeline.performed());
    // Nothing but the suppression the confirmed plan asked for: a launcher
    // gesture must not reach the shell by the wrong route.
    assert!(
        extension
            .calls()
            .iter()
            .all(|call| call.starts_with("SuppressBuiltInGestures")),
        "a launcher gesture reached the shell: {:?}",
        extension.calls()
    );
}

/// A gesture GNOME 46's own trackers answer: three fingers up. The Mac-style
/// preset has none (ticket 131), so the suppression path needs one of its own.
fn three_finger_overview() -> GestureConfig {
    let gesture = touchpad_gestures::GestureDefinition::new(
        "three-up",
        touchpad_gestures::GestureShape::Swipe,
        3,
        false,
        Some(touchpad_gestures::Direction::Up),
        better_actions::DesktopAction::ShowOverview,
    )
    .unwrap();
    GestureConfig::with_gestures(vec![gesture], touchpad_gestures::PresetId::Custom)
}

fn suppress_calls(extension: &Extension) -> usize {
    extension
        .calls()
        .iter()
        .filter(|call| *call == "SuppressBuiltInGestures(true)")
        .count()
}

const SERVICE_TEST_NAME: &str = "org.betteros.TouchpadGestures1Test";

fn next(service: &ServiceBus) -> SessionEvent {
    service
        .next_event_within(std::time::Duration::from_secs(5))
        .expect("an event within five seconds")
}

/// Ticket 131: the service's own connection owns its name, hears the adapter
/// come and go, carries its gestures, and makes its calls as that name's owner,
/// which is the one caller the extension takes a suppression from.
#[test]
fn the_service_bus_follows_the_adapter_and_calls_as_the_owner_of_its_own_name() {
    let bus = bus_or_skip!();
    let service = ServiceBus::connect_to(&bus.address, TEST_NAME, SERVICE_TEST_NAME)
        .expect("the service took its name");
    assert!(!service.adapter_present());

    // A second instance finds the name taken.
    assert!(matches!(
        ServiceBus::connect_to(&bus.address, TEST_NAME, SERVICE_TEST_NAME),
        Err(ServiceBusError::AlreadyRunning)
    ));

    let extension = Extension::serve(&bus, false);
    assert_eq!(next(&service), SessionEvent::AdapterAppeared);
    assert!(service.adapter_present());

    extension.swipe_on("/dev/input/event5", 0, 4, 0.0, -3.0, 7);
    match next(&service) {
        SessionEvent::Gesture(ShellGestureEvent::Swipe {
            fingers, device, ..
        }) => {
            assert_eq!(fingers, 4);
            assert_eq!(device, InputDeviceNode::from_wire("/dev/input/event5"));
        }
        other => panic!("{other:?}"),
    }

    let mut adapter =
        GnomeShellAdapter::connect(Box::new(service.shell())).expect("the adapter answered");
    adapter.suppress_built_in_gestures(true);
    let owner = {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let address: zbus::Address = bus.address.parse().unwrap();
            let connection = zbus::connection::Builder::address(address)
                .unwrap()
                .build()
                .await
                .unwrap();
            let proxy = zbus::fdo::DBusProxy::new(&connection).await.unwrap();
            proxy
                .get_name_owner(SERVICE_TEST_NAME.try_into().unwrap())
                .await
                .unwrap()
                .to_string()
        })
    };
    assert_eq!(extension.senders(), vec![owner]);

    extension.leave();
    assert_eq!(next(&service), SessionEvent::AdapterVanished);
    assert!(!service.adapter_present());

    service.stop_sender().send(SessionEvent::Stop).unwrap();
    assert_eq!(next(&service), SessionEvent::Stop);
}

#[test]
fn a_pinch_carries_its_device_like_a_swipe() {
    let bus = bus_or_skip!();
    let service = ServiceBus::connect_to(&bus.address, TEST_NAME, SERVICE_TEST_NAME)
        .expect("the service took its name");
    let extension = Extension::serve(&bus, false);
    assert_eq!(next(&service), SessionEvent::AdapterAppeared);
    extension
        .runtime
        .block_on(FakeExtension::pinch_gesture(
            &extension.emitter(),
            0,
            4,
            1.0,
            0.0,
            3,
            "/dev/input/event9",
        ))
        .expect("a pinch signal");
    match next(&service) {
        SessionEvent::Gesture(ShellGestureEvent::Pinch { device, .. }) => {
            assert_eq!(device, InputDeviceNode::from_wire("/dev/input/event9"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_shipped_preset_takes_none_of_gnomes_gestures() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));
    pipeline.verify();
    pipeline.suppress_built_in_gestures();
    swipe(&extension, 0.0, -WHOLE_SWIPE, 0, 8);
    pipeline.run(&mut listening);

    assert!(pipeline.colliding_gestures().is_empty());
    assert_eq!(suppress_calls(&extension), 0, "{:?}", extension.calls());
}

#[test]
fn a_colliding_gesture_takes_gnomes_only_after_a_gesture_arrives_and_gives_them_back() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut pipeline = GesturePipeline::new(
        three_finger_overview(),
        fixture.store(),
        Box::new(adapter(&bus)),
    );
    pipeline.verify();
    assert_eq!(pipeline.colliding_gestures(), vec!["three-up".to_string()]);

    // Verified and colliding, but nothing has arrived: nothing is taken.
    pipeline.suppress_built_in_gestures();
    assert!(!pipeline.suppression().is_suppressed());
    assert_eq!(suppress_calls(&extension), 0);

    // One event over the real bus is the evidence.
    extension.swipe(0, 3, 0.0, 0.0, 0);
    pipeline.run(&mut listening);
    assert!(pipeline.has_seen_events());
    assert!(pipeline.suppression().is_suppressed());
    assert_eq!(suppress_calls(&extension), 1, "{:?}", extension.calls());

    for event in [
        SuppressionEvent::Restored,
        SuppressionEvent::Disabled,
        SuppressionEvent::SafeMode,
        SuppressionEvent::Uninstalled,
    ] {
        let mut listening = events(&bus);
        let mut pipeline = GesturePipeline::new(
            three_finger_overview(),
            fixture.store(),
            Box::new(adapter(&bus)),
        );
        pipeline.verify();
        extension.swipe(0, 3, 0.0, 0.0, 0);
        pipeline.run(&mut listening);
        assert!(pipeline.suppression().is_suppressed(), "{}", event.key());
        pipeline.restore_built_in_gestures(event);
        assert!(!pipeline.suppression().is_suppressed(), "{}", event.key());
        assert_eq!(
            extension.calls().last(),
            Some(&"SuppressBuiltInGestures(false)".to_string()),
            "{}",
            event.key()
        );
    }
}

#[test]
fn three_failed_gestures_in_a_row_turn_the_integration_off_and_write_that_down() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, true);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    // The plan is built against a working adapter, so the configuration under
    // test is the one a user would have applied; the pipeline then runs against
    // an extension whose every call fails.
    let mut good = GnomeShellAdapter::with_reported(
        Box::new(touchpad_session::FakeShellBridge::new()),
        touchpad_session::FakeShellBridge::gnome_46_capabilities(),
    );
    let config = fixture.applied_preset(&mut good);
    let store = fixture.store();
    let mut pipeline = GesturePipeline::new(
        config,
        fixture.store(),
        Box::new(GnomeShellAdapter::with_reported(
            Box::new(shell(&bus)),
            touchpad_session::FakeShellBridge::gnome_46_capabilities(),
        )),
    );

    for round in 0..3 {
        swipe(&extension, 0.0, -WHOLE_SWIPE, round * 2_000, 8);
    }
    assert_eq!(pipeline.run(&mut listening), StopReason::AutoDisabled);
    assert!(!pipeline.is_enabled());
    assert_eq!(
        pipeline.problem(),
        Some("gestures.adapter_disabled_after_failures:3")
    );
    // And the window will open on the same state, because it is in the file.
    assert!(!store.load_config().expect("a saved configuration").enabled);
    // Better Manager's Home learns why from the marker beside it.
    assert!(store.turned_off_by_itself());
}

#[test]
fn verification_results_are_written_where_the_window_reads_them() {
    let bus = bus_or_skip!();
    let _extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let store = fixture.store();
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    // Overview is verified against the real bus; the windows of the current
    // application are not, and the stored reason says why.
    assert_eq!(pipeline.verify(), RunState::PartiallySupported);
    let stored = store.load_config().expect("a saved configuration");
    let overview = stored
        .get(&touchpad_gestures::GestureId::new("overview").unwrap())
        .unwrap();
    assert!(matches!(
        overview.last_verification,
        touchpad_gestures::VerificationRecord::Verified { .. }
    ));
    let windows = stored
        .get(&touchpad_gestures::GestureId::new("current-app-windows").unwrap())
        .unwrap();
    match &windows.last_verification {
        touchpad_gestures::VerificationRecord::Unsupported { reason, .. } => {
            assert_eq!(reason, "gnome.no_per_application_window_picker");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_signal_this_build_cannot_read_is_dropped_rather_than_turned_into_a_gesture() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    // A phase number no version of the interface defines, and a contact count
    // no hand has.
    extension.swipe(9, 4, 0.0, -WHOLE_SWIPE, 0);
    extension.swipe(1, 40_000, 0.0, -WHOLE_SWIPE, 16);
    pipeline.run(&mut listening);

    assert!(pipeline.performed().is_empty());
    assert!(
        extension
            .calls()
            .iter()
            .all(|call| call.starts_with("SuppressBuiltInGestures")),
        "{:?}",
        extension.calls()
    );
}

// ── Which pad ───────────────────────────────────────────────────────────────

/// One device under a temporary `/sys`, laid out the way the kernel does it.
fn plug(sys: &std::path::Path, handler: &str, uniq: &str, name: &str) {
    let device = sys.join("class/input").join(handler).join("device");
    std::fs::create_dir_all(device.join("id")).expect("a device directory");
    std::fs::write(device.join("name"), format!("{name}\n")).expect("a name");
    std::fs::write(device.join("uniq"), format!("{uniq}\n")).expect("a uniq");
    for (file, value) in [
        ("bustype", "0005"),
        ("vendor", "05ac"),
        ("product", "0265"),
        ("version", "0001"),
    ] {
        std::fs::write(device.join("id").join(file), format!("{value}\n")).expect("an id");
    }
}

/// The device argument crosses the real bus and picks the pad's own profile.
#[test]
fn the_device_named_on_the_wire_picks_that_pads_own_profile() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let sys = fixture.root.path().join("sys");
    plug(&sys, "event7", "a0:b1:c2:d3:e4:f5", "Magic Trackpad");

    let mut setup = adapter(&bus);
    let global = fixture.applied_preset(&mut setup);
    let mut trackpad = global.clone();
    // The trackpad's own profile turns the overview gesture off, so the same
    // swipe does something on the global profile and nothing on this one.
    trackpad
        .get_mut(&touchpad_gestures::GestureId::new("overview").unwrap())
        .expect("the preset's overview gesture")
        .enabled = false;
    let mut profiles = touchpad_gestures::GestureProfiles::global_only(global);
    profiles
        .devices
        .insert("uniq:a0:b1:c2:d3:e4:f5".to_string(), trackpad);

    let mut pipeline = GesturePipeline::with_profiles(
        profiles,
        fixture.store(),
        Box::new(adapter(&bus)),
        Box::new(touchpad_gesture_service::SysfsDeviceIdentities::at(&sys)),
    );

    // The trackpad first: its profile has no overview gesture.
    extension.swipe_on("/dev/input/event7", 0, 4, 0.0, 0.0, 0);
    for step in 1..=8 {
        let phase = if step == 8 { 2 } else { 1 };
        extension.swipe_on(
            "/dev/input/event7",
            phase,
            4,
            0.0,
            -WHOLE_SWIPE / 8.0,
            step * 16,
        );
    }
    // Then a pad the shell could not name, on the global profile.
    swipe(&extension, 0.0, -WHOLE_SWIPE, 2_000, 8);
    pipeline.run(&mut listening);

    assert_eq!(
        extension
            .calls()
            .iter()
            .filter(|call| *call == "ShowOverview")
            .count(),
        1,
        "{:?}",
        extension.calls()
    );
    let completed: Vec<Option<String>> = pipeline
        .performed()
        .iter()
        .filter(|one| one.outcome == InvocationOutcome::Invoked)
        .map(|one| one.profile.clone())
        .collect();
    assert_eq!(completed, vec![None]);
}

#[test]
fn a_protocol_1_extension_still_works_on_the_global_profile() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();
    let mut listening = events(&bus);

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    extension.protocol_1_swipe(0, 4, 0.0, 0.0, 0);
    for step in 1..=8 {
        let phase = if step == 8 { 2 } else { 1 };
        extension.protocol_1_swipe(phase, 4, 0.0, -WHOLE_SWIPE / 8.0, step * 16);
    }
    pipeline.run(&mut listening);

    assert!(
        extension.calls().contains(&"ShowOverview".to_string()),
        "{:?}",
        extension.calls()
    );
}
