//! The boundary a recognized gesture crosses to reach the desktop.
//!
//! Everything above this trait deals in [`DesktopAction`] values and a progress
//! fraction. Everything below it is whatever the session actually offers. The
//! trait is narrow on purpose — describe yourself, say what you can do, bind an
//! action, invoke an action, verify a binding — and it takes no string from
//! configuration at any point. There is no method here through which a
//! configuration file could ask for anything other than a catalog action.
//!
//! The vocabulary is deliberately the launcher's. [`GesturePhase`] has the same
//! four variants as `launcher_platform::gesture::GesturePhase` and means the
//! same thing by each of them, so a gesture recognized on the touchpad side can
//! be handed to the launcher's own recognizer without translation beyond the
//! shape of the struct. `touchpad-gestures` emits a superset — it distinguishes
//! the frame that crosses the activation threshold — and that superset maps
//! down onto these four.

use better_actions::{ActionCapabilities, ActionSupport, DesktopAction};

/// Where a gesture is in its life.
///
/// The same four phases `launcher-platform` defines. `End` means the fingers
/// left the pad; whether that commits is a threshold decision made before the
/// action ever reaches an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GesturePhase {
    Begin,
    Update,
    End,
    Cancel,
}

/// How far through a gesture is, and which phase reported it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureProgress {
    pub phase: GesturePhase,
    /// Clamped to `0.0..=1.0`. An adapter with no continuous progress sees
    /// `0.0` until it sees `1.0`.
    pub fraction: f32,
}

impl GestureProgress {
    pub fn new(phase: GesturePhase, fraction: f32) -> Self {
        Self {
            phase,
            fraction: fraction.clamp(0.0, 1.0),
        }
    }

    /// A completed gesture with no intermediate progress behind it. This is
    /// what a discrete backend produces, and what test mode replays.
    pub fn completed() -> Self {
        Self::new(GesturePhase::End, 1.0)
    }

    pub fn is_final(&self) -> bool {
        matches!(self.phase, GesturePhase::End)
    }
}

/// What an adapter is, for diagnostics and for the capability report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterDescription {
    pub name: String,
    /// Whether this adapter can deliver intermediate progress at all. An action
    /// animates only when this and [`DesktopAction::follows_progress`] are both
    /// true.
    pub continuous_progress: bool,
    /// Whether this adapter reaches a real desktop. A test-mode adapter says
    /// `false`, and the screen says so rather than implying a system changed.
    pub performs_system_actions: bool,
}

/// What happened when an action was invoked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvocationOutcome {
    Invoked,
    /// Nothing was done and nothing was wrong: a progress update reached an
    /// adapter that only acts on completion, or the action was `Disabled`.
    Ignored {
        reason: String,
    },
    Unsupported {
        reason: String,
        detail: String,
    },
    Failed {
        reason: String,
        detail: String,
    },
}

impl InvocationOutcome {
    pub fn ignored(reason: impl Into<String>) -> Self {
        Self::Ignored {
            reason: reason.into(),
        }
    }

    pub fn unsupported(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
            detail: detail.into(),
        }
    }

    pub fn failed(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
            detail: detail.into(),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Invoked)
    }
}

/// What happened when a gesture's action was bound.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindOutcome {
    Bound,
    Unsupported { reason: String, detail: String },
    Failed { reason: String, detail: String },
}

/// What a second look said about a binding.
///
/// Verification is a separate question from binding, for the same reason
/// `touchpad-platform` reads a setting back rather than trusting a successful
/// write: an adapter that accepted a binding has not thereby proved one exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationResult {
    Verified { continuous_progress: bool },
    Unverified { reason: String, detail: String },
    Unsupported { reason: String, detail: String },
}

impl VerificationResult {
    pub fn is_verified(&self) -> bool {
        matches!(self, Self::Verified { .. })
    }
}

/// What happened when the desktop's own gestures were asked to step aside.
///
/// Separate from [`InvocationOutcome`] because it is a separate promise. An
/// adapter that performs every Better OS action may still be unable to touch
/// what GNOME does with the same fingers, and a preview that said "the desktop
/// gesture will be turned off" has to be able to report that it was not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SuppressionOutcome {
    /// The desktop's own gestures are off.
    Suppressed,
    /// They are back exactly as they were.
    Restored,
    Unsupported {
        reason: String,
        detail: String,
    },
    Failed {
        reason: String,
        detail: String,
    },
}

impl SuppressionOutcome {
    /// The answer for an adapter that cannot reach the desktop's own gestures
    /// at all, which is every adapter that is not the GNOME Shell one.
    pub fn unsupported() -> Self {
        Self::Unsupported {
            reason: "session.built_in_gestures_not_changeable".to_string(),
            detail: "this adapter cannot change what the desktop does with the same fingers"
                .to_string(),
        }
    }

    pub fn failed(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
            detail: detail.into(),
        }
    }
}

/// The session boundary.
///
/// Only three methods have to be written by an implementation. Binding,
/// verifying, and suppressing have defaults, because an adapter that cannot do
/// an action cannot have bound it either, and an adapter that says nothing
/// about the desktop's own gestures has not changed them. Writing those rules
/// once means no adapter can disagree with them.
pub trait SessionAdapter {
    fn describe(&self) -> AdapterDescription;

    /// Everything this adapter can do. A missing action is unsupported.
    fn capabilities(&self) -> &ActionCapabilities;

    /// Performs the action. `progress` says which phase asked for it, so an
    /// adapter that animates can follow it and one that does not can wait for
    /// [`GesturePhase::End`].
    fn invoke(&mut self, action: &DesktopAction, progress: GestureProgress) -> InvocationOutcome;

    fn support(&self, action: &DesktopAction) -> ActionSupport {
        self.capabilities().support(action)
    }

    /// Attaches an action to a gesture. The default binds whatever the adapter
    /// says it supports and refuses the rest.
    fn bind(&mut self, action: &DesktopAction) -> BindOutcome {
        match self.support(action) {
            ActionSupport::Supported { .. } => BindOutcome::Bound,
            ActionSupport::Unsupported { reason, detail } => {
                BindOutcome::Unsupported { reason, detail }
            }
        }
    }

    /// Turns the desktop's own gestures off, or puts them back.
    ///
    /// The default refuses. That is deliberate: suppressing a built-in gesture
    /// is a real change to somebody else's software, and an adapter that has
    /// not implemented it must not appear to have done it. A confirmed plan
    /// that asked for it then reports what actually happened.
    fn suppress_built_in_gestures(&mut self, suppress: bool) -> SuppressionOutcome {
        let _ = suppress;
        SuppressionOutcome::unsupported()
    }

    /// Looks again. The default answers from the capability report, which is
    /// all a capability-only adapter can honestly say.
    fn verify(&self, action: &DesktopAction) -> VerificationResult {
        match self.support(action) {
            ActionSupport::Supported {
                continuous_progress,
            } => VerificationResult::Verified {
                continuous_progress,
            },
            ActionSupport::Unsupported { reason, detail } => {
                VerificationResult::Unsupported { reason, detail }
            }
        }
    }
}
