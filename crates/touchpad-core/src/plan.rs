//! What a change would do, and what it actually did.
//!
//! A plan is built before anything is touched and carries, per setting, the
//! value that was read first. That captured reading is what a restore returns
//! to; nothing in this crate can produce a "factory" value, because there is no
//! such thing to produce — only what the system said before Better OS wrote.
//!
//! An outcome is never inferred from a successful write. `Applied` is only
//! reachable by reading the setting back and seeing the requested value, which
//! is why every result carries the reading it was decided from.

use serde::{Deserialize, Serialize};

use crate::backup::Backup;
use crate::settings::{Reading, Section, SessionEffect, SettingId, SettingValue, Support};

/// One setting a plan intends to write.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplyStep {
    pub setting: SettingId,
    pub requested: SettingValue,
    /// What the backend said before this plan was built. A restore of this
    /// step goes back to exactly this.
    pub captured: Reading,
    pub effect: SessionEffect,
}

/// Why a setting is in the plan but will not be written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "skipped", rename_all = "snake_case")]
pub enum SkipReason {
    /// The backend does not own this setting at all.
    Unavailable { reason: String, detail: String },
    /// The setting already holds the requested value.
    AlreadyEffective,
    /// Better Touchpad is disabled, or safe mode is on.
    IntegrationDisabled { reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkippedStep {
    pub setting: SettingId,
    pub reason: SkipReason,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApplyPlan {
    pub steps: Vec<ApplyStep>,
    pub skipped: Vec<SkippedStep>,
}

impl ApplyPlan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn touches(&self, setting: SettingId) -> bool {
        self.steps.iter().any(|step| step.setting == setting)
    }

    pub fn sections(&self) -> Vec<Section> {
        let mut sections: Vec<Section> = self
            .steps
            .iter()
            .map(|step| step.setting.section())
            .collect();
        sections.sort_unstable();
        sections.dedup();
        sections
    }

    /// The captured readings this plan would need to undo itself. This is what
    /// gets written to the backup before the first mutation.
    pub fn capture(&self) -> Vec<(SettingId, Reading)> {
        self.steps
            .iter()
            .map(|step| (step.setting, step.captured.clone()))
            .collect()
    }
}

/// What a restore would write for one setting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "restore", rename_all = "snake_case")]
pub enum RestoreStep {
    /// Put a captured value back.
    Write {
        setting: SettingId,
        value: SettingValue,
    },
    /// The capture says nothing was set, so restoring means removing the entry
    /// again rather than writing a value nobody chose.
    Reset { setting: SettingId },
    /// The capture was never definite, or the backend cannot write it now.
    Impossible {
        setting: SettingId,
        reason: String,
        detail: String,
    },
}

impl RestoreStep {
    pub fn setting(&self) -> SettingId {
        match self {
            Self::Write { setting, .. } | Self::Reset { setting } => *setting,
            Self::Impossible { setting, .. } => *setting,
        }
    }

    pub fn is_actionable(&self) -> bool {
        !matches!(self, Self::Impossible { .. })
    }
}

/// How much of the configuration a restore covers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum RestoreScope {
    All,
    Section { section: Section },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RestorePlan {
    pub scope: RestoreScope,
    pub steps: Vec<RestoreStep>,
}

impl RestorePlan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn actionable(&self) -> impl Iterator<Item = &RestoreStep> {
        self.steps.iter().filter(|step| step.is_actionable())
    }

    /// Builds the plan that puts a backup back, for one section or all of it.
    ///
    /// A setting the backend cannot write now becomes an `Impossible` step
    /// rather than disappearing, so the review screen shows the whole captured
    /// state including the part that cannot be undone automatically.
    pub fn from_backup(
        backup: &Backup,
        scope: RestoreScope,
        support: impl Fn(SettingId) -> Support,
    ) -> Self {
        let wanted: Vec<SettingId> = match scope {
            RestoreScope::All => SettingId::ALL.to_vec(),
            RestoreScope::Section { section } => SettingId::in_section(section),
        };
        let steps = wanted
            .into_iter()
            .filter_map(|setting| {
                let captured = backup.reading(setting)?;
                Some(match support(setting) {
                    Support::Unavailable { reason, detail } => RestoreStep::Impossible {
                        setting,
                        reason,
                        detail,
                    },
                    Support::Full { .. } => match captured {
                        Reading::Value { value } => RestoreStep::Write {
                            setting,
                            value: *value,
                        },
                        Reading::SessionDefault { .. } => RestoreStep::Reset { setting },
                        other => RestoreStep::Impossible {
                            setting,
                            reason: "touchpad.captured_value_is_indeterminate".to_string(),
                            detail: other
                                .reason()
                                .unwrap_or("nothing definite was captured")
                                .to_string(),
                        },
                    },
                })
            })
            .collect();
        Self { scope, steps }
    }
}

/// What happened to one setting, decided by reading it back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum StepOutcome {
    /// The read-back agrees with what was asked for.
    Applied { effective: Reading },
    /// The write succeeded and the backend says it needs a sign-out, so the
    /// read-back cannot be expected to agree yet.
    AwaitingSignOut { requested: SettingValue },
    /// The write succeeded but the read-back shows something else — the
    /// backend accepted a different value than the one that was asked for.
    PartiallySupported {
        requested: SettingValue,
        effective: Reading,
    },
    /// The backend refused, or the read-back could not be made.
    Failed { reason: String, detail: String },
    /// Nothing was attempted because the backend does not own this setting.
    Unsupported { reason: String, detail: String },
}

impl StepOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Applied { .. } | Self::AwaitingSignOut { .. })
    }
}

/// The single word for a whole apply or restore.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// Nothing needed doing.
    NothingToDo,
    Applied,
    AwaitingSignOut,
    PartiallySupported,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub results: Vec<(SettingId, StepOutcome)>,
    pub skipped: Vec<SkippedStep>,
}

impl RunReport {
    pub fn outcome(&self, setting: SettingId) -> Option<&StepOutcome> {
        self.results
            .iter()
            .find(|(id, _)| *id == setting)
            .map(|(_, outcome)| outcome)
    }

    /// The worst thing that happened, because that is what the user has to be
    /// told. A run where one setting failed is a failed run, not a mostly
    /// applied one.
    pub fn state(&self) -> RunState {
        if self.results.is_empty() {
            return RunState::NothingToDo;
        }
        if self
            .results
            .iter()
            .any(|(_, outcome)| matches!(outcome, StepOutcome::Failed { .. }))
        {
            return RunState::Failed;
        }
        if self.results.iter().any(|(_, outcome)| {
            matches!(
                outcome,
                StepOutcome::PartiallySupported { .. } | StepOutcome::Unsupported { .. }
            )
        }) {
            return RunState::PartiallySupported;
        }
        if self
            .results
            .iter()
            .any(|(_, outcome)| matches!(outcome, StepOutcome::AwaitingSignOut { .. }))
        {
            return RunState::AwaitingSignOut;
        }
        RunState::Applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::Backup;
    use crate::value::Sensitivity;

    fn value(number: f64) -> SettingValue {
        SettingValue::sensitivity(Sensitivity::new(number).unwrap())
    }

    fn report(outcomes: Vec<StepOutcome>) -> RunReport {
        RunReport {
            results: SettingId::ALL.into_iter().zip(outcomes).collect(),
            skipped: Vec::new(),
        }
    }

    #[test]
    fn an_empty_run_is_nothing_to_do_rather_than_applied() {
        assert_eq!(RunReport::default().state(), RunState::NothingToDo);
    }

    #[test]
    fn every_run_state_is_reachable_and_the_worst_one_wins() {
        assert_eq!(
            report(vec![StepOutcome::Applied {
                effective: Reading::value(value(0.5))
            }])
            .state(),
            RunState::Applied
        );
        assert_eq!(
            report(vec![
                StepOutcome::Applied {
                    effective: Reading::value(value(0.5))
                },
                StepOutcome::AwaitingSignOut {
                    requested: value(0.5)
                },
            ])
            .state(),
            RunState::AwaitingSignOut
        );
        assert_eq!(
            report(vec![
                StepOutcome::AwaitingSignOut {
                    requested: value(0.5)
                },
                StepOutcome::PartiallySupported {
                    requested: value(0.9),
                    effective: Reading::value(value(0.5))
                },
            ])
            .state(),
            RunState::PartiallySupported
        );
        assert_eq!(
            report(vec![
                StepOutcome::Applied {
                    effective: Reading::value(value(0.5))
                },
                StepOutcome::PartiallySupported {
                    requested: value(0.9),
                    effective: Reading::value(value(0.5))
                },
                StepOutcome::Failed {
                    reason: "x".into(),
                    detail: "y".into()
                },
            ])
            .state(),
            RunState::Failed
        );
    }

    #[test]
    fn an_unsupported_result_reads_as_partially_supported_for_the_run() {
        assert_eq!(
            report(vec![
                StepOutcome::Applied {
                    effective: Reading::value(value(0.5))
                },
                StepOutcome::Unsupported {
                    reason: "x".into(),
                    detail: "y".into()
                },
            ])
            .state(),
            RunState::PartiallySupported
        );
    }

    #[test]
    fn restoring_a_setting_that_held_nothing_resets_it_rather_than_writing_a_guess() {
        let backup = Backup::capture(
            "gnome",
            None,
            vec![
                (
                    SettingId::PointerSensitivity,
                    Reading::session_default("gnome.no_user_scope_value"),
                ),
                (
                    SettingId::TapToClick,
                    Reading::value(SettingValue::toggle(true)),
                ),
                (
                    SettingId::NaturalScrolling,
                    Reading::unknown("gnome.database_unreadable"),
                ),
            ],
            0,
        );
        let plan = RestorePlan::from_backup(&backup, RestoreScope::All, |_| Support::immediate());

        assert_eq!(
            plan.steps,
            vec![
                RestoreStep::Reset {
                    setting: SettingId::PointerSensitivity
                },
                RestoreStep::Impossible {
                    setting: SettingId::NaturalScrolling,
                    reason: "touchpad.captured_value_is_indeterminate".to_string(),
                    detail: "gnome.database_unreadable".to_string(),
                },
                RestoreStep::Write {
                    setting: SettingId::TapToClick,
                    value: SettingValue::toggle(true)
                },
            ]
        );
        assert_eq!(plan.actionable().count(), 2);
    }

    #[test]
    fn a_section_restore_covers_only_that_section() {
        let backup = Backup::capture(
            "gnome",
            None,
            SettingId::ALL
                .into_iter()
                .map(|setting| (setting, Reading::session_default("unset")))
                .collect(),
            0,
        );
        let plan = RestorePlan::from_backup(
            &backup,
            RestoreScope::Section {
                section: Section::Clicking,
            },
            |_| Support::immediate(),
        );
        assert_eq!(
            plan.steps.len(),
            SettingId::in_section(Section::Clicking).len()
        );
        assert!(
            plan.steps
                .iter()
                .all(|step| step.setting().section() == Section::Clicking)
        );
    }

    #[test]
    fn a_setting_the_backend_can_no_longer_write_is_shown_rather_than_dropped() {
        let backup = Backup::capture(
            "gnome",
            None,
            vec![(
                SettingId::TapToClick,
                Reading::value(SettingValue::toggle(true)),
            )],
            0,
        );
        let plan = RestorePlan::from_backup(&backup, RestoreScope::All, |_| {
            Support::unavailable("gnome.key_absent", "this GNOME has no such key")
        });
        assert_eq!(
            plan.steps,
            vec![RestoreStep::Impossible {
                setting: SettingId::TapToClick,
                reason: "gnome.key_absent".to_string(),
                detail: "this GNOME has no such key".to_string(),
            }]
        );
        assert_eq!(plan.actionable().count(), 0);
    }
}
