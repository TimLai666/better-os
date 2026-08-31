//! The GNOME Shell route: `org.betteros.TouchpadAdapter1`.
//!
//! [ADR 0012](../../../docs/decisions/0012-touchpad-gesture-backend.md) chose
//! the minimal GNOME Shell adapter as the production gesture backend, and the
//! project owner granted the bounded GJS exception it depended on. The
//! extension lives in `adapters/gnome-shell-touchpad/`; this module is the Rust
//! half of the same contract.
//!
//! The split is the point. The extension reports what the compositor saw and
//! performs what it is told; every decision — which gesture, which threshold,
//! whether to commit, whether to suppress GNOME's own gestures — is made here
//! and above here. [`ShellBridge`] is the seam between the two, so this
//! adapter's whole behaviour is testable with no bus, no shell, and no
//! extension, and the session-bus implementation underneath it carries no
//! policy at all.
//!
//! Two limits are declared rather than discovered:
//!
//! - **No continuous progress reaches an action.** The event stream is
//!   continuous and the recognizer uses every frame of it, but GNOME 46 exposes
//!   no way to drive the overview's own transition from outside the shell. So
//!   the actions here are discrete: they happen at the end of a gesture that
//!   committed, and a reversal before the threshold never reaches the desktop
//!   at all. Issue #3 allows discrete activation as the first fallback.
//! - **`current-application windows` has nothing to map to.** GNOME 46's
//!   window picker *is* the overview and cannot be filtered to the focused
//!   application, so it is reported unsupported with that reason rather than
//!   quietly opening the overview twice.

use better_actions::{ActionCapabilities, ActionSupport, DesktopAction};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::adapter::{
    AdapterDescription, GesturePhase, GestureProgress, InvocationOutcome, SessionAdapter,
    SuppressionOutcome,
};

/// The name the extension owns on the session bus.
pub const BUS_NAME: &str = "org.betteros.TouchpadAdapter1";
pub const OBJECT_PATH: &str = "/org/betteros/TouchpadAdapter1";
pub const INTERFACE_NAME: &str = "org.betteros.TouchpadAdapter1";

/// Every method the extension serves, and every signal it emits. The names are
/// here once so that the client, the tests, and the interface XML can be
/// checked against each other rather than against memory.
pub const METHODS: &[&str] = &[
    "ShowOverview",
    "ShowDesktop",
    "SwitchWorkspace",
    "SuppressBuiltInGestures",
    "Capabilities",
];
pub const SIGNALS: &[&str] = &["SwipeGesture", "PinchGesture"];

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ShellError {
    #[error("session.shell_adapter_unreachable:{0}")]
    Unreachable(String),
    #[error("session.shell_call_failed:{0}")]
    CallFailed(String),
    #[error("session.shell_capabilities_unreadable:{0}")]
    Capabilities(String),
}

/// Which way a workspace switch goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceDirection {
    Previous,
    Next,
}

impl WorkspaceDirection {
    /// The wire value: -1 for the workspace on the left, 1 for the one on the
    /// right.
    pub fn wire(self) -> i32 {
        match self {
            Self::Previous => -1,
            Self::Next => 1,
        }
    }
}

/// Everything this adapter can ask the shell to do.
///
/// A closed enum with no string in it, for the same reason
/// [`better_actions::DesktopAction`] is one: there is no shape here that a
/// configuration file could fill with a command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellRequest {
    ShowOverview,
    ShowDesktop,
    SwitchWorkspace(WorkspaceDirection),
    SuppressBuiltInGestures(bool),
}

impl ShellRequest {
    /// The D-Bus method this request is made with.
    pub fn method(self) -> &'static str {
        match self {
            Self::ShowOverview => "ShowOverview",
            Self::ShowDesktop => "ShowDesktop",
            Self::SwitchWorkspace(_) => "SwitchWorkspace",
            Self::SuppressBuiltInGestures(_) => "SuppressBuiltInGestures",
        }
    }
}

/// What the extension says its event stream can distinguish.
///
/// Reported by the extension and parsed here rather than assumed, because the
/// honest answer changes with the shell: a newer GNOME could tell a thumb from
/// a finger, and nothing in this crate should have to be edited for that to
/// become visible.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShellCapabilities {
    pub protocol_version: u32,
    pub shell_version: String,
    /// Whether the stream carries how many contacts a gesture has.
    pub finger_count: bool,
    /// Whether it can tell which contact is a thumb. Clutter cannot, so this is
    /// `false` on GNOME 46, and the preset's thumb-and-three gestures are
    /// matched as four contacts with the thumb assumed.
    pub thumb_detection: bool,
    /// Whether gesture progress arrives continuously.
    pub continuous_progress: bool,
    pub gesture_kinds: Vec<String>,
    pub actions: Vec<String>,
    pub unsupported_actions: Vec<String>,
    /// How many of GNOME's own swipe trackers the extension can reach. Zero
    /// means suppression cannot work on this shell whatever it reports, which
    /// is a different thing from suppression having been asked for and refused.
    pub built_in_trackers: u32,
    pub built_in_gestures_suppressed: bool,
}

impl ShellCapabilities {
    pub fn from_json(text: &str) -> Result<Self, ShellError> {
        serde_json::from_str(text).map_err(|error| ShellError::Capabilities(error.to_string()))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("shell capabilities always serialize")
    }
}

/// One gesture signal, exactly as it arrived on the wire.
///
/// Untranslated on purpose. Turning these numbers into something a recognizer
/// understands is `touchpad_gestures::ingest`'s job, and it belongs there
/// because that is where the scales and the thresholds live. This type is the
/// transport's whole vocabulary, which is why an unknown phase number survives
/// as far as the crate that knows what to do about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShellGestureEvent {
    Swipe {
        phase: u32,
        fingers: u32,
        /// Motion since the previous event, in pixels.
        dx: f64,
        dy: f64,
        at_ms: u64,
    },
    Pinch {
        phase: u32,
        fingers: u32,
        /// The scale measured from the start of the gesture.
        scale: f64,
        /// The turn since the previous event, in radians.
        angle_delta: f64,
        at_ms: u64,
    },
}

impl ShellGestureEvent {
    pub fn phase(&self) -> u32 {
        match self {
            Self::Swipe { phase, .. } | Self::Pinch { phase, .. } => *phase,
        }
    }

    pub fn fingers(&self) -> u32 {
        match self {
            Self::Swipe { fingers, .. } | Self::Pinch { fingers, .. } => *fingers,
        }
    }

    pub fn at_ms(&self) -> u64 {
        match self {
            Self::Swipe { at_ms, .. } | Self::Pinch { at_ms, .. } => *at_ms,
        }
    }
}

/// A source of gesture signals.
///
/// Blocking, and with no runtime in the signature, so the pipeline that reads
/// it is ordinary synchronous code. The session-bus implementation is
/// [`crate::bus::SessionBusShellEvents`]; a recorded stream implements the same
/// trait, which is how the whole pipeline is driven end to end in a test.
pub trait ShellEvents: Send {
    /// The next event, or `None` when the stream has ended.
    fn recv(&mut self) -> Option<ShellGestureEvent>;
}

/// A recorded stream, replayed in order and then ended.
pub struct RecordedShellEvents {
    events: std::collections::VecDeque<ShellGestureEvent>,
}

impl RecordedShellEvents {
    pub fn new(events: impl IntoIterator<Item = ShellGestureEvent>) -> Self {
        Self {
            events: events.into_iter().collect(),
        }
    }
}

impl ShellEvents for RecordedShellEvents {
    fn recv(&mut self) -> Option<ShellGestureEvent> {
        self.events.pop_front()
    }
}

/// The transport under [`GnomeShellAdapter`].
///
/// It carries no policy: it makes a call and reports whether it worked. The
/// session-bus implementation is [`ZbusShellBridge`]; [`FakeShellBridge`]
/// records instead, and is what the adapter's own tests and the pipeline's
/// tests run against.
pub trait ShellBridge: Send {
    fn call(&self, request: ShellRequest) -> Result<(), ShellError>;
    fn capabilities(&self) -> Result<ShellCapabilities, ShellError>;
}

/// Invokes the shell-owned actions through the GNOME Shell adapter extension.
pub struct GnomeShellAdapter {
    bridge: Box<dyn ShellBridge>,
    capabilities: ActionCapabilities,
    reported: ShellCapabilities,
}

impl GnomeShellAdapter {
    /// Asks the extension what it can do and builds the adapter around the
    /// answer. A shell with no extension fails here rather than producing an
    /// adapter that claims actions nothing will perform.
    pub fn connect(bridge: Box<dyn ShellBridge>) -> Result<Self, ShellError> {
        let reported = bridge.capabilities()?;
        Ok(Self::with_reported(bridge, reported))
    }

    pub fn with_reported(bridge: Box<dyn ShellBridge>, reported: ShellCapabilities) -> Self {
        Self {
            bridge,
            capabilities: Self::declare(),
            reported,
        }
    }

    /// What the extension said about itself.
    pub fn reported(&self) -> &ShellCapabilities {
        &self.reported
    }

    /// The four actions GNOME Shell has a facility for, and the one it does
    /// not.
    fn declare() -> ActionCapabilities {
        ActionCapabilities::new()
            .with(&DesktopAction::ShowOverview, ActionSupport::discrete())
            .with(&DesktopAction::ShowDesktop, ActionSupport::discrete())
            .with(&DesktopAction::NextWorkspace, ActionSupport::discrete())
            .with(&DesktopAction::PreviousWorkspace, ActionSupport::discrete())
            .with(
                &DesktopAction::CurrentApplicationWindows,
                ActionSupport::unsupported(
                    "gnome.no_per_application_window_picker",
                    "GNOME 46's window picker is the overview itself and cannot be \
                     filtered to the focused application",
                ),
            )
    }

    fn request_for(action: &DesktopAction) -> Option<ShellRequest> {
        match action {
            DesktopAction::ShowOverview => Some(ShellRequest::ShowOverview),
            DesktopAction::ShowDesktop => Some(ShellRequest::ShowDesktop),
            DesktopAction::NextWorkspace => {
                Some(ShellRequest::SwitchWorkspace(WorkspaceDirection::Next))
            }
            DesktopAction::PreviousWorkspace => {
                Some(ShellRequest::SwitchWorkspace(WorkspaceDirection::Previous))
            }
            _ => None,
        }
    }
}

impl SessionAdapter for GnomeShellAdapter {
    fn describe(&self) -> AdapterDescription {
        AdapterDescription {
            name: format!("gnome-shell ({})", self.reported.shell_version),
            // The event stream is continuous; the actions are not. This field
            // is about what reaches the desktop, so it is false.
            continuous_progress: false,
            performs_system_actions: true,
        }
    }

    fn capabilities(&self) -> &ActionCapabilities {
        &self.capabilities
    }

    fn invoke(&mut self, action: &DesktopAction, progress: GestureProgress) -> InvocationOutcome {
        let Some(request) = Self::request_for(action) else {
            return match self.capabilities.support(action) {
                ActionSupport::Unsupported { reason, detail } => {
                    InvocationOutcome::unsupported(reason, detail)
                }
                // Declared and unroutable would be a bug rather than a
                // capability, and saying so beats doing nothing quietly.
                ActionSupport::Supported { .. } => InvocationOutcome::failed(
                    "session.declared_without_a_route",
                    format!(
                        "{} is declared but this adapter has no call for it",
                        action.key()
                    ),
                ),
            };
        };
        if progress.phase != GesturePhase::End {
            return InvocationOutcome::ignored("session.adapter_has_no_continuous_progress");
        }
        match self.bridge.call(request) {
            Ok(()) => InvocationOutcome::Invoked,
            Err(error) => InvocationOutcome::failed("session.shell_call_failed", error.to_string()),
        }
    }

    fn suppress_built_in_gestures(&mut self, suppress: bool) -> SuppressionOutcome {
        match self
            .bridge
            .call(ShellRequest::SuppressBuiltInGestures(suppress))
        {
            Ok(()) if suppress => SuppressionOutcome::Suppressed,
            Ok(()) => SuppressionOutcome::Restored,
            Err(error) => {
                SuppressionOutcome::failed("session.shell_call_failed", error.to_string())
            }
        }
    }
}

/// A bridge that records what it was asked and changes nothing.
///
/// Shipped rather than test-only for the same reason [`crate::MockSessionAdapter`]
/// is: the tests and the pipeline's own integration suite both drive it, so
/// there is one fake rather than two that can drift apart.
pub struct FakeShellBridge {
    state: std::sync::Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    calls: Vec<ShellRequest>,
    fail: Option<ShellError>,
    capabilities: Option<ShellCapabilities>,
}

impl Default for FakeShellBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeShellBridge {
    pub fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(FakeState::default()),
        }
    }

    /// A bridge whose every call fails, for the auto-disable path.
    pub fn failing(error: ShellError) -> Self {
        let bridge = Self::new();
        bridge.state.lock().expect("fake bridge").fail = Some(error);
        bridge
    }

    pub fn with_capabilities(capabilities: ShellCapabilities) -> Self {
        let bridge = Self::new();
        bridge.state.lock().expect("fake bridge").capabilities = Some(capabilities);
        bridge
    }

    pub fn calls(&self) -> Vec<ShellRequest> {
        self.state.lock().expect("fake bridge").calls.clone()
    }

    /// What a GNOME 46 extension reports: contact counts yes, thumbs no,
    /// continuous progress yes.
    pub fn gnome_46_capabilities() -> ShellCapabilities {
        ShellCapabilities {
            protocol_version: 1,
            shell_version: "46.0".to_string(),
            finger_count: true,
            thumb_detection: false,
            continuous_progress: true,
            gesture_kinds: vec!["swipe".to_string(), "pinch".to_string()],
            actions: vec![
                "overview".to_string(),
                "show-desktop".to_string(),
                "switch-workspace".to_string(),
            ],
            unsupported_actions: vec!["current-application-windows".to_string()],
            // GNOME 46 has two: the overview and application grid tracker, and
            // the workspace-switching one. Measured on a nested GNOME Shell
            // 46.0, not assumed.
            built_in_trackers: 2,
            built_in_gestures_suppressed: false,
        }
    }
}

impl ShellBridge for FakeShellBridge {
    fn call(&self, request: ShellRequest) -> Result<(), ShellError> {
        let mut state = self.state.lock().expect("fake bridge");
        if let Some(error) = state.fail.clone() {
            return Err(error);
        }
        state.calls.push(request);
        if let ShellRequest::SuppressBuiltInGestures(suppress) = request {
            if let Some(capabilities) = state.capabilities.as_mut() {
                capabilities.built_in_gestures_suppressed = suppress;
            }
        }
        Ok(())
    }

    fn capabilities(&self) -> Result<ShellCapabilities, ShellError> {
        let state = self.state.lock().expect("fake bridge");
        if let Some(error) = state.fail.clone() {
            return Err(error);
        }
        Ok(state
            .capabilities
            .clone()
            .unwrap_or_else(Self::gnome_46_capabilities))
    }
}

/// A handle a test or a caller can keep on a bridge the adapter owns.
pub struct SharedShellBridge(pub std::sync::Arc<FakeShellBridge>);

impl ShellBridge for SharedShellBridge {
    fn call(&self, request: ShellRequest) -> Result<(), ShellError> {
        self.0.call(request)
    }

    fn capabilities(&self) -> Result<ShellCapabilities, ShellError> {
        self.0.capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn adapter(bridge: Arc<FakeShellBridge>) -> GnomeShellAdapter {
        GnomeShellAdapter::connect(Box::new(SharedShellBridge(bridge))).expect("a shell adapter")
    }

    #[test]
    fn the_four_shell_actions_reach_the_extension_and_nothing_else_does() {
        let recorded = Arc::new(FakeShellBridge::new());
        let mut adapter = adapter(recorded.clone());
        for action in [
            DesktopAction::ShowOverview,
            DesktopAction::ShowDesktop,
            DesktopAction::NextWorkspace,
            DesktopAction::PreviousWorkspace,
        ] {
            assert_eq!(
                adapter.invoke(&action, GestureProgress::completed()),
                InvocationOutcome::Invoked,
                "{}",
                action.key()
            );
        }
        assert_eq!(
            recorded.calls(),
            vec![
                ShellRequest::ShowOverview,
                ShellRequest::ShowDesktop,
                ShellRequest::SwitchWorkspace(WorkspaceDirection::Next),
                ShellRequest::SwitchWorkspace(WorkspaceDirection::Previous),
            ]
        );
    }

    #[test]
    fn the_launcher_is_not_this_adapters_job_and_it_says_so() {
        let mut adapter = adapter(Arc::new(FakeShellBridge::new()));
        for action in [DesktopAction::LauncherOpen, DesktopAction::VolumeUp] {
            assert!(
                matches!(
                    adapter.invoke(&action, GestureProgress::completed()),
                    InvocationOutcome::Unsupported { .. }
                ),
                "{} was claimed",
                action.key()
            );
        }
    }

    /// The honest answer for the four-fingers-down row of the preset.
    #[test]
    fn the_current_applications_windows_have_no_gnome_46_facility_and_the_reason_is_named() {
        let mut adapter = adapter(Arc::new(FakeShellBridge::new()));
        let outcome = adapter.invoke(
            &DesktopAction::CurrentApplicationWindows,
            GestureProgress::completed(),
        );
        assert_eq!(
            outcome,
            InvocationOutcome::unsupported(
                "gnome.no_per_application_window_picker",
                "GNOME 46's window picker is the overview itself and cannot be \
                 filtered to the focused application",
            )
        );
    }

    #[test]
    fn progress_before_the_end_of_the_gesture_reaches_no_shell_call() {
        let recorded = Arc::new(FakeShellBridge::new());
        let mut adapter = adapter(recorded.clone());
        for phase in [
            GesturePhase::Begin,
            GesturePhase::Update,
            GesturePhase::Cancel,
        ] {
            assert!(matches!(
                adapter.invoke(
                    &DesktopAction::ShowOverview,
                    GestureProgress::new(phase, 0.8)
                ),
                InvocationOutcome::Ignored { .. }
            ));
        }
        assert!(recorded.calls().is_empty());
        assert!(!adapter.describe().continuous_progress);
        assert!(adapter.describe().performs_system_actions);
    }

    #[test]
    fn suppressing_and_restoring_the_desktops_own_gestures_are_both_reported() {
        let recorded = Arc::new(FakeShellBridge::new());
        let mut adapter = adapter(recorded.clone());
        assert_eq!(
            adapter.suppress_built_in_gestures(true),
            SuppressionOutcome::Suppressed
        );
        assert_eq!(
            adapter.suppress_built_in_gestures(false),
            SuppressionOutcome::Restored
        );
        assert_eq!(
            recorded.calls(),
            vec![
                ShellRequest::SuppressBuiltInGestures(true),
                ShellRequest::SuppressBuiltInGestures(false),
            ]
        );
    }

    #[test]
    fn an_unreachable_extension_is_a_reported_failure_rather_than_a_silent_one() {
        let bridge = FakeShellBridge::failing(ShellError::Unreachable("no such name".to_string()));
        assert!(GnomeShellAdapter::connect(Box::new(bridge)).is_err());

        let mut adapter = GnomeShellAdapter::with_reported(
            Box::new(FakeShellBridge::failing(ShellError::CallFailed(
                "the shell did not answer".to_string(),
            ))),
            FakeShellBridge::gnome_46_capabilities(),
        );
        assert!(matches!(
            adapter.invoke(&DesktopAction::ShowOverview, GestureProgress::completed()),
            InvocationOutcome::Failed { .. }
        ));
        assert!(matches!(
            adapter.suppress_built_in_gestures(true),
            SuppressionOutcome::Failed { .. }
        ));
    }

    #[test]
    fn the_reported_capabilities_say_the_shell_cannot_see_a_thumb() {
        let adapter = adapter(Arc::new(FakeShellBridge::new()));
        assert!(adapter.reported().finger_count);
        assert!(!adapter.reported().thumb_detection);
        assert!(adapter.reported().continuous_progress);
    }

    #[test]
    fn the_capability_document_round_trips_the_way_the_extension_writes_it() {
        let capabilities = FakeShellBridge::gnome_46_capabilities();
        assert_eq!(
            ShellCapabilities::from_json(&capabilities.to_json()).unwrap(),
            capabilities
        );
        assert!(matches!(
            ShellCapabilities::from_json("not a document"),
            Err(ShellError::Capabilities(_))
        ));
    }

    #[test]
    fn every_request_names_a_method_the_interface_declares() {
        for request in [
            ShellRequest::ShowOverview,
            ShellRequest::ShowDesktop,
            ShellRequest::SwitchWorkspace(WorkspaceDirection::Next),
            ShellRequest::SuppressBuiltInGestures(true),
        ] {
            assert!(
                METHODS.contains(&request.method()),
                "{} is not declared",
                request.method()
            );
        }
        assert_eq!(WorkspaceDirection::Next.wire(), 1);
        assert_eq!(WorkspaceDirection::Previous.wire(), -1);
    }
}
