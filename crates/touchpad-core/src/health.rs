//! The startup health check.
//!
//! A touchpad control centre that cannot read the touchpad is worse than one
//! that says so: the user changes a slider, nothing happens, and they have no
//! way to tell whether the change or the reading was wrong. Every check here
//! answers one question the Diagnostics screen shows verbatim.
//!
//! The checks take facts rather than gathering them, so this crate needs no
//! filesystem, session, or bus of its own and every combination is testable.

use serde::{Deserialize, Serialize};

use crate::settings::{Capabilities, SettingId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    /// Working.
    Ok,
    /// Usable, with something the user should know.
    Degraded,
    /// Better Touchpad cannot do its job.
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthCheck {
    /// A stable machine key. Presentation layers own the wording. It is set
    /// from a `&'static str` so a check cannot invent a key at runtime.
    pub id: String,
    pub state: HealthState,
    pub detail: String,
}

impl HealthCheck {
    pub fn new(id: &'static str, state: HealthState, detail: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            state,
            detail: detail.into(),
        }
    }
}

/// The facts the checks are decided from.
pub struct HealthFacts<'a> {
    pub configuration_readable: bool,
    pub configuration_detail: String,
    pub backend_name: &'a str,
    pub backend_reachable: bool,
    pub backend_detail: String,
    pub devices_found: usize,
    pub selected_device: Option<&'a str>,
    pub capabilities: &'a Capabilities,
    pub capture_present: bool,
    pub safe_mode: bool,
    pub integration_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthReport {
    pub checks: Vec<HealthCheck>,
}

impl HealthReport {
    /// The worst state any check reported, which is the one word the Overview
    /// screen shows.
    pub fn state(&self) -> HealthState {
        self.checks
            .iter()
            .map(|check| check.state)
            .max()
            .unwrap_or(HealthState::Ok)
    }

    pub fn check(&self, id: &str) -> Option<&HealthCheck> {
        self.checks.iter().find(|check| check.id == id)
    }

    pub fn evaluate(facts: &HealthFacts<'_>) -> Self {
        let mut checks = Vec::new();

        checks.push(if facts.configuration_readable {
            HealthCheck::new(
                "touchpad.configuration",
                HealthState::Ok,
                facts.configuration_detail.clone(),
            )
        } else {
            HealthCheck::new(
                "touchpad.configuration",
                HealthState::Failed,
                facts.configuration_detail.clone(),
            )
        });

        checks.push(if facts.backend_reachable {
            HealthCheck::new(
                "touchpad.backend",
                HealthState::Ok,
                facts.backend_detail.clone(),
            )
        } else {
            HealthCheck::new(
                "touchpad.backend",
                HealthState::Failed,
                facts.backend_detail.clone(),
            )
        });

        checks.push(match (facts.devices_found, facts.selected_device) {
            (0, _) => HealthCheck::new(
                "touchpad.device",
                HealthState::Failed,
                "no touchpad was found on this system",
            ),
            (found, None) => HealthCheck::new(
                "touchpad.device",
                HealthState::Degraded,
                format!("{found} touchpad(s) found, none selected"),
            ),
            (found, Some(identity)) => HealthCheck::new(
                "touchpad.device",
                HealthState::Ok,
                format!("{found} touchpad(s) found, using {identity}"),
            ),
        });

        let available = facts.capabilities.available().len();
        checks.push(match available {
            0 => HealthCheck::new(
                "touchpad.capabilities",
                HealthState::Failed,
                format!("{} reads and applies nothing", facts.backend_name),
            ),
            found if found == SettingId::ALL.len() => HealthCheck::new(
                "touchpad.capabilities",
                HealthState::Ok,
                format!("{found} of {} controls available", SettingId::ALL.len()),
            ),
            found => HealthCheck::new(
                "touchpad.capabilities",
                HealthState::Degraded,
                format!("{found} of {} controls available", SettingId::ALL.len()),
            ),
        });

        checks.push(if facts.capture_present {
            HealthCheck::new(
                "touchpad.capture",
                HealthState::Ok,
                "the settings from before the first change are recorded",
            )
        } else {
            HealthCheck::new(
                "touchpad.capture",
                HealthState::Ok,
                "nothing has been changed yet, so there is nothing to restore",
            )
        });

        if facts.safe_mode {
            checks.push(HealthCheck::new(
                "touchpad.safe_mode",
                HealthState::Degraded,
                "safe mode is on: Better Touchpad reads settings but changes none",
            ));
        } else if !facts.integration_enabled {
            checks.push(HealthCheck::new(
                "touchpad.integration",
                HealthState::Degraded,
                "Better Touchpad integration is switched off",
            ));
        }

        Self { checks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Support;

    fn facts<'a>(capabilities: &'a Capabilities) -> HealthFacts<'a> {
        HealthFacts {
            configuration_readable: true,
            configuration_detail: "read".to_string(),
            backend_name: "gnome",
            backend_reachable: true,
            backend_detail: "the dconf service answered".to_string(),
            devices_found: 1,
            selected_device: Some("usb:06cb:ce67"),
            capabilities,
            capture_present: false,
            safe_mode: false,
            integration_enabled: true,
        }
    }

    #[test]
    fn a_working_setup_reports_ok() {
        let capabilities = Capabilities::everything_immediate();
        let report = HealthReport::evaluate(&facts(&capabilities));
        assert_eq!(report.state(), HealthState::Ok);
        assert_eq!(report.checks.len(), 5);
    }

    #[test]
    fn a_backend_that_cannot_be_reached_fails_the_whole_report() {
        let capabilities = Capabilities::everything_immediate();
        let mut facts = facts(&capabilities);
        facts.backend_reachable = false;
        facts.backend_detail = "the session bus is not reachable".to_string();
        let report = HealthReport::evaluate(&facts);
        assert_eq!(report.state(), HealthState::Failed);
        assert_eq!(
            report.check("touchpad.backend").unwrap().state,
            HealthState::Failed
        );
    }

    #[test]
    fn no_touchpad_at_all_fails_rather_than_degrades() {
        let capabilities = Capabilities::everything_immediate();
        let mut facts = facts(&capabilities);
        facts.devices_found = 0;
        facts.selected_device = None;
        assert_eq!(
            HealthReport::evaluate(&facts)
                .check("touchpad.device")
                .unwrap()
                .state,
            HealthState::Failed
        );
    }

    #[test]
    fn a_backend_that_owns_only_some_controls_is_degraded_not_failed() {
        let capabilities = Capabilities::everything_immediate().with(
            SettingId::SmoothScrolling,
            Support::unavailable("gnome.no_key", "no such key"),
        );
        let report = HealthReport::evaluate(&facts(&capabilities));
        assert_eq!(report.state(), HealthState::Degraded);
        assert!(
            report
                .check("touchpad.capabilities")
                .unwrap()
                .detail
                .starts_with("12 of 13")
        );
    }

    #[test]
    fn a_backend_that_owns_nothing_fails() {
        let capabilities = Capabilities::new();
        assert_eq!(
            HealthReport::evaluate(&facts(&capabilities)).state(),
            HealthState::Failed
        );
    }

    #[test]
    fn safe_mode_shows_as_its_own_degraded_check() {
        let capabilities = Capabilities::everything_immediate();
        let mut facts = facts(&capabilities);
        facts.safe_mode = true;
        let report = HealthReport::evaluate(&facts);
        assert_eq!(report.state(), HealthState::Degraded);
        assert!(report.check("touchpad.safe_mode").is_some());
        assert!(report.check("touchpad.integration").is_none());
    }
}
