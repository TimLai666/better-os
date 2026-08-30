//! The four states Issue #3 requires, held together.
//!
//! For every setting there are four different things a user can be told, and
//! they are genuinely different:
//!
//! - **current** — what the saved configuration says.
//! - **pending** — what has been changed on screen but not applied.
//! - **effective** — what the backend last reported the system actually does.
//! - **previous** — what the backend reported before Better OS first wrote.
//!
//! Collapsing any two of them produces a specific lie. Without pending, a
//! slider that has moved looks applied. Without effective, a requested value
//! that the backend rounded looks honored. Without previous, restore has
//! nothing to go back to.

use std::collections::BTreeMap;

use crate::backup::Backup;
use crate::config::TouchpadConfig;
use crate::plan::{
    ApplyPlan, ApplyStep, RestorePlan, RestoreScope, RunReport, SkipReason, SkippedStep,
    StepOutcome,
};
use crate::settings::{
    Capabilities, Reading, Section, SessionEffect, SettingId, SettingValue, Support,
};
use crate::value::ValueError;

/// Everything there is to say about one setting.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingState {
    pub setting: SettingId,
    pub support: Support,
    pub current: SettingValue,
    pub pending: Option<SettingValue>,
    pub effective: Reading,
    pub previous: Option<Reading>,
}

impl SettingState {
    /// The value an apply would ask for.
    pub fn requested(&self) -> SettingValue {
        self.pending.unwrap_or(self.current)
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some_and(|value| value != self.current)
    }

    /// Whether the system already does what is being asked for. Only a value
    /// reading can agree; "nothing is set" never agrees with a request,
    /// because writing the request is exactly what would change it.
    pub fn matches_effective(&self) -> bool {
        self.effective.as_value() == Some(self.requested())
    }

    /// Whether what the system does differs from what Better Touchpad last
    /// asked for. This is how an external change shows up.
    pub fn drifted(&self) -> bool {
        match self.effective.as_value() {
            Some(value) => value != self.current,
            None => false,
        }
    }
}

/// Why nothing may be written right now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Inhibited {
    /// Better Touchpad integration is switched off in the configuration.
    Disabled,
    /// The safe-mode entry point has been used.
    SafeMode,
}

impl Inhibited {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Disabled => "touchpad.integration_disabled",
            Self::SafeMode => "touchpad.safe_mode",
        }
    }
}

/// The whole live picture: configuration, what the backend can do, what it
/// last said, what is staged, and what was captured first.
#[derive(Clone, Debug)]
pub struct TouchpadState {
    config: TouchpadConfig,
    capabilities: Capabilities,
    readings: BTreeMap<SettingId, Reading>,
    pending: BTreeMap<SettingId, SettingValue>,
    backup: Option<Backup>,
    inhibited: Option<Inhibited>,
}

impl TouchpadState {
    pub fn new(config: TouchpadConfig, capabilities: Capabilities) -> Self {
        let inhibited = (!config.enabled).then_some(Inhibited::Disabled);
        Self {
            config,
            capabilities,
            readings: BTreeMap::new(),
            pending: BTreeMap::new(),
            backup: None,
            inhibited,
        }
    }

    pub fn config(&self) -> &TouchpadConfig {
        &self.config
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub fn backup(&self) -> Option<&Backup> {
        self.backup.as_ref()
    }

    pub fn inhibited(&self) -> Option<&Inhibited> {
        self.inhibited.as_ref()
    }

    pub fn set_safe_mode(&mut self, on: bool) {
        if on {
            self.inhibited = Some(Inhibited::SafeMode);
        } else if self.config.enabled {
            self.inhibited = None;
        } else {
            self.inhibited = Some(Inhibited::Disabled);
        }
    }

    pub fn set_capabilities(&mut self, capabilities: Capabilities) {
        self.capabilities = capabilities;
    }

    pub fn adopt_backup(&mut self, backup: Backup) {
        self.backup = Some(backup);
    }

    /// Records what the backend just read.
    pub fn adopt_readings(&mut self, readings: Vec<(SettingId, Reading)>) {
        for (setting, reading) in readings {
            self.readings.insert(setting, reading);
        }
    }

    pub fn effective(&self, setting: SettingId) -> Reading {
        self.readings
            .get(&setting)
            .cloned()
            .unwrap_or_else(|| Reading::unknown("touchpad.not_read_yet"))
    }

    /// Stages a change without writing anything.
    ///
    /// The linked-axis rule is the configuration's, so this runs the change
    /// through a scratch copy of the configuration and stages whatever moved.
    /// Staging a vertical factor while the axes are linked therefore stages
    /// both, exactly as saving it would.
    pub fn stage(&mut self, setting: SettingId, value: SettingValue) -> Result<(), ValueError> {
        let mut scratch = self.config.clone();
        for (staged, staged_value) in &self.pending {
            // Already-staged values are re-applied raw so the link rule sees
            // the same starting point the user is looking at.
            scratch.set(*staged, *staged_value)?;
        }
        scratch.set(setting, value)?;

        for candidate in SettingId::ALL {
            let now = scratch.value(candidate);
            if now == self.config.value(candidate) {
                self.pending.remove(&candidate);
            } else {
                self.pending.insert(candidate, now);
            }
        }
        Ok(())
    }

    /// Stages the linked-axis switch itself.
    pub fn stage_linked_axes(&mut self, linked: bool) {
        let mut scratch = self.config.clone();
        for (staged, staged_value) in &self.pending {
            let _ = scratch.set(*staged, *staged_value);
        }
        scratch.set_linked_axes(linked);
        self.config.scrolling.linked_axes = linked;
        for candidate in SettingId::ALL {
            let now = scratch.value(candidate);
            if now == self.config.value(candidate) {
                self.pending.remove(&candidate);
            } else {
                self.pending.insert(candidate, now);
            }
        }
    }

    pub fn discard_pending(&mut self) {
        self.pending.clear();
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn state(&self, setting: SettingId) -> SettingState {
        SettingState {
            setting,
            support: self.capabilities.support(setting),
            current: self.config.value(setting),
            pending: self.pending.get(&setting).copied(),
            effective: self.effective(setting),
            previous: self
                .backup
                .as_ref()
                .and_then(|backup| backup.reading(setting))
                .cloned(),
        }
    }

    pub fn states(&self) -> Vec<SettingState> {
        SettingId::ALL
            .into_iter()
            .map(|setting| self.state(setting))
            .collect()
    }

    pub fn section_states(&self, section: Section) -> Vec<SettingState> {
        SettingId::in_section(section)
            .into_iter()
            .map(|setting| self.state(setting))
            .collect()
    }

    /// The plan for what is staged, and nothing else.
    ///
    /// Applying only ever writes settings the user actually moved. A plan built
    /// from the whole configuration would write twelve values a user never
    /// asked about the first time they nudged one slider, and would overwrite
    /// whatever they had set elsewhere in the process.
    ///
    /// A setting the backend does not own never becomes a step. It becomes a
    /// skipped entry naming why, which is what the review screen shows.
    pub fn apply_plan(&self) -> ApplyPlan {
        self.plan_for(self.pending.keys().copied().collect())
    }

    /// The plan that makes the whole saved configuration effective. Nothing in
    /// the shipped GUI calls this; it exists for a caller that wants to reapply
    /// a profile, and it is the same rules over a wider set.
    pub fn enforce_plan(&self) -> ApplyPlan {
        self.plan_for(SettingId::ALL.to_vec())
    }

    fn plan_for(&self, considered: Vec<SettingId>) -> ApplyPlan {
        let mut plan = ApplyPlan::default();
        for setting in considered {
            let state = self.state(setting);
            if let Some(inhibited) = &self.inhibited {
                plan.skipped.push(SkippedStep {
                    setting,
                    reason: SkipReason::IntegrationDisabled {
                        reason: inhibited.reason().to_string(),
                    },
                });
                continue;
            }
            match state.support {
                Support::Unavailable { reason, detail } => plan.skipped.push(SkippedStep {
                    setting,
                    reason: SkipReason::Unavailable { reason, detail },
                }),
                Support::Full { effect } => {
                    if state.matches_effective() {
                        plan.skipped.push(SkippedStep {
                            setting,
                            reason: SkipReason::AlreadyEffective,
                        });
                    } else {
                        plan.steps.push(ApplyStep {
                            setting,
                            requested: state.requested(),
                            captured: state.effective.clone(),
                            effect,
                        });
                    }
                }
            }
        }
        plan
    }

    /// The plan that puts the captured state back.
    pub fn restore_plan(&self, scope: RestoreScope) -> Option<RestorePlan> {
        let backup = self.backup.as_ref()?;
        Some(RestorePlan::from_backup(backup, scope, |setting| {
            self.capabilities.support(setting)
        }))
    }

    /// Captures the pre-change readings a plan needs, without ever replacing a
    /// reading an earlier capture already holds.
    pub fn capture_before(&mut self, plan: &ApplyPlan, backend: &str, at: u64) -> Vec<SettingId> {
        match &mut self.backup {
            Some(backup) => backup.extend_untouched(plan.capture()),
            None => {
                let captured = plan.capture();
                let settings = captured.iter().map(|(setting, _)| *setting).collect();
                self.backup = Some(Backup::capture(
                    backend,
                    self.device_identity(),
                    captured,
                    at,
                ));
                settings
            }
        }
    }

    fn device_identity(&self) -> Option<String> {
        match &self.config.selected_device {
            crate::config::DeviceSelection::Auto => None,
            crate::config::DeviceSelection::Device { identity } => Some(identity.clone()),
        }
    }

    /// Folds a finished run back in: successful settings become the saved
    /// configuration, and every read-back becomes the new effective value.
    pub fn record(&mut self, report: &RunReport) {
        for (setting, outcome) in &report.results {
            match outcome {
                StepOutcome::Applied { effective } => {
                    if let Some(value) = self.pending.remove(setting) {
                        let _ = self.config.set(*setting, value);
                    }
                    self.readings.insert(*setting, effective.clone());
                }
                StepOutcome::AwaitingSignOut { requested } => {
                    if let Some(value) = self.pending.remove(setting) {
                        let _ = self.config.set(*setting, value);
                    }
                    let _ = requested;
                }
                StepOutcome::PartiallySupported { effective, .. } => {
                    // The saved configuration keeps what the user asked for;
                    // the effective reading records what the system did. That
                    // difference is the whole point of showing both.
                    if let Some(value) = self.pending.remove(setting) {
                        let _ = self.config.set(*setting, value);
                    }
                    self.readings.insert(*setting, effective.clone());
                }
                StepOutcome::Failed { .. } | StepOutcome::Unsupported { .. } => {}
            }
        }
    }

    /// Folds a finished restore back in, so the saved configuration stops
    /// asking for the value that was just undone.
    pub fn record_restore(&mut self, report: &RunReport) {
        for (setting, outcome) in &report.results {
            match outcome {
                StepOutcome::Applied { effective } => {
                    if let Some(value) = effective.as_value() {
                        let _ = self.config.set(*setting, value);
                    }
                    self.pending.remove(setting);
                    self.readings.insert(*setting, effective.clone());
                }
                StepOutcome::PartiallySupported { effective, .. } => {
                    self.readings.insert(*setting, effective.clone());
                }
                _ => {}
            }
        }
    }

    /// Whether anything the plan will write only takes effect after a sign-out.
    pub fn plan_needs_sign_out(plan: &ApplyPlan) -> bool {
        plan.steps
            .iter()
            .any(|step| step.effect == SessionEffect::SignOutRequired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Support;
    use crate::value::{ScrollFactor, Sensitivity};

    fn state() -> TouchpadState {
        TouchpadState::new(
            TouchpadConfig::default(),
            Capabilities::everything_immediate(),
        )
    }

    fn sensitivity(value: f64) -> SettingValue {
        SettingValue::sensitivity(Sensitivity::new(value).unwrap())
    }

    #[test]
    fn a_setting_that_was_never_read_is_unknown_rather_than_its_configured_value() {
        let state = state();
        let entry = state.state(SettingId::PointerSensitivity);
        assert_eq!(entry.current, sensitivity(0.5));
        assert_eq!(entry.effective, Reading::unknown("touchpad.not_read_yet"));
        assert_eq!(entry.pending, None);
        assert_eq!(entry.previous, None);
    }

    #[test]
    fn staging_a_change_leaves_the_saved_value_alone_until_it_is_recorded() {
        let mut state = state();
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();

        let entry = state.state(SettingId::PointerSensitivity);
        assert_eq!(entry.current, sensitivity(0.5));
        assert_eq!(entry.pending, Some(sensitivity(0.9)));
        assert_eq!(entry.requested(), sensitivity(0.9));
        assert!(entry.is_pending());
        assert_eq!(state.config().pointer.sensitivity.get(), 0.5);
    }

    #[test]
    fn staging_a_linked_scroll_factor_stages_both_axes() {
        let mut state = state();
        state
            .stage(
                SettingId::VerticalScrollFactor,
                SettingValue::factor(ScrollFactor::new(2.0).unwrap()),
            )
            .unwrap();
        assert_eq!(
            state.state(SettingId::HorizontalScrollFactor).pending,
            Some(SettingValue::factor(ScrollFactor::new(2.0).unwrap()))
        );
    }

    #[test]
    fn unlinking_the_axes_stops_them_moving_together() {
        let mut state = state();
        state.stage_linked_axes(false);
        state
            .stage(
                SettingId::VerticalScrollFactor,
                SettingValue::factor(ScrollFactor::new(2.0).unwrap()),
            )
            .unwrap();
        assert_eq!(state.state(SettingId::HorizontalScrollFactor).pending, None);
    }

    #[test]
    fn staging_back_to_the_saved_value_clears_the_pending_state() {
        let mut state = state();
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.5))
            .unwrap();
        assert!(!state.has_pending());
    }

    #[test]
    fn an_impossible_staged_value_is_refused_and_stages_nothing() {
        let mut state = state();
        assert!(
            state
                .stage(SettingId::PointerSensitivity, SettingValue::toggle(true))
                .is_err()
        );
        assert!(!state.has_pending());
    }

    #[test]
    fn a_plan_skips_what_the_backend_does_not_own_instead_of_writing_it() {
        let capabilities = Capabilities::everything_immediate().with(
            SettingId::HorizontalScrollFactor,
            Support::unavailable("gnome.no_key", "GNOME has no horizontal scroll factor"),
        );
        let mut state = TouchpadState::new(TouchpadConfig::default(), capabilities);
        state.stage_linked_axes(false);
        state
            .stage(
                SettingId::HorizontalScrollFactor,
                SettingValue::factor(ScrollFactor::new(2.0).unwrap()),
            )
            .unwrap();

        let plan = state.apply_plan();
        assert!(plan.steps.is_empty());
        assert_eq!(
            plan.skipped,
            vec![SkippedStep {
                setting: SettingId::HorizontalScrollFactor,
                reason: SkipReason::Unavailable {
                    reason: "gnome.no_key".to_string(),
                    detail: "GNOME has no horizontal scroll factor".to_string(),
                }
            }]
        );
    }

    #[test]
    fn a_setting_that_already_holds_the_requested_value_is_skipped() {
        let mut state = state();
        state.adopt_readings(vec![(
            SettingId::PointerSensitivity,
            Reading::value(sensitivity(0.9)),
        )]);
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();

        let plan = state.apply_plan();
        assert!(plan.steps.is_empty());
        assert_eq!(plan.skipped[0].reason, SkipReason::AlreadyEffective);
    }

    #[test]
    fn a_disabled_integration_plans_no_write_at_all() {
        let mut state = state();
        state.set_safe_mode(true);
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();

        let plan = state.apply_plan();
        assert!(plan.steps.is_empty());
        assert_eq!(
            plan.skipped[0].reason,
            SkipReason::IntegrationDisabled {
                reason: "touchpad.safe_mode".to_string()
            }
        );
    }

    #[test]
    fn the_capture_taken_before_the_first_write_survives_a_second_one() {
        let mut state = state();
        state.adopt_readings(vec![(
            SettingId::PointerSensitivity,
            Reading::value(sensitivity(0.2)),
        )]);
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();
        let plan = state.apply_plan();
        state.capture_before(&plan, "gnome", 100);

        // Better OS wrote 0.9; a later plan must not record that as previous.
        state.adopt_readings(vec![(
            SettingId::PointerSensitivity,
            Reading::value(sensitivity(0.9)),
        )]);
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.3))
            .unwrap();
        let second = state.apply_plan();
        state.capture_before(&second, "gnome", 200);

        assert_eq!(
            state
                .backup()
                .unwrap()
                .reading(SettingId::PointerSensitivity),
            Some(&Reading::value(sensitivity(0.2)))
        );
        assert_eq!(state.backup().unwrap().captured_at, 100);
    }

    #[test]
    fn recording_an_applied_run_saves_the_value_and_the_read_back() {
        let mut state = state();
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();
        let report = RunReport {
            results: vec![(
                SettingId::PointerSensitivity,
                StepOutcome::Applied {
                    effective: Reading::value(sensitivity(0.9)),
                },
            )],
            skipped: Vec::new(),
        };
        state.record(&report);

        assert!(!state.has_pending());
        assert_eq!(state.config().pointer.sensitivity.get(), 0.9);
        assert_eq!(
            state.state(SettingId::PointerSensitivity).effective,
            Reading::value(sensitivity(0.9))
        );
    }

    #[test]
    fn a_partially_supported_run_keeps_the_request_and_the_different_effective_value() {
        let mut state = state();
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();
        state.record(&RunReport {
            results: vec![(
                SettingId::PointerSensitivity,
                StepOutcome::PartiallySupported {
                    requested: sensitivity(0.9),
                    effective: Reading::value(sensitivity(0.8)),
                },
            )],
            skipped: Vec::new(),
        });

        let entry = state.state(SettingId::PointerSensitivity);
        assert_eq!(entry.current, sensitivity(0.9));
        assert_eq!(entry.effective, Reading::value(sensitivity(0.8)));
        assert!(!entry.matches_effective());
    }

    #[test]
    fn a_failed_run_changes_neither_the_configuration_nor_the_pending_change() {
        let mut state = state();
        state
            .stage(SettingId::PointerSensitivity, sensitivity(0.9))
            .unwrap();
        state.record(&RunReport {
            results: vec![(
                SettingId::PointerSensitivity,
                StepOutcome::Failed {
                    reason: "dconf.write_failed".to_string(),
                    detail: "no session bus".to_string(),
                },
            )],
            skipped: Vec::new(),
        });

        assert_eq!(state.config().pointer.sensitivity.get(), 0.5);
        assert!(state.has_pending());
    }

    #[test]
    fn a_value_changed_outside_better_touchpad_reads_as_drift() {
        let mut state = state();
        state.adopt_readings(vec![(
            SettingId::PointerSensitivity,
            Reading::value(sensitivity(0.1)),
        )]);
        assert!(state.state(SettingId::PointerSensitivity).drifted());
    }

    #[test]
    fn a_restore_plan_needs_a_capture_to_exist() {
        let state = state();
        assert!(state.restore_plan(RestoreScope::All).is_none());
    }
}
