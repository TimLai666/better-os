//! What an adapter says it can do, action by action.
//!
//! The shape is the one `touchpad-core::Capabilities` already uses for
//! settings, for the same reason: an action an adapter never mentioned is
//! unsupported, not assumed working. A partial declaration is therefore safe by
//! default, and the screen renders an explanation rather than a control that
//! does nothing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::catalog::DesktopAction;

/// Whether an adapter can perform one action.
///
/// There is no third state. Either the adapter performs the action — and says
/// whether it can follow gesture progress while doing so — or it says why not.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "support", rename_all = "snake_case")]
pub enum ActionSupport {
    Supported {
        /// Whether the adapter delivers intermediate progress. Discrete
        /// activation is a supported answer; it just cannot animate.
        continuous_progress: bool,
    },
    Unsupported {
        reason: String,
        detail: String,
    },
}

impl ActionSupport {
    pub fn discrete() -> Self {
        Self::Supported {
            continuous_progress: false,
        }
    }

    pub fn continuous() -> Self {
        Self::Supported {
            continuous_progress: true,
        }
    }

    pub fn unsupported(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
            detail: detail.into(),
        }
    }

    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Supported { .. })
    }

    pub fn follows_progress(&self) -> bool {
        matches!(
            self,
            Self::Supported {
                continuous_progress: true
            }
        )
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Supported { .. } => None,
            Self::Unsupported { detail, .. } => Some(detail),
        }
    }
}

/// One adapter's answer for every action it was asked about.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionCapabilities {
    entries: BTreeMap<String, ActionSupport>,
}

impl ActionCapabilities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, action: &DesktopAction, support: ActionSupport) -> Self {
        self.insert(action, support);
        self
    }

    pub fn insert(&mut self, action: &DesktopAction, support: ActionSupport) {
        self.entries.insert(action.key().to_string(), support);
    }

    /// Every action supported, with progress where the action itself could use
    /// it. Only an adapter that genuinely does all of it may build one; the
    /// mock adapter and its tests are the callers today.
    pub fn everything() -> Self {
        let mut capabilities = Self::new();
        for action in DesktopAction::catalog() {
            let support = if action.follows_progress() {
                ActionSupport::continuous()
            } else {
                ActionSupport::discrete()
            };
            capabilities.insert(&action, support);
        }
        capabilities
    }

    pub fn support(&self, action: &DesktopAction) -> ActionSupport {
        self.entries.get(action.key()).cloned().unwrap_or_else(|| {
            ActionSupport::unsupported(
                "actions.adapter_declares_no_support",
                "the active adapter did not declare this action at all",
            )
        })
    }

    pub fn is_supported(&self, action: &DesktopAction) -> bool {
        self.support(action).is_supported()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_action_the_adapter_never_mentioned_is_unsupported_rather_than_assumed() {
        let capabilities =
            ActionCapabilities::new().with(&DesktopAction::LauncherOpen, ActionSupport::discrete());
        assert!(capabilities.is_supported(&DesktopAction::LauncherOpen));
        assert!(!capabilities.is_supported(&DesktopAction::ShowDesktop));
        assert_eq!(
            capabilities.support(&DesktopAction::ShowDesktop).detail(),
            Some("the active adapter did not declare this action at all")
        );
    }

    #[test]
    fn a_full_declaration_only_promises_progress_where_the_action_could_use_it() {
        let capabilities = ActionCapabilities::everything();
        assert!(
            capabilities
                .support(&DesktopAction::ShowOverview)
                .follows_progress()
        );
        assert!(capabilities.is_supported(&DesktopAction::VolumeUp));
        assert!(
            !capabilities
                .support(&DesktopAction::VolumeUp)
                .follows_progress()
        );
    }

    #[test]
    fn declaring_a_shortcut_covers_every_shortcut_because_the_route_is_the_same() {
        let one = DesktopAction::KeyboardShortcut {
            shortcut: DesktopAction::placeholder_shortcut(),
        };
        let capabilities = ActionCapabilities::new().with(&one, ActionSupport::discrete());
        let another = DesktopAction::KeyboardShortcut {
            shortcut: crate::key::KeyboardShortcut::parse("<Ctrl><Alt>t").unwrap(),
        };
        assert!(capabilities.is_supported(&another));
    }
}
