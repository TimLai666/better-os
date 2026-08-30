//! A backend that changes nothing outside its own memory.
//!
//! This is how every apply-and-read-back path is proven for every control,
//! including the ones no shipped backend owns and the ones that need a
//! sign-out. It runs the production [`TouchpadBackend::apply`] and
//! [`TouchpadBackend::restore`] code — only the write and the read are
//! replaced — so a test that proves an outcome here proves the rule production
//! uses to reach it.

use std::collections::BTreeMap;

use touchpad_core::{Capabilities, Reading, SettingId, SettingValue, Support};

use crate::{TouchpadBackend, WriteOutcome};

/// How the fake backend should misbehave, so the unhappy paths are reachable.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum MockBehavior {
    #[default]
    Honest,
    /// Accepts the write and stores something else, which is what a backend
    /// that quantises a value does.
    StoresInstead(SettingValue),
    /// Refuses the write.
    RefusesWrite { reason: String, detail: String },
    /// Accepts the write and then cannot be read back.
    UnreadableAfterWrite,
    /// Accepts the write and stores nothing, which is a backend that silently
    /// dropped it.
    DiscardsWrite,
}

pub struct MockBackend {
    capabilities: Capabilities,
    values: BTreeMap<SettingId, Reading>,
    behavior: BTreeMap<SettingId, MockBehavior>,
    /// Every write this backend was asked to make, in order.
    pub writes: Vec<(SettingId, Option<SettingValue>)>,
}

impl MockBackend {
    /// A backend that owns every control and applies immediately.
    pub fn new() -> Self {
        Self {
            capabilities: Capabilities::everything_immediate(),
            values: BTreeMap::new(),
            behavior: BTreeMap::new(),
            writes: Vec::new(),
        }
    }

    pub fn with_capabilities(capabilities: Capabilities) -> Self {
        Self {
            capabilities,
            values: BTreeMap::new(),
            behavior: BTreeMap::new(),
            writes: Vec::new(),
        }
    }

    pub fn holding(mut self, setting: SettingId, value: SettingValue) -> Self {
        self.values.insert(setting, Reading::value(value));
        self
    }

    pub fn behaving(mut self, setting: SettingId, behavior: MockBehavior) -> Self {
        self.behavior.insert(setting, behavior);
        self
    }

    pub fn set_support(&mut self, setting: SettingId, support: Support) {
        self.capabilities.insert(setting, support);
    }

    fn behavior(&self, setting: SettingId) -> MockBehavior {
        self.behavior
            .get(&setting)
            .cloned()
            .unwrap_or(MockBehavior::Honest)
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchpadBackend for MockBackend {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn read(&self, settings: &[SettingId]) -> Vec<(SettingId, Reading)> {
        settings
            .iter()
            .map(|setting| {
                let reading = self
                    .values
                    .get(setting)
                    .cloned()
                    .unwrap_or_else(|| Reading::session_default("mock.nothing_was_ever_set"));
                (*setting, reading)
            })
            .collect()
    }

    fn write_all(
        &mut self,
        writes: &[(SettingId, Option<SettingValue>)],
    ) -> Vec<(SettingId, WriteOutcome)> {
        writes
            .iter()
            .map(|(setting, value)| {
                self.writes.push((*setting, *value));
                if !self.capabilities.is_available(*setting) {
                    return (
                        *setting,
                        WriteOutcome::unsupported(
                            "mock.not_declared",
                            "this fake backend does not own that setting",
                        ),
                    );
                }
                match self.behavior(*setting) {
                    MockBehavior::Honest => {
                        match value {
                            Some(value) => self.values.insert(*setting, Reading::value(*value)),
                            None => self.values.remove(setting),
                        };
                        (*setting, WriteOutcome::Written)
                    }
                    MockBehavior::StoresInstead(other) => {
                        self.values.insert(*setting, Reading::value(other));
                        (*setting, WriteOutcome::Written)
                    }
                    MockBehavior::RefusesWrite { reason, detail } => {
                        (*setting, WriteOutcome::failed(reason, detail))
                    }
                    MockBehavior::UnreadableAfterWrite => {
                        self.values
                            .insert(*setting, Reading::unknown("mock.unreadable"));
                        (*setting, WriteOutcome::Written)
                    }
                    MockBehavior::DiscardsWrite => (*setting, WriteOutcome::Written),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use touchpad_core::{
        ApplyPlan, ApplyStep, RestorePlan, RestoreScope, RestoreStep, RunState, SessionEffect,
        SettingValue, StepOutcome,
    };
    use touchpad_core::{ScrollFactor, Sensitivity};

    fn sensitivity(value: f64) -> SettingValue {
        SettingValue::sensitivity(Sensitivity::new(value).unwrap())
    }

    fn plan(setting: SettingId, requested: SettingValue, effect: SessionEffect) -> ApplyPlan {
        ApplyPlan {
            steps: vec![ApplyStep {
                setting,
                requested,
                captured: Reading::session_default("mock.nothing_was_ever_set"),
                effect,
            }],
            skipped: Vec::new(),
        }
    }

    #[test]
    fn an_honest_write_reads_back_as_applied() {
        let mut backend = MockBackend::new();
        let report = backend.apply(&plan(
            SettingId::PointerSensitivity,
            sensitivity(0.8),
            SessionEffect::Immediate,
        ));
        assert_eq!(report.state(), RunState::Applied);
        assert_eq!(
            report.outcome(SettingId::PointerSensitivity),
            Some(&StepOutcome::Applied {
                effective: Reading::value(sensitivity(0.8))
            })
        );
    }

    #[test]
    fn every_control_can_be_applied_and_read_back() {
        for setting in SettingId::ALL {
            let requested = match setting.kind() {
                touchpad_core::ValueKind::Sensitivity => sensitivity(0.8),
                touchpad_core::ValueKind::Factor => {
                    SettingValue::factor(ScrollFactor::new(1.7).unwrap())
                }
                touchpad_core::ValueKind::Toggle => SettingValue::toggle(true),
                touchpad_core::ValueKind::Acceleration => {
                    SettingValue::acceleration(touchpad_core::AccelerationProfile::Flat)
                }
                touchpad_core::ValueKind::Click => {
                    SettingValue::click(touchpad_core::ClickMethod::Fingers)
                }
            };
            let mut backend = MockBackend::new();
            let report = backend.apply(&plan(setting, requested, SessionEffect::Immediate));
            assert_eq!(
                report.state(),
                RunState::Applied,
                "{setting} did not apply: {report:?}"
            );
        }
    }

    #[test]
    fn a_setting_that_needs_a_sign_out_is_reported_as_awaiting_one() {
        let mut backend = MockBackend::new();
        backend.set_support(SettingId::SmoothScrolling, Support::sign_out_required());
        let report = backend.apply(&plan(
            SettingId::SmoothScrolling,
            SettingValue::toggle(true),
            SessionEffect::SignOutRequired,
        ));
        assert_eq!(report.state(), RunState::AwaitingSignOut);
    }

    #[test]
    fn a_backend_that_stores_something_else_is_partially_supported() {
        let mut backend = MockBackend::new().behaving(
            SettingId::PointerSensitivity,
            MockBehavior::StoresInstead(sensitivity(0.75)),
        );
        let report = backend.apply(&plan(
            SettingId::PointerSensitivity,
            sensitivity(0.8),
            SessionEffect::Immediate,
        ));
        assert_eq!(report.state(), RunState::PartiallySupported);
        assert_eq!(
            report.outcome(SettingId::PointerSensitivity),
            Some(&StepOutcome::PartiallySupported {
                requested: sensitivity(0.8),
                effective: Reading::value(sensitivity(0.75)),
            })
        );
    }

    #[test]
    fn a_refused_write_is_reported_as_failed() {
        let mut backend = MockBackend::new().behaving(
            SettingId::TapToClick,
            MockBehavior::RefusesWrite {
                reason: "mock.refused".to_string(),
                detail: "no".to_string(),
            },
        );
        let report = backend.apply(&plan(
            SettingId::TapToClick,
            SettingValue::toggle(true),
            SessionEffect::Immediate,
        ));
        assert_eq!(report.state(), RunState::Failed);
    }

    #[test]
    fn a_write_that_cannot_be_read_back_is_failed_and_not_applied() {
        let mut backend =
            MockBackend::new().behaving(SettingId::TapToClick, MockBehavior::UnreadableAfterWrite);
        let report = backend.apply(&plan(
            SettingId::TapToClick,
            SettingValue::toggle(true),
            SessionEffect::Immediate,
        ));
        assert_eq!(
            report.outcome(SettingId::TapToClick),
            Some(&StepOutcome::Failed {
                reason: "touchpad.verification_indeterminate".to_string(),
                detail: "mock.unreadable".to_string(),
            })
        );
    }

    #[test]
    fn a_write_the_backend_silently_dropped_is_not_reported_as_applied() {
        let mut backend =
            MockBackend::new().behaving(SettingId::TapToClick, MockBehavior::DiscardsWrite);
        let report = backend.apply(&plan(
            SettingId::TapToClick,
            SettingValue::toggle(true),
            SessionEffect::Immediate,
        ));
        // Nothing was stored, so the read back is "nothing is set" — which is
        // indeterminate against a request, not agreement.
        assert!(matches!(
            report.outcome(SettingId::TapToClick),
            Some(StepOutcome::Failed { .. })
        ));
    }

    #[test]
    fn a_setting_the_backend_does_not_own_is_unsupported_rather_than_written() {
        let mut backend = MockBackend::new();
        backend.set_support(
            SettingId::HorizontalScrollFactor,
            Support::unavailable("mock.no_key", "not here"),
        );
        let report = backend.apply(&plan(
            SettingId::HorizontalScrollFactor,
            SettingValue::factor(ScrollFactor::new(2.0).unwrap()),
            SessionEffect::Immediate,
        ));
        assert_eq!(report.state(), RunState::PartiallySupported);
        assert!(matches!(
            report.outcome(SettingId::HorizontalScrollFactor),
            Some(StepOutcome::Unsupported { .. })
        ));
    }

    #[test]
    fn restoring_a_captured_value_puts_it_back_and_verifies_it() {
        let mut backend =
            MockBackend::new().holding(SettingId::TapToClick, SettingValue::toggle(false));
        let plan = RestorePlan {
            scope: RestoreScope::All,
            steps: vec![RestoreStep::Write {
                setting: SettingId::TapToClick,
                value: SettingValue::toggle(true),
            }],
        };
        let report = backend.restore(&plan);
        assert_eq!(report.state(), RunState::Applied);
        assert_eq!(
            backend.read_one(SettingId::TapToClick),
            Reading::value(SettingValue::toggle(true))
        );
    }

    #[test]
    fn restoring_a_setting_that_held_nothing_removes_it_again() {
        let mut backend =
            MockBackend::new().holding(SettingId::TapToClick, SettingValue::toggle(true));
        let plan = RestorePlan {
            scope: RestoreScope::All,
            steps: vec![RestoreStep::Reset {
                setting: SettingId::TapToClick,
            }],
        };
        let report = backend.restore(&plan);

        assert_eq!(report.state(), RunState::Applied);
        assert_eq!(backend.writes, vec![(SettingId::TapToClick, None)]);
        assert!(matches!(
            backend.read_one(SettingId::TapToClick),
            Reading::SessionDefault { .. }
        ));
    }

    #[test]
    fn a_reset_that_leaves_a_value_behind_is_not_reported_as_restored() {
        let mut backend = MockBackend::new().behaving(
            SettingId::TapToClick,
            MockBehavior::StoresInstead(SettingValue::toggle(true)),
        );
        let report = backend.restore(&RestorePlan {
            scope: RestoreScope::All,
            steps: vec![RestoreStep::Reset {
                setting: SettingId::TapToClick,
            }],
        });
        assert_eq!(report.state(), RunState::PartiallySupported);
    }

    #[test]
    fn a_restore_step_that_cannot_be_written_is_reported_and_not_attempted() {
        let mut backend = MockBackend::new();
        let report = backend.restore(&RestorePlan {
            scope: RestoreScope::All,
            steps: vec![RestoreStep::Impossible {
                setting: SettingId::TapToClick,
                reason: "touchpad.captured_value_is_indeterminate".to_string(),
                detail: "nothing definite was captured".to_string(),
            }],
        });
        assert!(backend.writes.is_empty());
        assert!(matches!(
            report.outcome(SettingId::TapToClick),
            Some(StepOutcome::Unsupported { .. })
        ));
    }

    #[test]
    fn an_empty_plan_writes_nothing_and_reports_nothing_to_do() {
        let mut backend = MockBackend::new();
        let report = backend.apply(&ApplyPlan::default());
        assert!(backend.writes.is_empty());
        assert_eq!(report.state(), RunState::NothingToDo);
    }
}
