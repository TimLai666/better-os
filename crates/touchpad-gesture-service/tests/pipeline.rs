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
    GnomeShellAdapter, InvocationOutcome, SessionAdapter, SessionBusShell, ShellEvents,
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

    async fn suppress_built_in_gestures(&self, suppress: bool) -> Result<(), zbus::fdo::Error> {
        self.record(&format!("SuppressBuiltInGestures({suppress})"))
    }

    async fn capabilities(&self) -> String {
        // Word for word what the GJS extension reports on GNOME 46.
        r#"{"protocol_version":1,"shell_version":"46.0","finger_count":true,
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
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn pinch_gesture(
        emitter: &SignalEmitter<'_>,
        phase: u32,
        fingers: u32,
        scale: f64,
        angle_delta: f64,
        at_ms: u64,
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

    fn swipe(&self, phase: u32, fingers: u32, dx: f64, dy: f64, at_ms: u64) {
        self.runtime
            .block_on(FakeExtension::swipe_gesture(
                &self.emitter(),
                phase,
                fingers,
                dx,
                dy,
                at_ms,
            ))
            .expect("a swipe signal");
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
            ))
            .expect("a pinch signal");
    }

    fn calls(&self) -> Vec<String> {
        self.recorded.lock().expect("recorded calls").calls.clone()
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

#[test]
fn a_confirmed_plan_asks_the_desktop_to_give_up_its_own_gestures_and_gives_them_back() {
    let bus = bus_or_skip!();
    let extension = Extension::serve(&bus, false);
    let fixture = Fixture::new();

    let mut setup = adapter(&bus);
    let config = fixture.applied_preset(&mut setup);
    let mut pipeline = GesturePipeline::new(config, fixture.store(), Box::new(adapter(&bus)));

    pipeline.suppress_built_in_gestures();
    assert!(pipeline.suppression().is_suppressed());
    assert!(
        extension
            .calls()
            .contains(&"SuppressBuiltInGestures(true)".to_string()),
        "{:?}",
        extension.calls()
    );

    for event in [
        SuppressionEvent::Restored,
        SuppressionEvent::Disabled,
        SuppressionEvent::SafeMode,
        SuppressionEvent::Uninstalled,
    ] {
        let mut pipeline = GesturePipeline::new(
            fixture.applied_preset(&mut setup),
            fixture.store(),
            Box::new(adapter(&bus)),
        );
        pipeline.suppress_built_in_gestures();
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
