//! The service around the pipeline (ticket 131): waiting for the adapter,
//! starting and stopping with it, and the rule that GNOME's own gestures are
//! never taken while ours are not working.
//!
//! The session is scripted and the adapter is the shipped GNOME Shell adapter
//! over a recording bridge, so every call that would have reached the shell is
//! asserted, and nothing here needs a bus.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use better_actions::DesktopAction;
use touchpad_core::TouchpadStore;
use touchpad_gesture_service::{
    GesturePipeline, NoDeviceIdentities, ServiceExit, ServiceSession, read_configuration, serve,
};
use touchpad_gestures::{
    Direction, GestureConfig, GestureDefinition, GestureProfiles, GestureShape, GestureStore,
    PresetId, mac_style,
};
use touchpad_session::gnome::{FakeShellBridge, SharedShellBridge, ShellError, ShellRequest};
use touchpad_session::{GnomeShellAdapter, SessionAdapter, SessionEvent, ShellGestureEvent};

/// One step of a scripted session.
enum Step {
    /// The event, as the bus would deliver it.
    Event(SessionEvent),
    /// A window saves this global profile, and the watch reports it.
    Save(GestureConfig),
    /// A window saves this global profile, and the service has not been told
    /// yet: the race a stale copy would lose.
    SaveUnannounced(GestureConfig),
    /// Safe mode is turned on or off, and the watch reports it.
    SafeMode(bool),
}

/// A session that replays a script, with the adapter present or not at the
/// start, and a recording bridge behind every adapter it hands out.
struct ScriptedSession {
    present: bool,
    steps: VecDeque<Step>,
    bridges: Vec<Arc<FakeShellBridge>>,
    failing: bool,
    /// What the adapter reports about GNOME's gestures when it is connected.
    reported_suppressed: bool,
    store: Option<PathBuf>,
}

impl ScriptedSession {
    fn new(present: bool, events: Vec<SessionEvent>) -> Self {
        Self::with_steps(present, events.into_iter().map(Step::Event).collect())
    }

    fn with_steps(present: bool, steps: Vec<Step>) -> Self {
        Self {
            present,
            steps: steps.into(),
            bridges: Vec::new(),
            failing: false,
            reported_suppressed: false,
            store: None,
        }
    }

    /// Every call any adapter this session handed out received, in order.
    fn calls(&self) -> Vec<ShellRequest> {
        self.bridges
            .iter()
            .flat_map(|bridge| bridge.calls())
            .collect()
    }

    fn suppressions(&self) -> Vec<bool> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                ShellRequest::SuppressBuiltInGestures(on) => Some(on),
                _ => None,
            })
            .collect()
    }

    fn gestures(&self) -> GestureStore {
        GestureStore::at(self.store.as_ref().expect("a store"))
    }
}

impl ServiceSession for ScriptedSession {
    fn adapter_present(&mut self) -> bool {
        self.present
    }

    fn connect_adapter(&mut self) -> Result<Box<dyn SessionAdapter>, String> {
        let bridge = Arc::new(if self.failing {
            FakeShellBridge::failing(ShellError::CallFailed("the shell did not answer".into()))
        } else {
            FakeShellBridge::new()
        });
        self.bridges.push(bridge.clone());
        let mut reported = FakeShellBridge::gnome_46_capabilities();
        reported.built_in_gestures_suppressed = self.reported_suppressed;
        Ok(Box::new(GnomeShellAdapter::with_reported(
            Box::new(SharedShellBridge(bridge)),
            reported,
        )))
    }

    fn next_event(&mut self) -> SessionEvent {
        loop {
            let Some(step) = self.steps.pop_front() else {
                return SessionEvent::Closed;
            };
            match step {
                Step::Event(event) => return event,
                Step::Save(config) => {
                    self.gestures().save_config(&config).unwrap();
                    return SessionEvent::ConfigurationChanged;
                }
                Step::SaveUnannounced(config) => {
                    self.gestures().save_config(&config).unwrap();
                }
                Step::SafeMode(on) => {
                    let settings = TouchpadStore::new(self.store.as_ref().unwrap());
                    if on {
                        settings.enable_safe_mode().unwrap();
                    } else {
                        settings.clear_safe_mode().unwrap();
                    }
                    return SessionEvent::ConfigurationChanged;
                }
            }
        }
    }
}

/// A gesture that takes one of GNOME's: three fingers up, which GNOME 46's
/// overview tracker answers.
fn three_finger_overview() -> GestureConfig {
    let gesture = GestureDefinition::new(
        "three-up",
        GestureShape::Swipe,
        3,
        false,
        Some(Direction::Up),
        DesktopAction::ShowOverview,
    )
    .unwrap();
    GestureConfig::with_gestures(vec![gesture], PresetId::Custom)
}

fn swipe(phase: u32, fingers: u32, dy: f64, at_ms: u64) -> SessionEvent {
    SessionEvent::Gesture(ShellGestureEvent::Swipe {
        phase,
        fingers,
        dx: 0.0,
        dy,
        at_ms,
        device: None,
    })
}

/// One whole four-finger swipe up, far enough to commit.
fn four_finger_swipe_up(from_ms: u64) -> Vec<SessionEvent> {
    let mut events = vec![swipe(0, 4, 0.0, from_ms)];
    for step in 1..=8 {
        events.push(swipe(1, 4, -40.0, from_ms + step * 16));
    }
    events.push(swipe(2, 4, 0.0, from_ms + 9 * 16));
    events
}

fn events(events: Vec<SessionEvent>) -> Vec<Step> {
    events.into_iter().map(Step::Event).collect()
}

struct Run {
    exit: ServiceExit,
    session: ScriptedSession,
    journal: Vec<String>,
    /// The global profile as the file has it at the end.
    stored: GestureConfig,
    _root: tempfile::TempDir,
}

/// Runs the real `serve` against the real store in a scratch directory, with
/// `config` saved there first the way a window would have saved it.
fn run(config: GestureConfig, mut session: ScriptedSession) -> Run {
    let root = tempfile::tempdir().unwrap();
    let store_root = root.path().to_path_buf();
    session.store = Some(store_root.clone());
    let gestures = GestureStore::at(&store_root);
    gestures.save_config(&config).unwrap();
    let settings = TouchpadStore::new(&store_root);
    let mut configuration = || read_configuration(&settings, &gestures);
    let mut pipeline_for = |adapter: Box<dyn SessionAdapter>, profiles: GestureProfiles| {
        GesturePipeline::with_profiles(
            profiles,
            GestureStore::at(&store_root),
            adapter,
            Box::new(NoDeviceIdentities),
        )
    };
    let mut journal = Vec::new();
    let exit = serve(
        &mut session,
        &mut configuration,
        &mut pipeline_for,
        &mut |line| journal.push(line),
    );
    let stored = GestureStore::at(&store_root).load_config().unwrap();
    Run {
        exit,
        session,
        journal,
        stored,
        _root: root,
    }
}

fn mentions(journal: &[String], words: &str) -> bool {
    journal.iter().any(|line| line.contains(words))
}

#[test]
fn no_events_no_suppression() {
    let run = run(
        three_finger_overview(),
        ScriptedSession::new(true, vec![SessionEvent::Stop]),
    );
    assert_eq!(run.exit, ServiceExit::Stopped { restored: true });
    assert!(
        run.session.suppressions().is_empty(),
        "GNOME's gestures were taken before anything arrived: {:?}",
        run.session.calls()
    );
    assert!(
        mentions(
            &run.journal,
            "GNOME keeps its own gestures until a gesture arrives"
        ),
        "{:?}",
        run.journal
    );
}

#[test]
fn the_first_gesture_from_the_adapter_is_the_evidence_suppression_waits_for() {
    let run = run(
        three_finger_overview(),
        ScriptedSession::new(true, vec![swipe(0, 3, 0.0, 0), SessionEvent::Stop]),
    );
    // Taken once the event path showed it works, and given back on the stop.
    assert_eq!(run.session.suppressions(), vec![true, false]);
    assert_eq!(run.exit, ServiceExit::Stopped { restored: true });
    assert!(mentions(&run.journal, "built-in gestures suppressed"));
    assert!(mentions(&run.journal, "built-in gestures restored"));
}

#[test]
fn the_mac_style_preset_never_takes_gnomes_gestures() {
    let mut events = four_finger_swipe_up(0);
    events.push(SessionEvent::Stop);
    let run = run(mac_style(), ScriptedSession::new(true, events));
    assert!(
        run.session.suppressions().is_empty(),
        "{:?}",
        run.session.calls()
    );
    // And it still performs: four fingers up is the overview.
    assert!(
        run.session.calls().contains(&ShellRequest::ShowOverview),
        "{:?}",
        run.session.calls()
    );
}

#[test]
fn a_colliding_gesture_that_cannot_be_performed_takes_nothing() {
    let gesture = GestureDefinition::new(
        "three-down",
        GestureShape::Swipe,
        3,
        false,
        Some(Direction::Down),
        // GNOME 46 has no facility for it, so it verifies as unsupported.
        DesktopAction::CurrentApplicationWindows,
    )
    .unwrap();
    let config = GestureConfig::with_gestures(vec![gesture], PresetId::Custom);
    let run = run(
        config,
        ScriptedSession::new(true, vec![swipe(0, 3, 0.0, 0), SessionEvent::Stop]),
    );
    assert!(run.session.suppressions().is_empty());
}

#[test]
fn a_missing_adapter_is_waited_for_rather_than_a_reason_to_exit() {
    let run = run(
        three_finger_overview(),
        ScriptedSession::new(
            false,
            vec![
                // A gesture before the adapter is there belongs to nothing.
                swipe(0, 3, 0.0, 0),
                SessionEvent::AdapterAppeared,
                swipe(0, 3, 0.0, 100),
                SessionEvent::Stop,
            ],
        ),
    );
    assert_eq!(run.session.bridges.len(), 1);
    assert_eq!(run.session.suppressions(), vec![true, false]);
    assert!(mentions(
        &run.journal,
        "waiting for the GNOME Shell adapter"
    ));
    assert!(mentions(&run.journal, "appeared after"));
}

#[test]
fn the_adapter_vanishing_ends_the_pipeline_and_the_service_waits_again() {
    let run = run(
        three_finger_overview(),
        ScriptedSession::new(
            true,
            vec![
                swipe(0, 3, 0.0, 0),
                SessionEvent::AdapterVanished,
                // Nothing is recognized while it is gone.
                swipe(0, 3, 0.0, 50),
                SessionEvent::AdapterAppeared,
                SessionEvent::Stop,
            ],
        ),
    );
    assert_eq!(run.session.bridges.len(), 2);
    // The first adapter took them after its first gesture; nothing called the
    // one that left, because the extension restores on its own way out.
    assert_eq!(
        run.session.bridges[0].calls(),
        vec![ShellRequest::SuppressBuiltInGestures(true)]
    );
    // The second has seen no gesture yet, so it takes nothing.
    assert!(
        run.session.bridges[1]
            .calls()
            .iter()
            .all(|call| !matches!(call, ShellRequest::SuppressBuiltInGestures(true))),
        "{:?}",
        run.session.bridges[1].calls()
    );
    assert!(mentions(&run.journal, "left the session bus"));
    assert!(mentions(
        &run.journal,
        "waiting for the GNOME Shell adapter extension to come back"
    ));
    assert_eq!(run.exit, ServiceExit::Stopped { restored: true });
}

#[test]
fn the_service_stopping_gives_gnome_its_gestures_back() {
    for (last, expected) in [
        (SessionEvent::Stop, ServiceExit::Stopped { restored: true }),
        (SessionEvent::Closed, ServiceExit::Closed { restored: true }),
    ] {
        let run = run(
            three_finger_overview(),
            ScriptedSession::new(true, vec![swipe(0, 3, 0.0, 0), last]),
        );
        assert_eq!(run.exit, expected);
        assert_eq!(
            run.session.calls().last(),
            Some(&ShellRequest::SuppressBuiltInGestures(false))
        );
    }
}

#[test]
fn a_failing_adapter_turns_gestures_off_and_gives_gnome_its_own_back() {
    let mut events = Vec::new();
    for round in 0..3 {
        events.extend(four_finger_swipe_up(round * 2_000));
    }
    events.push(SessionEvent::Stop);
    let mut session = ScriptedSession::new(true, events);
    session.failing = true;
    let run = run(mac_style(), session);
    // It turns gestures off and stays, rather than exiting into a state
    // Better Manager would read as finished on purpose.
    assert_eq!(run.exit, ServiceExit::Stopped { restored: true });
    assert!(mentions(&run.journal, "gestures turned off"));
    assert!(mentions(&run.journal, "the service stays running"));
    assert!(!run.stored.enabled);
}

/// The automatic disable writes the switch and nothing else, on the file as
/// it is now: a gesture a window saved after the service read the file
/// survives it.
#[test]
fn the_automatic_disable_never_writes_a_stale_copy_over_the_window() {
    let mut steps = vec![Step::Event(swipe(0, 4, 0.0, 0))];
    let mut saved = mac_style();
    saved.gestures.push(
        GestureDefinition::new(
            "mine",
            GestureShape::Swipe,
            5,
            false,
            Some(Direction::Up),
            DesktopAction::ShowOverview,
        )
        .unwrap(),
    );
    steps.push(Step::SaveUnannounced(saved));
    for round in 0..3 {
        steps.extend(events(four_finger_swipe_up(1_000 + round * 2_000)));
    }
    steps.push(Step::Event(SessionEvent::Stop));
    let mut session = ScriptedSession::with_steps(true, steps);
    session.failing = true;
    let run = run(mac_style(), session);
    assert!(
        mentions(&run.journal, "gestures turned off"),
        "{:?}",
        run.journal
    );
    assert!(!run.stored.enabled);
    assert!(
        run.stored
            .gestures
            .iter()
            .any(|gesture| gesture.id.as_str() == "mine"),
        "the window's gesture was overwritten"
    );
}

/// Turning gestures off in a window stops them at once, and turning them on
/// again starts them, with no restart and no login (ticket 131).
#[test]
fn a_window_turning_gestures_off_and_on_takes_effect_at_once() {
    let mut off = mac_style();
    off.enabled = false;
    let mut steps = vec![Step::Save(off)];
    steps.extend(events(four_finger_swipe_up(0)));
    steps.push(Step::Save(mac_style()));
    steps.extend(events(four_finger_swipe_up(2_000)));
    steps.push(Step::Event(SessionEvent::Stop));
    let run = run(mac_style(), ScriptedSession::with_steps(true, steps));
    let overviews = run
        .session
        .calls()
        .iter()
        .filter(|call| **call == ShellRequest::ShowOverview)
        .count();
    assert_eq!(overviews, 1, "{:?}", run.journal);
    assert!(mentions(&run.journal, "no gesture is switched on"));
    assert!(mentions(&run.journal, "gestures are configured"));
}

/// A gesture added in a window is recognized without a restart, and one that
/// collides takes GNOME's gestures only once a gesture has arrived; removing
/// it gives them back.
#[test]
fn a_colliding_gesture_added_and_removed_in_a_window_takes_and_gives_back() {
    let steps = vec![
        Step::Save(three_finger_overview()),
        Step::Event(swipe(0, 3, 0.0, 0)),
        Step::Save(mac_style()),
        Step::Event(SessionEvent::Stop),
    ];
    let run = run(mac_style(), ScriptedSession::with_steps(true, steps));
    assert_eq!(
        run.session.suppressions(),
        vec![true, false],
        "{:?}",
        run.journal
    );
}

/// Nothing configured is not a reason to exit: the service waits, and a
/// gesture configured later works.
#[test]
fn with_nothing_configured_the_service_stays_and_starts_when_one_is() {
    let mut steps = vec![Step::Save(mac_style())];
    steps.extend(events(four_finger_swipe_up(0)));
    steps.push(Step::Event(SessionEvent::Stop));
    let run = run(
        GestureConfig::with_gestures(Vec::new(), PresetId::Custom),
        ScriptedSession::with_steps(true, steps),
    );
    assert!(mentions(&run.journal, "waiting for one to be configured"));
    assert!(run.session.calls().contains(&ShellRequest::ShowOverview));
}

/// Safe mode keeps the service resident and recognizing nothing, gives
/// GNOME's gestures back, and leaving it starts gestures again.
#[test]
fn safe_mode_recognizes_nothing_gives_gnome_back_and_ends_by_itself() {
    let mut steps = vec![Step::Event(swipe(0, 3, 0.0, 0)), Step::SafeMode(true)];
    steps.extend(events(four_finger_swipe_up(1_000)));
    steps.push(Step::SafeMode(false));
    steps.push(Step::Event(SessionEvent::Stop));
    let mut config = three_finger_overview();
    config.gestures.extend(mac_style().gestures);
    let run = run(config, ScriptedSession::with_steps(true, steps));
    assert!(mentions(&run.journal, "safe mode is on"));
    // Taken after the first gesture, given back on safe mode, taken again
    // when it ends (gestures have arrived already), and given back at stop.
    assert_eq!(
        run.session.suppressions(),
        vec![true, false, true, false],
        "{:?}",
        run.journal
    );
    assert!(
        !run.session.calls().contains(&ShellRequest::ShowOverview),
        "a gesture was performed in safe mode"
    );
}

/// After an upgrade the shell still runs the old extension, which kept
/// GNOME's gestures off after the old service was gone. The new service
/// reads what the adapter reports and gives them back (ticket 131).
#[test]
fn a_suppression_left_by_an_older_service_is_lifted_when_the_adapter_attaches() {
    let mut session = ScriptedSession::new(true, vec![SessionEvent::Stop]);
    session.reported_suppressed = true;
    let run = run(mac_style(), session);
    assert_eq!(run.session.suppressions(), vec![false]);
    assert!(mentions(&run.journal, "from before this service started"));
}

#[test]
fn every_gesture_and_action_is_one_journal_line() {
    let mut events = four_finger_swipe_up(0);
    events.push(SessionEvent::Stop);
    let run = run(mac_style(), ScriptedSession::new(true, events));
    let began: Vec<&String> = run
        .journal
        .iter()
        .filter(|line| line.starts_with("gesture began"))
        .collect();
    assert_eq!(began.len(), 1, "{:?}", run.journal);
    assert_eq!(
        began[0],
        "gesture began: swipe with 4 fingers on an unknown device, the global profile"
    );
    assert!(mentions(
        &run.journal,
        "gesture ended: swipe with 4 fingers"
    ));
    assert!(
        mentions(
            &run.journal,
            "performed overview.show for overview (the global profile): done"
        ),
        "{:?}",
        run.journal
    );
    assert!(mentions(&run.journal, "gestures verified"));
}

/// Writes `contents` to `path` from a child process, the way the store does
/// it (a file beside it, renamed over it). A child writes it so this process
/// never holds a write descriptor a forking test thread could carry into a
/// late close-write (AGENTS.md).
fn child_writes(path: &std::path::Path, contents: &str) {
    let status = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            "printf %s \"$1\" > \"$2.partial\" && mv \"$2.partial\" \"$2\"",
            "sh",
            contents,
        ])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

/// The watch the service runs on: a write to the gesture configuration or the
/// safe mode marker is reported, a write to anything else in the directory is
/// not.
#[test]
fn the_configuration_watch_reports_the_two_files_and_nothing_else() {
    use std::time::Duration;
    let root = tempfile::tempdir().unwrap();
    let gestures = root.path().join("gestures.json");
    let safe_mode = root.path().join("safe-mode");
    let (sender, receiver) = std::sync::mpsc::channel();
    let _watch = touchpad_gesture_service::ConfigurationWatch::start(
        &[gestures.clone(), safe_mode.clone()],
        sender,
    )
    .unwrap();
    let drain = || while receiver.recv_timeout(Duration::from_millis(300)).is_ok() {};

    child_writes(&root.path().join("config.json"), "{}");
    drain();

    child_writes(&gestures, "{}");
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(5)),
        Ok(SessionEvent::ConfigurationChanged)
    );
    drain();

    child_writes(&safe_mode, "on");
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(5)),
        Ok(SessionEvent::ConfigurationChanged)
    );
    drain();

    child_writes(&root.path().join("config.json"), "{\"x\":1}");
    assert!(
        receiver.recv_timeout(Duration::from_millis(500)).is_err(),
        "a write to another file was reported"
    );
}
