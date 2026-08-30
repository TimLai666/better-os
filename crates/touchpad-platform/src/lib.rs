//! The typed boundary between Better Touchpad's decisions and the desktop.
//!
//! Everything above this crate works in [`touchpad_core`] identities and typed
//! values. This is the only layer that knows a dconf key exists, and it still
//! builds no shell string: there is no `gsettings`, no `xinput`, and no command
//! anywhere in it.
//!
//! Three rules hold throughout, and they are the same ones `defaults-platform`
//! follows:
//!
//! - **A backend reports what it saw**, including that it saw nothing usable.
//!   [`touchpad_core::Reading`] keeps a value, an unset key, an unsupported
//!   setting, an undeterminable one, and a refused one apart, because only some
//!   of those can be safely overwritten and only some can be restored to.
//! - **A backend that cannot change something never offers it.** Capability is
//!   read, apply, *and* verify. A setting missing any of the three is
//!   [`touchpad_core::Support::Unavailable`] with the reason attached, and the
//!   GUI renders that reason instead of a switch that does nothing.
//! - **Verification is a second read, never an assumption.** Nothing in this
//!   crate reports `Applied` because a write returned success.

pub mod devices;
pub mod gnome;
pub mod gvariant;
pub mod mock;
pub mod roots;
pub mod session;

#[cfg(feature = "dconf-write")]
pub mod dconf;

use thiserror::Error;
use touchpad_core::{
    ApplyPlan, Capabilities, Reading, RestorePlan, RestoreStep, RunReport, SessionEffect,
    SettingId, SettingValue, StepOutcome, Support,
};

pub use devices::{DeviceCapabilities, DeviceInventory, DeviceState, TouchpadDevice};
pub use gnome::{GnomeBackend, TOUCHPAD_PREFIX};
pub use gvariant::{ChangeValue, Changeset, ChangesetError};
pub use mock::MockBackend;
pub use roots::Roots;
pub use session::{Session, SessionKind};

/// Two values that came from the same request are compared to this precision.
///
/// The Better OS sensitivity scale and GNOME's speed scale are related by a
/// linear map that is not exactly invertible in binary floating point, so a
/// value written and read straight back can differ in the last bit. Treating
/// that as "the backend applied something else" would report a partial success
/// for a write that was exact.
pub const VALUE_TOLERANCE: f64 = 1e-9;

#[derive(Clone, Debug, Error, PartialEq)]
pub enum PlatformError {
    #[error("no session bus is reachable: {0}")]
    NoSessionBus(String),
    #[error("the dconf service refused the call: {0}")]
    CallFailed(String),
    #[error("this build has no dconf write support compiled in")]
    NoWriteSupport,
    #[error(transparent)]
    Changeset(#[from] ChangesetError),
}

/// What a backend did when asked to write.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteOutcome {
    /// The backend wrote. Whether it took is decided by reading it back, not
    /// by this value.
    Written,
    Failed {
        reason: String,
        detail: String,
    },
    Unsupported {
        reason: String,
        detail: String,
    },
}

impl WriteOutcome {
    pub fn failed(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
            detail: detail.into(),
        }
    }

    pub fn unsupported(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
            detail: detail.into(),
        }
    }
}

/// Reads and changes touchpad settings.
///
/// Only two methods have to be written by an implementation. Applying and
/// restoring are provided here so that the write-then-read-back rule, and every
/// outcome it produces, is written once and shared by the production backend
/// and the test one.
pub trait TouchpadBackend {
    fn name(&self) -> &'static str;

    fn capabilities(&self) -> &Capabilities;

    /// The effective value of each requested setting.
    fn read(&self, settings: &[SettingId]) -> Vec<(SettingId, Reading)>;

    /// Writes values, or removes them when the value is `None`.
    ///
    /// Every write in one call is one change to the system where the backend
    /// can manage it, so a half-applied set is not something the user sees.
    fn write_all(
        &mut self,
        writes: &[(SettingId, Option<SettingValue>)],
    ) -> Vec<(SettingId, WriteOutcome)>;

    /// What this backend says about itself, for the Diagnostics screen.
    ///
    /// The default answer is derived from the capability table: a backend that
    /// owns nothing is not reachable, whatever it would like to claim.
    fn status(&self) -> BackendStatus {
        let available = self.capabilities().available().len();
        if available == 0 {
            return BackendStatus::unreachable(
                self.name(),
                "touchpad.backend_owns_nothing",
                "nothing can be read, applied, and verified in this session",
            );
        }
        BackendStatus::reachable(
            self.name(),
            format!(
                "{available} of {} controls can be read, applied, and verified",
                SettingId::ALL.len()
            ),
        )
    }

    fn read_all(&self) -> Vec<(SettingId, Reading)> {
        self.read(&SettingId::ALL)
    }

    fn read_one(&self, setting: SettingId) -> Reading {
        self.read(&[setting])
            .into_iter()
            .next()
            .map(|(_, reading)| reading)
            .unwrap_or_else(|| Reading::unknown("touchpad.backend_returned_nothing"))
    }

    /// Writes a plan and decides each outcome by reading the setting back.
    fn apply(&mut self, plan: &ApplyPlan) -> RunReport {
        let writes: Vec<(SettingId, Option<SettingValue>)> = plan
            .steps
            .iter()
            .map(|step| (step.setting, Some(step.requested)))
            .collect();
        let outcomes = self.write_all(&writes);
        let settings: Vec<SettingId> = plan.steps.iter().map(|step| step.setting).collect();
        let readings = self.read(&settings);

        let results = plan
            .steps
            .iter()
            .map(|step| {
                let outcome = outcome_for(&outcomes, step.setting);
                let reading = reading_for(&readings, step.setting);
                (
                    step.setting,
                    decide(outcome, step.requested, reading, step.effect),
                )
            })
            .collect();
        RunReport {
            results,
            skipped: plan.skipped.clone(),
        }
    }

    /// Puts a captured state back, then reads it back to say what happened.
    ///
    /// A reset is verified differently from a write: it succeeded when the
    /// setting holds nothing again, not when it holds some particular value.
    fn restore(&mut self, plan: &RestorePlan) -> RunReport {
        let writes: Vec<(SettingId, Option<SettingValue>)> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                RestoreStep::Write { setting, value } => Some((*setting, Some(*value))),
                RestoreStep::Reset { setting } => Some((*setting, None)),
                RestoreStep::Impossible { .. } => None,
            })
            .collect();
        let outcomes = self.write_all(&writes);
        let settings: Vec<SettingId> = writes.iter().map(|(setting, _)| *setting).collect();
        let readings = self.read(&settings);

        let results = plan
            .steps
            .iter()
            .map(|step| match step {
                RestoreStep::Impossible {
                    setting,
                    reason,
                    detail,
                } => (
                    *setting,
                    StepOutcome::Unsupported {
                        reason: reason.clone(),
                        detail: detail.clone(),
                    },
                ),
                RestoreStep::Write { setting, value } => {
                    let effect = self
                        .capabilities()
                        .support(*setting)
                        .effect()
                        .unwrap_or(SessionEffect::Immediate);
                    let outcome = outcome_for(&outcomes, *setting);
                    let reading = reading_for(&readings, *setting);
                    (*setting, decide(outcome, *value, reading, effect))
                }
                RestoreStep::Reset { setting } => {
                    let outcome = outcome_for(&outcomes, *setting);
                    let reading = reading_for(&readings, *setting);
                    (*setting, decide_reset(outcome, reading))
                }
            })
            .collect();
        RunReport {
            results,
            skipped: Vec::new(),
        }
    }
}

fn outcome_for(outcomes: &[(SettingId, WriteOutcome)], setting: SettingId) -> WriteOutcome {
    outcomes
        .iter()
        .find(|(id, _)| *id == setting)
        .map(|(_, outcome)| outcome.clone())
        .unwrap_or_else(|| {
            WriteOutcome::failed(
                "touchpad.backend_skipped_the_write",
                "the backend returned no outcome for this setting",
            )
        })
}

fn reading_for(readings: &[(SettingId, Reading)], setting: SettingId) -> Reading {
    readings
        .iter()
        .find(|(id, _)| *id == setting)
        .map(|(_, reading)| reading.clone())
        .unwrap_or_else(|| Reading::unknown("touchpad.read_back_returned_nothing"))
}

fn decide(
    outcome: WriteOutcome,
    requested: SettingValue,
    reading: Reading,
    effect: SessionEffect,
) -> StepOutcome {
    match outcome {
        WriteOutcome::Unsupported { reason, detail } => StepOutcome::Unsupported { reason, detail },
        WriteOutcome::Failed { reason, detail } => StepOutcome::Failed { reason, detail },
        WriteOutcome::Written => {
            if effect == SessionEffect::SignOutRequired {
                return StepOutcome::AwaitingSignOut { requested };
            }
            match reading.as_value() {
                Some(value) if agrees(&value, &requested) => {
                    StepOutcome::Applied { effective: reading }
                }
                Some(_) => StepOutcome::PartiallySupported {
                    requested,
                    effective: reading,
                },
                None => StepOutcome::Failed {
                    reason: "touchpad.verification_indeterminate".to_string(),
                    detail: reading
                        .reason()
                        .unwrap_or("the setting could not be read back")
                        .to_string(),
                },
            }
        }
    }
}

fn decide_reset(outcome: WriteOutcome, reading: Reading) -> StepOutcome {
    match outcome {
        WriteOutcome::Unsupported { reason, detail } => StepOutcome::Unsupported { reason, detail },
        WriteOutcome::Failed { reason, detail } => StepOutcome::Failed { reason, detail },
        WriteOutcome::Written => match &reading {
            Reading::SessionDefault { .. } => StepOutcome::Applied {
                effective: reading.clone(),
            },
            Reading::Value { value } => StepOutcome::PartiallySupported {
                requested: *value,
                effective: reading.clone(),
            },
            _ => StepOutcome::Failed {
                reason: "touchpad.verification_indeterminate".to_string(),
                detail: reading
                    .reason()
                    .unwrap_or("the setting could not be read back")
                    .to_string(),
            },
        },
    }
}

/// Whether two values are the same to the precision the mapping can carry.
pub fn agrees(left: &SettingValue, right: &SettingValue) -> bool {
    match (left.as_number(), right.as_number()) {
        (Some(left), Some(right)) => (left - right).abs() <= VALUE_TOLERANCE,
        _ => left == right,
    }
}

/// What the whole backend layer says about itself, for the Diagnostics screen.
#[derive(Clone, Debug, PartialEq)]
pub struct BackendStatus {
    pub name: &'static str,
    pub reachable: bool,
    /// A stable machine key when something is wrong.
    pub reason: Option<String>,
    pub detail: String,
}

impl BackendStatus {
    pub fn reachable(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            reachable: true,
            reason: None,
            detail: detail.into(),
        }
    }

    pub fn unreachable(
        name: &'static str,
        reason: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            name,
            reachable: false,
            reason: Some(reason.into()),
            detail: detail.into(),
        }
    }
}

/// Everything a backend needs to know about the machine it is running on.
pub fn unavailable_everywhere(reason: &str, detail: &str) -> Capabilities {
    let mut capabilities = Capabilities::new();
    for setting in SettingId::ALL {
        capabilities.insert(setting, Support::unavailable(reason, detail));
    }
    capabilities
}

#[cfg(test)]
mod tests {
    use super::*;
    use touchpad_core::{ScrollFactor, Sensitivity};

    #[test]
    fn two_numbers_a_scale_conversion_apart_still_agree() {
        let requested = SettingValue::sensitivity(Sensitivity::new(0.35).unwrap());
        let read_back = SettingValue::sensitivity(Sensitivity::new(0.35 + 1e-12).unwrap());
        assert!(agrees(&requested, &read_back));
    }

    #[test]
    fn two_numbers_a_user_visible_step_apart_do_not_agree() {
        let requested = SettingValue::sensitivity(Sensitivity::new(0.35).unwrap());
        let read_back = SettingValue::sensitivity(Sensitivity::new(0.36).unwrap());
        assert!(!agrees(&requested, &read_back));
    }

    #[test]
    fn values_that_are_not_numbers_compare_exactly() {
        assert!(agrees(
            &SettingValue::toggle(true),
            &SettingValue::toggle(true)
        ));
        assert!(!agrees(
            &SettingValue::toggle(true),
            &SettingValue::toggle(false)
        ));
        assert!(!agrees(
            &SettingValue::toggle(true),
            &SettingValue::factor(ScrollFactor::neutral())
        ));
    }

    #[test]
    fn a_backend_that_answers_for_nothing_makes_every_control_unavailable() {
        let capabilities = unavailable_everywhere("gnome.no_bus", "no session bus");
        assert!(capabilities.available().is_empty());
        assert_eq!(capabilities.unavailable().len(), SettingId::ALL.len());
    }
}
