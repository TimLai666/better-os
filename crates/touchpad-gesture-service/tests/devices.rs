//! Two touchpads, each with its own gesture profile.
//!
//! ADR 0015 lets the GNOME Shell adapter name the device node behind each
//! gesture. These tests drive the shipped pipeline with the Rust fake of that
//! adapter — a recorded event stream in, a recording shell bridge out — and a
//! temporary directory standing in for `/sys`, so the device a node resolves to
//! is read the way the service reads it on a real machine.
//!
//! The pads:
//!
//! - `event5` is a laptop pad with no `Uniq`, filed under its bus, vendor,
//!   product, version, and name. Its own profile maps four fingers up to Show
//!   Desktop.
//! - `event7` is a Bluetooth pad with a `Uniq`. Its own profile maps four
//!   fingers up to the next workspace.
//! - `event9` is a third pad the kernel knows and the configuration does not.
//!
//! The global profile maps four fingers up to the overview.

use std::path::Path;
use std::sync::Arc;

use better_actions::DesktopAction;
use touchpad_gesture_service::{
    DeviceIdentities, GesturePipeline, NoDeviceIdentities, SysfsDeviceIdentities,
};
use touchpad_gestures::{
    Direction, GestureConfig, GestureDefinition, GestureEventKind, GestureProfiles, GestureShape,
    GestureStore, PresetId,
};
use touchpad_session::gnome::{InputDeviceNode, SharedShellBridge};
use touchpad_session::{
    FakeShellBridge, GnomeShellAdapter, InvocationOutcome, RecordedShellEvents, ShellGestureEvent,
    ShellRequest, WorkspaceDirection,
};

const LAPTOP: &str = "input:0018:06cb:ce67:0100:SynPS/2 Synaptics TouchPad";
const TRACKPAD: &str = "uniq:a0:b1:c2:d3:e4:f5";
const UNPROFILED: &str = "input:0003:046d:4069:0111:Logitech T650";

const LAPTOP_NODE: InputDeviceNode = InputDeviceNode::new(5);
const TRACKPAD_NODE: InputDeviceNode = InputDeviceNode::new(7);
const UNPROFILED_NODE: InputDeviceNode = InputDeviceNode::new(9);

/// One whole swipe under the shipped scales: 0.18 of the pad at 1000 pixels to
/// the pad.
const WHOLE_SWIPE: f64 = 180.0;

// ── The machine ─────────────────────────────────────────────────────────────

/// Writes one input device the way the kernel lays it out under
/// `/sys/class/input/event<N>/device`.
fn plug(sys: &Path, handler: &str, uniq: &str, id: [&str; 4], name: &str) -> std::io::Result<()> {
    let device = sys.join("class/input").join(handler).join("device");
    std::fs::create_dir_all(device.join("id"))?;
    std::fs::write(device.join("name"), format!("{name}\n"))?;
    std::fs::write(device.join("uniq"), format!("{uniq}\n"))?;
    for (file, value) in ["bustype", "vendor", "product", "version"]
        .into_iter()
        .zip(id)
    {
        std::fs::write(device.join("id").join(file), format!("{value}\n"))?;
    }
    Ok(())
}

struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        let sys = root.path().join("sys");
        plug(
            &sys,
            "event5",
            "",
            ["0018", "06cb", "ce67", "0100"],
            "SynPS/2 Synaptics TouchPad",
        )
        .expect("the laptop pad");
        plug(
            &sys,
            "event7",
            "a0:b1:c2:d3:e4:f5",
            ["0005", "05ac", "0265", "0001"],
            "Magic Trackpad",
        )
        .expect("the bluetooth pad");
        plug(
            &sys,
            "event9",
            "",
            ["0003", "046d", "4069", "0111"],
            "Logitech T650",
        )
        .expect("the unprofiled pad");
        Self { root }
    }

    fn identities(&self) -> SysfsDeviceIdentities {
        SysfsDeviceIdentities::at(self.root.path().join("sys"))
    }

    fn store(&self) -> GestureStore {
        GestureStore::at(self.root.path().join("touchpad"))
    }
}

fn swipe_up(action: DesktopAction) -> GestureConfig {
    GestureConfig::with_gestures(
        vec![
            GestureDefinition::new(
                "four-up",
                GestureShape::Swipe,
                4,
                false,
                Some(Direction::Up),
                action,
            )
            .expect("a gesture"),
        ],
        PresetId::Custom,
    )
}

fn profiles() -> GestureProfiles {
    let mut profiles = GestureProfiles::global_only(swipe_up(DesktopAction::ShowOverview));
    profiles
        .devices
        .insert(LAPTOP.to_string(), swipe_up(DesktopAction::ShowDesktop));
    profiles
        .devices
        .insert(TRACKPAD.to_string(), swipe_up(DesktopAction::NextWorkspace));
    profiles
}

fn pipeline(
    machine: &Machine,
    profiles: GestureProfiles,
    identities: Box<dyn DeviceIdentities>,
) -> (GesturePipeline, Arc<FakeShellBridge>) {
    let bridge = Arc::new(FakeShellBridge::new());
    let adapter = GnomeShellAdapter::connect(Box::new(SharedShellBridge(bridge.clone())))
        .expect("the fake extension answers Capabilities");
    let pipeline =
        GesturePipeline::with_profiles(profiles, machine.store(), Box::new(adapter), identities);
    (pipeline, bridge)
}

// ── Gesture streams ─────────────────────────────────────────────────────────

fn event(phase: u32, dy: f64, at_ms: u64, device: Option<InputDeviceNode>) -> ShellGestureEvent {
    ShellGestureEvent::Swipe {
        phase,
        fingers: 4,
        dx: 0.0,
        dy,
        at_ms,
        device,
    }
}

/// A whole four-finger swipe up, as the compositor reports it.
fn whole_swipe(device: Option<InputDeviceNode>, from_ms: u64) -> Vec<ShellGestureEvent> {
    let mut events = vec![event(0, 0.0, from_ms, device)];
    for step in 1..=8 {
        let phase = if step == 8 { 2 } else { 1 };
        events.push(event(
            phase,
            -WHOLE_SWIPE / 8.0,
            from_ms + step * 16,
            device,
        ));
    }
    events
}

fn run(pipeline: &mut GesturePipeline, events: Vec<ShellGestureEvent>) {
    pipeline.run(&mut RecordedShellEvents::new(events));
}

/// The profile each completed gesture was recognized against, in order.
fn completed_profiles(pipeline: &GesturePipeline) -> Vec<Option<String>> {
    pipeline
        .performed()
        .iter()
        .filter(|one| one.outcome == InvocationOutcome::Invoked)
        .map(|one| one.profile.clone())
        .collect()
}

// ── The tests ───────────────────────────────────────────────────────────────

#[test]
fn each_pad_performs_its_own_profile() {
    let machine = Machine::new();
    let (mut pipeline, shell) = pipeline(&machine, profiles(), Box::new(machine.identities()));

    let mut events = whole_swipe(Some(LAPTOP_NODE), 0);
    events.extend(whole_swipe(Some(TRACKPAD_NODE), 2_000));
    events.extend(whole_swipe(Some(LAPTOP_NODE), 4_000));
    run(&mut pipeline, events);

    assert_eq!(
        shell.calls(),
        vec![
            ShellRequest::ShowDesktop,
            ShellRequest::SwitchWorkspace(WorkspaceDirection::Next),
            ShellRequest::ShowDesktop,
        ]
    );
    assert_eq!(
        completed_profiles(&pipeline),
        vec![
            Some(LAPTOP.to_string()),
            Some(TRACKPAD.to_string()),
            Some(LAPTOP.to_string()),
        ]
    );
}

#[test]
fn an_unknown_or_unprofiled_device_falls_back_to_the_global_profile() {
    let machine = Machine::new();
    let (mut pipeline, shell) = pipeline(&machine, profiles(), Box::new(machine.identities()));

    // The shell named no device; a pad the configuration has never seen; and a
    // node the kernel has no attributes for.
    let mut events = whole_swipe(None, 0);
    events.extend(whole_swipe(Some(UNPROFILED_NODE), 2_000));
    events.extend(whole_swipe(Some(InputDeviceNode::new(42)), 4_000));
    run(&mut pipeline, events);

    assert_eq!(
        shell.calls(),
        vec![
            ShellRequest::ShowOverview,
            ShellRequest::ShowOverview,
            ShellRequest::ShowOverview,
        ]
    );
    assert_eq!(completed_profiles(&pipeline), vec![None, None, None]);
}

#[test]
fn a_pad_whose_own_profile_is_off_does_not_borrow_the_global_one() {
    let machine = Machine::new();
    let mut profiles = profiles();
    profiles
        .devices
        .get_mut(LAPTOP)
        .expect("the laptop profile")
        .enabled = false;
    let (mut pipeline, shell) = pipeline(&machine, profiles, Box::new(machine.identities()));

    let mut events = whole_swipe(Some(LAPTOP_NODE), 0);
    events.extend(whole_swipe(Some(TRACKPAD_NODE), 2_000));
    run(&mut pipeline, events);

    assert_eq!(
        shell.calls(),
        vec![ShellRequest::SwitchWorkspace(WorkspaceDirection::Next)]
    );
}

#[test]
fn another_pad_starting_a_gesture_cancels_the_one_under_way() {
    let machine = Machine::new();
    let (mut pipeline, shell) = pipeline(&machine, profiles(), Box::new(machine.identities()));

    // The laptop pad gets most of the way, then the trackpad begins and
    // completes its own gesture, then the laptop pad's fingers lift.
    let mut events = vec![event(0, 0.0, 0, Some(LAPTOP_NODE))];
    for step in 1..=7 {
        events.push(event(1, -WHOLE_SWIPE / 8.0, step * 16, Some(LAPTOP_NODE)));
    }
    events.extend(whole_swipe(Some(TRACKPAD_NODE), 200));
    events.push(event(2, -WHOLE_SWIPE / 8.0, 400, Some(LAPTOP_NODE)));
    run(&mut pipeline, events);

    // The trackpad's gesture happened. The laptop pad's never did: its frames
    // were not finished by the trackpad's, and its lift found no gesture.
    assert_eq!(
        shell.calls(),
        vec![ShellRequest::SwitchWorkspace(WorkspaceDirection::Next)]
    );
    assert_eq!(
        completed_profiles(&pipeline),
        vec![Some(TRACKPAD.to_string())]
    );
    // And the laptop pad's gesture was ended, on its own profile, when the
    // trackpad began — not left armed.
    let cancelled: Vec<Option<String>> = pipeline
        .performed()
        .iter()
        .filter(|one| one.kind == GestureEventKind::Cancel)
        .map(|one| one.profile.clone())
        .collect();
    assert_eq!(cancelled, vec![Some(LAPTOP.to_string())]);
}

#[test]
fn one_stray_event_from_another_pad_ends_the_gesture_under_way() {
    let machine = Machine::new();
    let (mut pipeline, shell) = pipeline(&machine, profiles(), Box::new(machine.identities()));

    // The laptop pad swipes the whole way, but one update in the middle came
    // from the trackpad. The laptop's gesture ends at that event, so the rest
    // of its frames belong to nothing.
    let mut events = vec![event(0, 0.0, 0, Some(LAPTOP_NODE))];
    for step in 1..=8 {
        let device = if step == 4 {
            Some(TRACKPAD_NODE)
        } else {
            Some(LAPTOP_NODE)
        };
        let phase = if step == 8 { 2 } else { 1 };
        events.push(event(phase, -WHOLE_SWIPE / 8.0, step * 16, device));
    }
    run(&mut pipeline, events);

    assert!(shell.calls().is_empty(), "{:?}", shell.calls());
}

#[test]
fn frames_from_two_pads_following_one_profile_are_never_one_gesture() {
    let machine = Machine::new();
    // Every pad follows the global profile, so the only thing keeping the two
    // streams apart is which device made each event.
    let profiles = GestureProfiles::global_only(swipe_up(DesktopAction::ShowOverview));
    let (mut pipeline, shell) = pipeline(&machine, profiles, Box::new(machine.identities()));

    // Half a swipe on one pad and the other half on another. Measured as one
    // gesture it would be a whole swipe.
    let mut events = vec![event(0, 0.0, 0, Some(LAPTOP_NODE))];
    for step in 1..=4 {
        events.push(event(1, -WHOLE_SWIPE / 8.0, step * 16, Some(LAPTOP_NODE)));
    }
    for step in 5..=7 {
        events.push(event(
            1,
            -WHOLE_SWIPE / 8.0,
            step * 16,
            Some(UNPROFILED_NODE),
        ));
    }
    events.push(event(2, -WHOLE_SWIPE / 8.0, 8 * 16, Some(UNPROFILED_NODE)));
    run(&mut pipeline, events);

    assert!(shell.calls().is_empty(), "{:?}", shell.calls());
    assert!(
        pipeline
            .performed()
            .iter()
            .all(|one| one.outcome != InvocationOutcome::Invoked),
        "{:?}",
        pipeline.performed()
    );
}

#[test]
fn a_pipeline_that_cannot_resolve_devices_performs_the_global_profile_everywhere() {
    let machine = Machine::new();
    let (mut pipeline, shell) = pipeline(&machine, profiles(), Box::new(NoDeviceIdentities));

    let mut events = whole_swipe(Some(LAPTOP_NODE), 0);
    events.extend(whole_swipe(Some(TRACKPAD_NODE), 2_000));
    run(&mut pipeline, events);

    assert_eq!(
        shell.calls(),
        vec![ShellRequest::ShowOverview, ShellRequest::ShowOverview]
    );
}

/// The identity the service reads from `/sys` is the identity
/// `touchpad-platform` files a profile under, built by the same function.
#[test]
fn a_node_resolves_to_the_stable_identity_and_nothing_else_does() {
    let machine = Machine::new();
    let identities = machine.identities();
    assert_eq!(identities.identity(LAPTOP_NODE).as_deref(), Some(LAPTOP));
    assert_eq!(
        identities.identity(TRACKPAD_NODE).as_deref(),
        Some(TRACKPAD)
    );
    assert_eq!(
        identities.identity(UNPROFILED_NODE).as_deref(),
        Some(UNPROFILED)
    );
    // A node with no device behind it.
    assert_eq!(identities.identity(InputDeviceNode::new(42)), None);
    assert_eq!(NoDeviceIdentities.identity(LAPTOP_NODE), None);
}

/// Auto-disable is about the adapter, which every pad shares, so it turns
/// every profile off and writes that down.
#[test]
fn a_failing_adapter_turns_every_profile_off() {
    let machine = Machine::new();
    let adapter = GnomeShellAdapter::with_reported(
        Box::new(FakeShellBridge::failing(
            touchpad_session::ShellError::CallFailed("the shell did not answer".to_string()),
        )),
        FakeShellBridge::gnome_46_capabilities(),
    );
    let store = machine.store();
    store.save_profiles(&profiles()).expect("a saved document");
    let mut pipeline = GesturePipeline::with_profiles(
        profiles(),
        machine.store(),
        Box::new(adapter),
        Box::new(machine.identities()),
    );

    let mut events = whole_swipe(Some(LAPTOP_NODE), 0);
    events.extend(whole_swipe(Some(TRACKPAD_NODE), 2_000));
    events.extend(whole_swipe(None, 4_000));
    run(&mut pipeline, events);

    assert!(!pipeline.is_enabled());
    let stored = store.load_profiles().expect("a saved document");
    assert!(!stored.global.enabled);
    assert!(stored.devices.values().all(|profile| !profile.enabled));
}
