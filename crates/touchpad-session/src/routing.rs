//! One adapter made of several, because no single route reaches everything.
//!
//! Better Launcher is reached through the activation interface it already
//! serves, and the desktop is reached through the GNOME Shell adapter
//! extension. Those are two different transports and neither can do the other's
//! job, so something has to decide which one an action goes to. This is that
//! something, and the rule is the only one that cannot drift: an action goes to
//! the first adapter that says it supports it.
//!
//! It carries no table of its own. A capability report is the single source of
//! truth for what an adapter can do, and the routing is derived from it, so an
//! adapter that gains an action gains the routing with it.

use better_actions::{ActionCapabilities, ActionSupport, DesktopAction};

use crate::adapter::{
    AdapterDescription, GestureProgress, InvocationOutcome, SessionAdapter, SuppressionOutcome,
};

/// Whether a refusal was written by an adapter or produced by the default for
/// an action it never mentioned.
fn is_stated(support: &ActionSupport) -> bool {
    !matches!(
        support,
        ActionSupport::Unsupported { reason, .. } if reason == "actions.adapter_declares_no_support"
    )
}

/// Routes each action to the first adapter that declares it.
pub struct RoutingAdapter {
    adapters: Vec<Box<dyn SessionAdapter>>,
    capabilities: ActionCapabilities,
}

impl RoutingAdapter {
    pub fn new(adapters: Vec<Box<dyn SessionAdapter>>) -> Self {
        // Merged in order: the first adapter that supports an action owns it.
        // Where nobody does, the refusal a route actually wrote beats the
        // default one an adapter gets for never mentioning the action, so the
        // reason a user reads says something.
        let mut capabilities = ActionCapabilities::new();
        for action in DesktopAction::catalog() {
            let mut chosen: Option<ActionSupport> = None;
            for adapter in &adapters {
                let support = adapter.support(&action);
                let supported = support.is_supported();
                let better = match &chosen {
                    None => true,
                    Some(_) if supported => true,
                    Some(existing) => !existing.is_supported() && is_stated(&support),
                };
                if better {
                    chosen = Some(support);
                }
                if supported {
                    break;
                }
            }
            if let Some(support) = chosen {
                capabilities.insert(&action, support);
            }
        }
        Self {
            adapters,
            capabilities,
        }
    }

    fn route(&mut self, action: &DesktopAction) -> Option<&mut Box<dyn SessionAdapter>> {
        self.adapters
            .iter_mut()
            .find(|adapter| adapter.support(action).is_supported())
    }
}

impl SessionAdapter for RoutingAdapter {
    fn describe(&self) -> AdapterDescription {
        let names: Vec<String> = self
            .adapters
            .iter()
            .map(|adapter| adapter.describe().name)
            .collect();
        AdapterDescription {
            name: names.join(" + "),
            continuous_progress: self
                .adapters
                .iter()
                .any(|adapter| adapter.describe().continuous_progress),
            // One route that reaches a real desktop is enough for the screen to
            // stop saying that nothing does.
            performs_system_actions: self
                .adapters
                .iter()
                .any(|adapter| adapter.describe().performs_system_actions),
        }
    }

    fn capabilities(&self) -> &ActionCapabilities {
        &self.capabilities
    }

    fn invoke(&mut self, action: &DesktopAction, progress: GestureProgress) -> InvocationOutcome {
        match self.route(action) {
            Some(adapter) => adapter.invoke(action, progress),
            None => match self.capabilities.support(action) {
                ActionSupport::Unsupported { reason, detail } => {
                    InvocationOutcome::unsupported(reason, detail)
                }
                ActionSupport::Supported { .. } => InvocationOutcome::failed(
                    "session.declared_without_a_route",
                    format!(
                        "{} is declared and no adapter behind this one will perform it",
                        action.key()
                    ),
                ),
            },
        }
    }

    /// Asks every adapter, and reports the best answer any of them gave.
    ///
    /// Best rather than first, because only one route can suppress a GNOME
    /// gesture and the others will all say they cannot. A refusal from a route
    /// that was never going to be able to do it is not the answer the user
    /// needs.
    fn suppress_built_in_gestures(&mut self, suppress: bool) -> SuppressionOutcome {
        let mut best = SuppressionOutcome::unsupported();
        for adapter in &mut self.adapters {
            let outcome = adapter.suppress_built_in_gestures(suppress);
            best = match (&best, &outcome) {
                (SuppressionOutcome::Suppressed | SuppressionOutcome::Restored, _) => best,
                (_, SuppressionOutcome::Suppressed | SuppressionOutcome::Restored) => outcome,
                (SuppressionOutcome::Failed { .. }, _) => best,
                (_, SuppressionOutcome::Failed { .. }) => outcome,
                _ => best,
            };
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gnome::{FakeShellBridge, GnomeShellAdapter};
    use crate::mock::MockSessionAdapter;

    fn shell() -> GnomeShellAdapter {
        GnomeShellAdapter::connect(Box::new(FakeShellBridge::new())).expect("a shell adapter")
    }

    /// A stand-in for the launcher route, so this test needs no bus and no
    /// `launcher-activation` feature.
    struct LauncherOnly {
        capabilities: ActionCapabilities,
        invoked: Vec<String>,
    }

    impl LauncherOnly {
        fn new() -> Self {
            Self {
                capabilities: ActionCapabilities::new()
                    .with(&DesktopAction::LauncherOpen, ActionSupport::discrete())
                    .with(&DesktopAction::LauncherClose, ActionSupport::discrete()),
                invoked: Vec::new(),
            }
        }
    }

    impl SessionAdapter for LauncherOnly {
        fn describe(&self) -> AdapterDescription {
            AdapterDescription {
                name: "launcher-only".to_string(),
                continuous_progress: false,
                performs_system_actions: true,
            }
        }

        fn capabilities(&self) -> &ActionCapabilities {
            &self.capabilities
        }

        fn invoke(
            &mut self,
            action: &DesktopAction,
            _progress: GestureProgress,
        ) -> InvocationOutcome {
            self.invoked.push(action.key().to_string());
            InvocationOutcome::Invoked
        }
    }

    #[test]
    fn the_launcher_goes_to_the_launcher_and_the_desktop_goes_to_the_shell() {
        let recorded = std::sync::Arc::new(FakeShellBridge::new());
        let mut routing = RoutingAdapter::new(vec![
            Box::new(LauncherOnly::new()),
            Box::new(
                GnomeShellAdapter::connect(Box::new(crate::gnome::SharedShellBridge(
                    recorded.clone(),
                )))
                .unwrap(),
            ),
        ]);

        assert_eq!(
            routing.invoke(&DesktopAction::LauncherOpen, GestureProgress::completed()),
            InvocationOutcome::Invoked
        );
        assert!(
            recorded.calls().is_empty(),
            "the launcher reached the shell"
        );

        assert_eq!(
            routing.invoke(&DesktopAction::ShowOverview, GestureProgress::completed()),
            InvocationOutcome::Invoked
        );
        assert_eq!(recorded.calls().len(), 1);
    }

    #[test]
    fn an_action_neither_route_performs_keeps_a_real_reason() {
        let routing = RoutingAdapter::new(vec![Box::new(LauncherOnly::new()), Box::new(shell())]);
        let support = routing.support(&DesktopAction::CurrentApplicationWindows);
        assert_eq!(
            support.detail(),
            Some(
                "GNOME 46's window picker is the overview itself and cannot be \
                 filtered to the focused application"
            )
        );
        assert!(
            !routing
                .support(&DesktopAction::ApplicationZoom)
                .is_supported()
        );
    }

    #[test]
    fn suppression_is_the_answer_of_the_one_route_that_can_do_it() {
        let mut routing =
            RoutingAdapter::new(vec![Box::new(LauncherOnly::new()), Box::new(shell())]);
        assert_eq!(
            routing.suppress_built_in_gestures(true),
            SuppressionOutcome::Suppressed
        );
        assert_eq!(
            routing.suppress_built_in_gestures(false),
            SuppressionOutcome::Restored
        );
    }

    #[test]
    fn with_no_shell_route_suppression_is_unsupported_rather_than_assumed() {
        let mut routing = RoutingAdapter::new(vec![
            Box::new(LauncherOnly::new()),
            Box::new(MockSessionAdapter::new()),
        ]);
        assert!(matches!(
            routing.suppress_built_in_gestures(true),
            SuppressionOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn the_description_names_every_route_behind_it() {
        let routing = RoutingAdapter::new(vec![Box::new(LauncherOnly::new()), Box::new(shell())]);
        let described = routing.describe();
        assert!(described.name.contains("launcher-only"));
        assert!(described.name.contains("gnome-shell"));
        assert!(described.performs_system_actions);
    }
}
