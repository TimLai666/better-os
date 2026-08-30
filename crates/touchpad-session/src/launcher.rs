//! The one production route in this build: opening and closing Better
//! Launcher.
//!
//! It reinvents nothing. Better Launcher already owns `org.betteros.Launcher1`
//! and already serves `org.freedesktop.Application`, which is how a second
//! launch, a dock, and `gio launch` reach the running overlay. This adapter
//! sends the same calls through `launcher_platform`'s own
//! [`NameRegistry::forward`], so the gesture is one more caller of a path that
//! is already tested there rather than a second implementation of it.
//!
//! It is honest about its narrowness. Every action other than the two launcher
//! ones is reported unsupported, because this route reaches the launcher and
//! nothing else, and there is no adapter in this build that reaches the shell.
//! Which adapter eventually does is
//! [ADR 0012](../../../docs/decisions/0012-touchpad-gesture-backend.md).
//!
//! There is no continuous progress here. `org.freedesktop.Application` carries
//! activation, not a progress stream, so the adapter acts on the end of a
//! gesture and ignores what comes before it.

use better_actions::{ActionCapabilities, ActionSupport, DesktopAction};
use launcher_platform::activation::{ActivationRequest, NameRegistry, SingleInstance};

use crate::adapter::{
    AdapterDescription, GesturePhase, GestureProgress, InvocationOutcome, SessionAdapter,
};

/// Translates one Better Touchpad progress report into the sample shape
/// `launcher-platform` already defines.
///
/// This function is the whole cost of the two crates agreeing. Phase for phase
/// the vocabularies are identical, so the translation carries no policy: it
/// renames nothing, decides nothing, and drops nothing. What Better Touchpad
/// adds on top — that it distinguishes the frame which crossed the activation
/// threshold, and that a gesture can require a thumb — is a superset that maps
/// down onto these four phases without loss.
pub fn launcher_sample(
    progress: GestureProgress,
    inward: bool,
    contacts: u8,
) -> launcher_platform::gesture::GestureSample {
    use launcher_platform::gesture::{GestureDirection, GesturePhase as LauncherPhase};

    let phase = match progress.phase {
        GesturePhase::Begin => LauncherPhase::Begin,
        GesturePhase::Update => LauncherPhase::Update,
        GesturePhase::End => LauncherPhase::End,
        GesturePhase::Cancel => LauncherPhase::Cancel,
    };
    let direction = if inward {
        GestureDirection::Inward
    } else {
        GestureDirection::Outward
    };
    launcher_platform::gesture::GestureSample::new(phase, direction, progress.fraction, contacts)
}

/// Invokes the launcher actions over Better Launcher's activation interface.
pub struct LauncherActivationAdapter {
    registry: Box<dyn NameRegistry>,
    instance: SingleInstance,
    capabilities: ActionCapabilities,
}

impl LauncherActivationAdapter {
    /// Wraps any registry. The session-bus one lives in `launcher-platform`;
    /// a test passes a fake and never touches a bus.
    pub fn new(registry: Box<dyn NameRegistry>) -> Self {
        Self::named(registry, SingleInstance::default())
    }

    pub fn named(registry: Box<dyn NameRegistry>, instance: SingleInstance) -> Self {
        let capabilities = ActionCapabilities::new()
            .with(&DesktopAction::LauncherOpen, ActionSupport::discrete())
            .with(&DesktopAction::LauncherClose, ActionSupport::discrete());
        Self {
            registry,
            instance,
            capabilities,
        }
    }
}

impl SessionAdapter for LauncherActivationAdapter {
    fn describe(&self) -> AdapterDescription {
        AdapterDescription {
            name: format!("launcher-activation ({})", self.instance.name()),
            continuous_progress: false,
            performs_system_actions: true,
        }
    }

    fn capabilities(&self) -> &ActionCapabilities {
        &self.capabilities
    }

    fn invoke(&mut self, action: &DesktopAction, progress: GestureProgress) -> InvocationOutcome {
        let request = match action {
            DesktopAction::LauncherOpen => ActivationRequest::Open,
            DesktopAction::LauncherClose => ActivationRequest::Close,
            other => {
                let support = self.capabilities.support(other);
                return match support {
                    ActionSupport::Unsupported { reason, detail } => {
                        InvocationOutcome::unsupported(reason, detail)
                    }
                    // Only the two launcher actions are declared, so this is
                    // unreachable; if a declaration is ever added without a
                    // route, saying so is better than silently doing nothing.
                    ActionSupport::Supported { .. } => InvocationOutcome::failed(
                        "session.declared_without_a_route",
                        format!(
                            "{} is declared but this adapter has no call for it",
                            other.key()
                        ),
                    ),
                };
            }
        };
        if progress.phase != GesturePhase::End {
            return InvocationOutcome::ignored("session.adapter_has_no_continuous_progress");
        }
        match self.registry.forward(self.instance.name(), request) {
            Ok(()) => InvocationOutcome::Invoked,
            Err(error) => InvocationOutcome::failed("session.activation_failed", error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use launcher_platform::PlatformError;
    use launcher_platform::activation::{FakeNameRegistry, NameOwnership};

    /// The adapter owns its registry, so a test that wants to read what was
    /// forwarded hands it a shared handle to one.
    struct Shared(Arc<FakeNameRegistry>);

    impl NameRegistry for Shared {
        fn request_name(&self, name: &str) -> Result<NameOwnership, PlatformError> {
            self.0.request_name(name)
        }

        fn forward(&self, name: &str, request: ActivationRequest) -> Result<(), PlatformError> {
            self.0.forward(name, request)
        }
    }

    #[test]
    fn opening_the_launcher_sends_the_activation_the_launcher_already_serves() {
        let recorded = Arc::new(FakeNameRegistry::new());
        let mut adapter = LauncherActivationAdapter::new(Box::new(Shared(recorded.clone())));
        assert_eq!(
            adapter.invoke(&DesktopAction::LauncherOpen, GestureProgress::completed()),
            InvocationOutcome::Invoked
        );
        assert_eq!(
            adapter.invoke(&DesktopAction::LauncherClose, GestureProgress::completed()),
            InvocationOutcome::Invoked
        );
        assert_eq!(
            recorded.forwarded(),
            vec![ActivationRequest::Open, ActivationRequest::Close]
        );
    }

    #[test]
    fn this_route_reaches_the_launcher_and_says_so_about_everything_else() {
        let mut adapter = LauncherActivationAdapter::new(Box::new(FakeNameRegistry::new()));
        for action in [
            DesktopAction::ShowDesktop,
            DesktopAction::ShowOverview,
            DesktopAction::NextWorkspace,
            DesktopAction::VolumeUp,
        ] {
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

    #[test]
    fn progress_before_the_end_of_the_gesture_is_ignored_rather_than_flapping_the_overlay() {
        let mut adapter = LauncherActivationAdapter::new(Box::new(FakeNameRegistry::new()));
        assert!(matches!(
            adapter.invoke(
                &DesktopAction::LauncherOpen,
                GestureProgress::new(GesturePhase::Update, 0.8)
            ),
            InvocationOutcome::Ignored { .. }
        ));
        assert!(!adapter.describe().continuous_progress);
        assert!(adapter.describe().performs_system_actions);
    }

    #[test]
    fn a_touchpad_progress_stream_drives_the_launchers_own_recognizer_unchanged() {
        use launcher_platform::activation::OverlayVisibility;
        use launcher_platform::gesture::{GestureOutcome, GestureRecognizer, GestureThresholds};

        // The launcher's default is five fingers; the Mac-style preset uses a
        // thumb and three, which is four contact points. That difference is
        // configuration, and it is the only thing the two sides have to agree
        // on beyond the event shape.
        let mut recognizer = GestureRecognizer::new(GestureThresholds {
            fingers: 4,
            ..GestureThresholds::default()
        });
        let now = std::time::Instant::now();
        let stream = [
            GestureProgress::new(GesturePhase::Begin, 0.05),
            GestureProgress::new(GesturePhase::Update, 0.45),
            GestureProgress::new(GesturePhase::Update, 0.72),
            GestureProgress::new(GesturePhase::End, 0.85),
        ];
        let outcomes: Vec<GestureOutcome> = stream
            .into_iter()
            .map(|progress| {
                recognizer.observe(
                    launcher_sample(progress, true, 4),
                    OverlayVisibility::Hidden,
                    now,
                )
            })
            .collect();

        assert_eq!(outcomes.last(), Some(&GestureOutcome::Open));
        assert!(
            outcomes[..3]
                .iter()
                .all(|outcome| matches!(outcome, GestureOutcome::Progress(_))),
            "{outcomes:?}"
        );
    }

    #[test]
    fn a_cancelled_touchpad_gesture_cancels_on_the_launcher_side_too() {
        use launcher_platform::activation::OverlayVisibility;
        use launcher_platform::gesture::{GestureOutcome, GestureRecognizer, GestureThresholds};

        let mut recognizer = GestureRecognizer::new(GestureThresholds {
            fingers: 4,
            ..GestureThresholds::default()
        });
        let now = std::time::Instant::now();
        recognizer.observe(
            launcher_sample(GestureProgress::new(GesturePhase::Begin, 0.3), true, 4),
            OverlayVisibility::Hidden,
            now,
        );
        assert_eq!(
            recognizer.observe(
                launcher_sample(GestureProgress::new(GesturePhase::Cancel, 0.3), true, 4),
                OverlayVisibility::Hidden,
                now,
            ),
            GestureOutcome::Cancelled
        );
    }

    #[test]
    fn an_unreachable_launcher_is_a_reported_failure_and_not_a_silent_one() {
        let registry = FakeNameRegistry::with_unreachable_owner(SingleInstance::DEFAULT_NAME);
        let mut adapter = LauncherActivationAdapter::new(Box::new(registry));
        assert!(matches!(
            adapter.invoke(&DesktopAction::LauncherOpen, GestureProgress::completed()),
            InvocationOutcome::Failed { .. }
        ));
    }
}
