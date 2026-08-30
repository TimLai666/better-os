//! The recording adapter.
//!
//! It is two things at once, and deliberately so: the seam every test in
//! `touchpad-gestures` and `touchpad-gui` runs against, and the adapter the
//! shipped Test gestures mode uses. Both want the same behaviour — take a typed
//! action, write it down, change nothing — so there is one implementation
//! rather than a test double that drifts from what the screen actually runs.

use std::collections::BTreeSet;

use better_actions::{ActionCapabilities, DesktopAction};

use crate::adapter::{
    AdapterDescription, GesturePhase, GestureProgress, InvocationOutcome, SessionAdapter,
};

/// One recorded call.
#[derive(Clone, Debug, PartialEq)]
pub struct Invocation {
    pub action: DesktopAction,
    pub progress: GestureProgress,
}

/// An adapter that records what it was asked to do and does none of it.
#[derive(Clone, Debug)]
pub struct MockSessionAdapter {
    name: String,
    capabilities: ActionCapabilities,
    invocations: Vec<Invocation>,
    bound: BTreeSet<String>,
    /// Actions this adapter refuses at invocation time even though it declares
    /// support. This is the failing-adapter case, which has to be reachable in
    /// a test or the automatic-disable rule is untested.
    failing: BTreeSet<String>,
    /// Whether intermediate phases are recorded. A discrete adapter ignores
    /// everything until the gesture ends.
    continuous_progress: bool,
}

impl Default for MockSessionAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSessionAdapter {
    /// An adapter that can do everything, with progress where the action could
    /// use it.
    pub fn new() -> Self {
        Self {
            name: "mock".to_string(),
            capabilities: ActionCapabilities::everything(),
            invocations: Vec::new(),
            bound: BTreeSet::new(),
            failing: BTreeSet::new(),
            continuous_progress: true,
        }
    }

    /// An adapter that can do exactly what it was given and nothing else.
    pub fn with_capabilities(capabilities: ActionCapabilities) -> Self {
        Self {
            capabilities,
            ..Self::new()
        }
    }

    /// An adapter standing in for an integration path that reports only the
    /// completed gesture.
    pub fn without_progress(mut self) -> Self {
        self.continuous_progress = false;
        self
    }

    /// Marks an action as one this adapter accepts and then fails.
    pub fn failing(mut self, action: &DesktopAction) -> Self {
        self.failing.insert(action.key().to_string());
        self
    }

    pub fn invocations(&self) -> &[Invocation] {
        &self.invocations
    }

    /// The actions invoked, in order, as machine keys. The shape most
    /// assertions want.
    pub fn invoked_keys(&self) -> Vec<&'static str> {
        self.invocations
            .iter()
            .map(|invocation| invocation.action.key())
            .collect()
    }

    pub fn bound_keys(&self) -> Vec<String> {
        self.bound.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.invocations.clear();
    }
}

impl SessionAdapter for MockSessionAdapter {
    fn describe(&self) -> AdapterDescription {
        AdapterDescription {
            name: self.name.clone(),
            continuous_progress: self.continuous_progress,
            // The point of this adapter: nothing outside its own memory moves.
            performs_system_actions: false,
        }
    }

    fn capabilities(&self) -> &ActionCapabilities {
        &self.capabilities
    }

    fn invoke(&mut self, action: &DesktopAction, progress: GestureProgress) -> InvocationOutcome {
        if matches!(action, DesktopAction::Disabled) {
            return InvocationOutcome::ignored("session.action_disabled");
        }
        if !self.capabilities.is_supported(action) {
            let support = self.capabilities.support(action);
            let (reason, detail) = match support {
                better_actions::ActionSupport::Unsupported { reason, detail } => (reason, detail),
                better_actions::ActionSupport::Supported { .. } => unreachable!("just checked"),
            };
            return InvocationOutcome::unsupported(reason, detail);
        }
        if !self.continuous_progress && progress.phase != GesturePhase::End {
            return InvocationOutcome::ignored("session.adapter_has_no_continuous_progress");
        }
        if self.failing.contains(action.key()) {
            return InvocationOutcome::failed(
                "session.adapter_refused",
                format!("the mock adapter is set to fail {}", action.key()),
            );
        }
        self.invocations.push(Invocation {
            action: action.clone(),
            progress,
        });
        InvocationOutcome::Invoked
    }

    fn bind(&mut self, action: &DesktopAction) -> crate::adapter::BindOutcome {
        match self.support(action) {
            better_actions::ActionSupport::Supported { .. } => {
                if self.failing.contains(action.key()) {
                    crate::adapter::BindOutcome::Failed {
                        reason: "session.adapter_refused".to_string(),
                        detail: format!("the mock adapter is set to fail {}", action.key()),
                    }
                } else {
                    self.bound.insert(action.key().to_string());
                    crate::adapter::BindOutcome::Bound
                }
            }
            better_actions::ActionSupport::Unsupported { reason, detail } => {
                crate::adapter::BindOutcome::Unsupported { reason, detail }
            }
        }
    }

    fn verify(&self, action: &DesktopAction) -> crate::adapter::VerificationResult {
        match self.support(action) {
            better_actions::ActionSupport::Supported {
                continuous_progress,
            } => {
                if self.bound.contains(action.key()) {
                    crate::adapter::VerificationResult::Verified {
                        continuous_progress: continuous_progress && self.continuous_progress,
                    }
                } else {
                    // Supported, but nobody bound it. Reporting this as
                    // verified would be the exact lie the read-back rule
                    // exists to prevent.
                    crate::adapter::VerificationResult::Unverified {
                        reason: "session.binding_absent".to_string(),
                        detail: "the adapter holds no binding for this action".to_string(),
                    }
                }
            }
            better_actions::ActionSupport::Unsupported { reason, detail } => {
                crate::adapter::VerificationResult::Unsupported { reason, detail }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use better_actions::ActionSupport;

    use crate::adapter::{BindOutcome, VerificationResult};

    #[test]
    fn every_invocation_is_recorded_with_the_progress_that_caused_it() {
        let mut adapter = MockSessionAdapter::new();
        adapter.invoke(
            &DesktopAction::LauncherOpen,
            GestureProgress::new(GesturePhase::Update, 0.4),
        );
        adapter.invoke(&DesktopAction::ShowDesktop, GestureProgress::completed());

        assert_eq!(
            adapter.invoked_keys(),
            vec!["better-launcher.open", "desktop.show"]
        );
        assert_eq!(adapter.invocations()[0].progress.fraction, 0.4);
        assert!(adapter.invocations()[1].progress.is_final());
    }

    #[test]
    fn an_action_the_adapter_cannot_do_is_reported_rather_than_recorded() {
        let mut adapter = MockSessionAdapter::with_capabilities(
            ActionCapabilities::new().with(&DesktopAction::LauncherOpen, ActionSupport::discrete()),
        );
        let outcome = adapter.invoke(&DesktopAction::ShowOverview, GestureProgress::completed());
        assert!(matches!(outcome, InvocationOutcome::Unsupported { .. }));
        assert!(adapter.invocations().is_empty());
    }

    #[test]
    fn a_disabled_action_does_nothing_and_says_which_nothing_it_did() {
        let mut adapter = MockSessionAdapter::new();
        assert_eq!(
            adapter.invoke(&DesktopAction::Disabled, GestureProgress::completed()),
            InvocationOutcome::ignored("session.action_disabled")
        );
        assert!(adapter.invocations().is_empty());
    }

    #[test]
    fn an_adapter_with_no_continuous_progress_only_acts_on_the_end_of_a_gesture() {
        let mut adapter = MockSessionAdapter::new().without_progress();
        assert!(matches!(
            adapter.invoke(
                &DesktopAction::LauncherOpen,
                GestureProgress::new(GesturePhase::Update, 0.9)
            ),
            InvocationOutcome::Ignored { .. }
        ));
        assert_eq!(
            adapter.invoke(&DesktopAction::LauncherOpen, GestureProgress::completed()),
            InvocationOutcome::Invoked
        );
        assert_eq!(adapter.invoked_keys(), vec!["better-launcher.open"]);
    }

    #[test]
    fn a_supported_action_nobody_bound_verifies_as_absent_rather_than_as_working() {
        let mut adapter = MockSessionAdapter::new();
        assert!(matches!(
            adapter.verify(&DesktopAction::LauncherOpen),
            VerificationResult::Unverified { .. }
        ));
        assert_eq!(
            adapter.bind(&DesktopAction::LauncherOpen),
            BindOutcome::Bound
        );
        assert!(adapter.verify(&DesktopAction::LauncherOpen).is_verified());
        assert_eq!(adapter.bound_keys(), vec!["better-launcher.open"]);
    }

    #[test]
    fn a_failing_adapter_fails_both_binding_and_invoking_and_records_neither() {
        let mut adapter = MockSessionAdapter::new().failing(&DesktopAction::ShowDesktop);
        assert!(matches!(
            adapter.bind(&DesktopAction::ShowDesktop),
            BindOutcome::Failed { .. }
        ));
        assert!(matches!(
            adapter.invoke(&DesktopAction::ShowDesktop, GestureProgress::completed()),
            InvocationOutcome::Failed { .. }
        ));
        assert!(adapter.invocations().is_empty());
        assert!(adapter.bound_keys().is_empty());
    }

    #[test]
    fn the_mock_says_out_loud_that_it_changes_nothing() {
        let description = MockSessionAdapter::new().describe();
        assert!(!description.performs_system_actions);
        assert!(description.continuous_progress);
        assert_eq!(description.name, "mock");
    }

    #[test]
    fn progress_is_clamped_so_no_caller_can_report_past_the_end() {
        assert_eq!(
            GestureProgress::new(GesturePhase::Update, 4.0).fraction,
            1.0
        );
        assert_eq!(
            GestureProgress::new(GesturePhase::Update, -2.0).fraction,
            0.0
        );
    }
}
