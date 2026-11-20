//! The service around the pipeline (ticket 131): waiting for the adapter,
//! starting and stopping with it, and the rule that GNOME's own gestures are
//! never taken while ours are not working.
//!
//! The session is scripted and the adapter is the shipped GNOME Shell adapter
//! over a recording bridge, so every call that would have reached the shell is
//! asserted, and nothing here needs a bus.

use std::collections::VecDeque;
use std::sync::Arc;

use better_actions::DesktopAction;
use touchpad_gesture_service::{
    GesturePipeline, NoDeviceIdentities, ServiceExit, ServiceSession, serve,
};
use touchpad_gestures::{
    Direction, GestureConfig, GestureDefinition, GestureProfiles, GestureShape, GestureStore,
    PresetId, mac_style,
};
use touchpad_session::gnome::{FakeShellBridge, SharedShellBridge, ShellError, ShellRequest};
use touchpad_session::{GnomeShellAdapter, SessionAdapter, SessionEvent, ShellGestureEvent};

/// A session that replays a script, with the adapter present or not at the
/// start, and a recording bridge behind every adapter it hands out.
struct ScriptedSession {
    present: bool,
    events: VecDeque<SessionEvent>,
    bridges: Vec<Arc<FakeShellBridge>>,
    failing: bool,
}

impl ScriptedSession {
    fn new(present: bool, events: Vec<SessionEvent>) -> Self {
        Self {
            present,
            events: events.into(),
            bridges: Vec::new(),
            failing: false,
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
        Ok(Box::new(GnomeShellAdapter::with_reported(
            Box::new(SharedShellBridge(bridge)),
            FakeShellBridge::gnome_46_capabilities(),
        )))
    }

    fn next_event(&mut self) -> SessionEvent {
        self.events.pop_front().unwrap_or(SessionEvent::Closed)
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

struct Run {
    exit: ServiceExit,
    session: ScriptedSession,
    journal: Vec<String>,
}

fn run(config: GestureConfig, mut session: ScriptedSession) -> Run {
    let root = tempfile::tempdir().unwrap();
    let store_root = root.path().to_path_buf();
    let mut pipeline_for = |adapter: Box<dyn SessionAdapter>| {
        Ok(GesturePipeline::with_profiles(
            GestureProfiles::global_only(config.clone()),
            GestureStore::at(&store_root),
            adapter,
            Box::new(NoDeviceIdentities),
        ))
    };
    let mut journal = Vec::new();
    let exit = serve(&mut session, &mut pipeline_for, &mut |line| {
        journal.push(line)
    });
    Run {
        exit,
        session,
        journal,
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
    assert_eq!(run.exit, ServiceExit::TurnedOff { restored: true });
    assert!(mentions(&run.journal, "gestures turned off"));
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
