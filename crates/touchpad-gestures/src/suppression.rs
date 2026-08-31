//! Whether GNOME keeps its own gestures, and who decided.
//!
//! Issue #3 forbids replacing a desktop gesture silently, and the preview and
//! the confirmation already make that true on the way in. This is the other
//! half: once a confirmed plan has taken a gesture away from GNOME, something
//! has to remember that it did, and has to give it back on every path out —
//! restoring a capture, turning gestures off, entering safe mode, and removing
//! the component.
//!
//! It is a state machine rather than a pair of calls because the failure that
//! matters is the one nobody notices. A restore that was never attempted, and a
//! restore that was attempted and failed, look identical to a user whose
//! overview stopped working; they are different states here, and the second one
//! keeps saying so until it succeeds.
//!
//! The state lives in this process, not in a file. The extension restores the
//! desktop's own gestures when it is disabled or the shell restarts, so a
//! Better OS process that dies without restoring costs nothing — which is why
//! there is no persisted flag here to go stale.

use touchpad_session::{SessionAdapter, SuppressionOutcome};

/// What happened to the machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SuppressionEvent {
    /// A confirmed plan was applied. `wanted` is whether any conflict in it was
    /// resolved by taking the gesture away from the desktop.
    PlanApplied { wanted: bool },
    /// The captured configuration was put back.
    Restored,
    /// The integration was switched off, including by the automatic disable
    /// after a repeatedly failing adapter.
    Disabled,
    /// Safe mode. Better Touchpad reads and changes nothing, so the desktop
    /// gets its gestures back before anything else happens.
    SafeMode,
    /// The component is being removed.
    Uninstalled,
}

impl SuppressionEvent {
    /// Whether the desktop's own gestures should be off after this event.
    ///
    /// Every way out restores. That is the whole rule, and it is written once
    /// so that adding a way out cannot forget it.
    pub fn wants_suppression(self) -> bool {
        match self {
            Self::PlanApplied { wanted } => wanted,
            Self::Restored | Self::Disabled | Self::SafeMode | Self::Uninstalled => false,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::PlanApplied { .. } => "plan-applied",
            Self::Restored => "restored",
            Self::Disabled => "disabled",
            Self::SafeMode => "safe-mode",
            Self::Uninstalled => "uninstalled",
        }
    }
}

/// Whether the desktop's own gestures are currently suppressed, and what the
/// last attempt to change that said.
#[derive(Clone, Debug, Default)]
pub struct SuppressionState {
    /// What the adapter last confirmed. `None` until anything has been asked.
    confirmed: Option<bool>,
    /// What the last transition wanted, whether or not it worked.
    wanted: bool,
    last: Option<SuppressionOutcome>,
}

impl SuppressionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the desktop's own gestures are off, as far as anything here
    /// knows. `false` before the first successful call, because nothing has
    /// been suppressed until something says it has.
    pub fn is_suppressed(&self) -> bool {
        self.confirmed.unwrap_or(false)
    }

    /// Whether the state the adapter confirmed is the state that was wanted. A
    /// failed restore leaves this false, and it stays false until a later
    /// attempt succeeds.
    pub fn is_settled(&self) -> bool {
        self.confirmed == Some(self.wanted)
    }

    pub fn last_outcome(&self) -> Option<&SuppressionOutcome> {
        self.last.as_ref()
    }

    /// Applies an event, calling the adapter only where it would change
    /// something.
    ///
    /// "Would change something" includes the case where the last attempt
    /// failed: a restore that did not happen is retried on the next event
    /// rather than being assumed to have caught up on its own.
    pub fn transition(
        &mut self,
        event: SuppressionEvent,
        adapter: &mut dyn SessionAdapter,
    ) -> SuppressionOutcome {
        let wanted = event.wants_suppression();
        self.wanted = wanted;
        if self.confirmed == Some(wanted) {
            let outcome = if wanted {
                SuppressionOutcome::Suppressed
            } else {
                SuppressionOutcome::Restored
            };
            self.last = Some(outcome.clone());
            return outcome;
        }
        // Nothing was ever suppressed and nothing is being asked for: there is
        // no call to make, and pretending to have restored gestures that were
        // never taken would be a claim about somebody else's software.
        if self.confirmed.is_none() && !wanted {
            let outcome = SuppressionOutcome::Restored;
            self.last = Some(outcome.clone());
            return outcome;
        }

        let outcome = adapter.suppress_built_in_gestures(wanted);
        match &outcome {
            SuppressionOutcome::Suppressed => self.confirmed = Some(true),
            SuppressionOutcome::Restored => self.confirmed = Some(false),
            // An adapter that cannot do this never took the gesture away in the
            // first place, so there is nothing outstanding to put back.
            SuppressionOutcome::Unsupported { .. } => {
                if !wanted {
                    self.confirmed = None;
                }
            }
            SuppressionOutcome::Failed { .. } => {}
        }
        self.last = Some(outcome.clone());
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use touchpad_session::gnome::{FakeShellBridge, SharedShellBridge, ShellError, ShellRequest};
    use touchpad_session::{GnomeShellAdapter, MockSessionAdapter};

    fn shell(bridge: Arc<FakeShellBridge>) -> GnomeShellAdapter {
        GnomeShellAdapter::connect(Box::new(SharedShellBridge(bridge))).expect("a shell adapter")
    }

    #[test]
    fn a_plan_that_takes_a_gesture_from_the_desktop_suppresses_it() {
        let recorded = Arc::new(FakeShellBridge::new());
        let mut adapter = shell(recorded.clone());
        let mut state = SuppressionState::new();

        assert_eq!(
            state.transition(SuppressionEvent::PlanApplied { wanted: true }, &mut adapter),
            SuppressionOutcome::Suppressed
        );
        assert!(state.is_suppressed());
        assert!(state.is_settled());
        assert_eq!(
            recorded.calls(),
            vec![ShellRequest::SuppressBuiltInGestures(true)]
        );
    }

    #[test]
    fn a_plan_that_keeps_every_desktop_gesture_changes_nothing() {
        let recorded = Arc::new(FakeShellBridge::new());
        let mut adapter = shell(recorded.clone());
        let mut state = SuppressionState::new();

        assert_eq!(
            state.transition(
                SuppressionEvent::PlanApplied { wanted: false },
                &mut adapter
            ),
            SuppressionOutcome::Restored
        );
        assert!(!state.is_suppressed());
        assert!(
            recorded.calls().is_empty(),
            "the desktop was called about a gesture nobody took"
        );
    }

    #[test]
    fn every_way_out_gives_the_desktop_its_gestures_back() {
        for event in [
            SuppressionEvent::Restored,
            SuppressionEvent::Disabled,
            SuppressionEvent::SafeMode,
            SuppressionEvent::Uninstalled,
        ] {
            let recorded = Arc::new(FakeShellBridge::new());
            let mut adapter = shell(recorded.clone());
            let mut state = SuppressionState::new();
            state.transition(SuppressionEvent::PlanApplied { wanted: true }, &mut adapter);

            assert_eq!(
                state.transition(event, &mut adapter),
                SuppressionOutcome::Restored,
                "{}",
                event.key()
            );
            assert!(!state.is_suppressed(), "{}", event.key());
            assert_eq!(
                recorded.calls(),
                vec![
                    ShellRequest::SuppressBuiltInGestures(true),
                    ShellRequest::SuppressBuiltInGestures(false),
                ],
                "{}",
                event.key()
            );
        }
    }

    #[test]
    fn asking_twice_for_the_same_state_makes_one_call() {
        let recorded = Arc::new(FakeShellBridge::new());
        let mut adapter = shell(recorded.clone());
        let mut state = SuppressionState::new();
        for _ in 0..3 {
            state.transition(SuppressionEvent::PlanApplied { wanted: true }, &mut adapter);
        }
        assert_eq!(recorded.calls().len(), 1);
    }

    #[test]
    fn a_restore_that_failed_is_retried_rather_than_assumed() {
        let recorded = Arc::new(FakeShellBridge::new());
        let mut adapter = shell(recorded.clone());
        let mut state = SuppressionState::new();
        state.transition(SuppressionEvent::PlanApplied { wanted: true }, &mut adapter);

        // The shell stops answering, and the restore fails.
        let mut broken = GnomeShellAdapter::with_reported(
            Box::new(FakeShellBridge::failing(ShellError::CallFailed(
                "the shell did not answer".to_string(),
            ))),
            FakeShellBridge::gnome_46_capabilities(),
        );
        let mut carried = state.clone();
        assert!(matches!(
            carried.transition(SuppressionEvent::Disabled, &mut broken),
            SuppressionOutcome::Failed { .. }
        ));
        assert!(
            carried.is_suppressed(),
            "a failed restore must not report the desktop as restored"
        );
        assert!(!carried.is_settled());

        // The next way out tries again rather than believing the first one.
        assert_eq!(
            carried.transition(SuppressionEvent::SafeMode, &mut adapter),
            SuppressionOutcome::Restored
        );
        assert!(carried.is_settled());
    }

    #[test]
    fn an_adapter_that_cannot_suppress_anything_says_so_and_stays_unsuppressed() {
        let mut adapter = MockSessionAdapter::new();
        let mut state = SuppressionState::new();
        assert!(matches!(
            state.transition(SuppressionEvent::PlanApplied { wanted: true }, &mut adapter),
            SuppressionOutcome::Unsupported { .. }
        ));
        assert!(!state.is_suppressed());
        // And the way out reports the truth: nothing was taken, so nothing is
        // outstanding.
        assert_eq!(
            state.transition(SuppressionEvent::Disabled, &mut adapter),
            SuppressionOutcome::Restored
        );
    }
}
