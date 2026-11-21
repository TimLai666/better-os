//! The power menu (design/login-screen-spec.md section 3.12).
//!
//! Each item calls logind (`Suspend`, `Reboot`, `PowerOff`) and is shown only
//! when logind's `CanSuspend`, `CanReboot` or `CanPowerOff` answers `yes` or
//! `challenge`. On the login screen `challenge` means another person is
//! signed in: the greeter has no polkit agent to give an administrator's
//! password, so pressing Restart or Shut down says so instead of calling
//! logind. On the lock screen the person's own session is the one that ends,
//! so Restart and Shut down carry "Unsaved work will be lost".

use crate::copy::Language;
use crate::view::ScreenKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerAction {
    Sleep,
    Restart,
    ShutDown,
}

impl PowerAction {
    pub fn label(self, language: Language) -> &'static str {
        let copy = language.copy();
        match self {
            Self::Sleep => copy.sleep,
            Self::Restart => copy.restart,
            Self::ShutDown => copy.shut_down,
        }
    }

    /// The full-screen overlay's line while it happens.
    pub fn overlay(self, language: Language) -> &'static str {
        let copy = language.copy();
        match self {
            Self::Sleep => copy.going_to_sleep,
            Self::Restart => copy.restarting,
            Self::ShutDown => copy.shutting_down,
        }
    }

    /// The logind method.
    pub fn logind_method(self) -> &'static str {
        match self {
            Self::Sleep => "Suspend",
            Self::Restart => "Reboot",
            Self::ShutDown => "PowerOff",
        }
    }
}

/// logind's answer to a `Can…` call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    Yes,
    Challenge,
    No,
    NotApplicable,
}

impl Capability {
    /// logind's string; anything it does not document counts as `no`.
    pub fn from_logind(value: &str) -> Self {
        match value {
            "yes" => Self::Yes,
            "challenge" => Self::Challenge,
            "na" => Self::NotApplicable,
            _ => Self::No,
        }
    }
}

/// What pressing an item does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerResponse {
    /// Call logind and show the overlay.
    Call,
    /// Show this sentence in the message line instead.
    Refuse(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PowerItem {
    pub action: PowerAction,
    pub label: &'static str,
    /// The Caption under it.
    pub caption: Option<&'static str>,
    pub response: PowerResponse,
}

/// The menu's items in order: Sleep, Restart, Shut down.
pub fn power_menu(
    screen: ScreenKind,
    language: Language,
    can_suspend: Capability,
    can_reboot: Capability,
    can_power_off: Capability,
) -> Vec<PowerItem> {
    let copy = language.copy();
    [
        (PowerAction::Sleep, can_suspend),
        (PowerAction::Restart, can_reboot),
        (PowerAction::ShutDown, can_power_off),
    ]
    .into_iter()
    .filter(|(_, can)| matches!(can, Capability::Yes | Capability::Challenge))
    .map(|(action, can)| {
        let ends_session = action != PowerAction::Sleep;
        let response = if can == Capability::Challenge && ends_session {
            PowerResponse::Refuse(copy.someone_else_signed_in)
        } else {
            PowerResponse::Call
        };
        let caption =
            (screen == ScreenKind::Lock && ends_session).then_some(copy.unsaved_work_lost);
        PowerItem {
            action,
            label: action.label(language),
            caption,
            response,
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_follow_logind() {
        let menu = power_menu(
            ScreenKind::Login,
            Language::ZhTw,
            Capability::Yes,
            Capability::from_logind("yes"),
            Capability::from_logind("no"),
        );
        let actions: Vec<PowerAction> = menu.iter().map(|item| item.action).collect();
        assert_eq!(actions, [PowerAction::Sleep, PowerAction::Restart]);
        assert_eq!(menu[1].label, "重新開機");
        assert_eq!(menu[1].caption, None);
        assert_eq!(menu[1].response, PowerResponse::Call);
        assert!(
            power_menu(
                ScreenKind::Login,
                Language::EnUs,
                Capability::from_logind("na"),
                Capability::from_logind("garbage"),
                Capability::No
            )
            .is_empty()
        );
    }

    #[test]
    fn a_challenge_means_someone_else_is_signed_in() {
        let menu = power_menu(
            ScreenKind::Login,
            Language::EnUs,
            Capability::Challenge,
            Capability::Challenge,
            Capability::Challenge,
        );
        assert_eq!(menu[0].response, PowerResponse::Call);
        for item in &menu[1..] {
            assert_eq!(
                item.response,
                PowerResponse::Refuse(
                    "Someone else is still signed in, so you can't restart or shut down from here."
                )
            );
        }
    }

    #[test]
    fn the_lock_screen_warns_about_unsaved_work() {
        let menu = power_menu(
            ScreenKind::Lock,
            Language::ZhTw,
            Capability::Yes,
            Capability::Yes,
            Capability::Yes,
        );
        assert_eq!(menu[0].caption, None);
        assert_eq!(menu[1].caption, Some("未儲存的工作會遺失"));
        assert_eq!(menu[2].caption, Some("未儲存的工作會遺失"));
        assert_eq!(
            PowerAction::ShutDown.overlay(Language::EnUs),
            "Shutting down…"
        );
        assert_eq!(PowerAction::Restart.logind_method(), "Reboot");
    }
}
