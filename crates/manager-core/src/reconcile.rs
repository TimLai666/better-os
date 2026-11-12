//! Making the recorded intent true when Better Manager starts.
//!
//! A component the manager records as enabled is one the person asked to run.
//! The packages install their user units without enabling them, and an upgrade
//! of Better Manager does not re-run the step that would have enabled them, so
//! a machine can carry a correct record and a stopped service. That is the
//! state the owner's machine was in, and it is fixed here: at startup, every
//! enabled component whose units are not enabled and running has them enabled
//! and started.
//!
//! This is not asked again, because the record already is the answer — and
//! that includes a unit someone disabled by hand with `systemctl`: the record
//! still says enabled, so it is enabled again and the activity log says it was
//! done because of the record. A disabled component is left alone, a component
//! dpkg cannot describe is skipped rather than guessed at, a masked unit is
//! reported once rather than at every start, and a failure is reported as a
//! warning and a degraded component instead of being retried in a loop.
//!
//! The pass is two halves so a window can run the slow one off its UI thread:
//! [`Manager::run_user_unit_healing`] asks systemd and starts units, reading
//! the state and changing none of it, and [`Manager::record_user_unit_healing`]
//! writes what happened into whatever the state is when the answer lands.
//! [`Manager::heal_user_units`] is both, in order.

use manager_platform::dpkg::PackageStateProbe;
use manager_platform::user_units::{UnitActivity, UnitFailure, UserUnitController, service_units};

use crate::{ActivityKind, ComponentId, HealthState, Manager, ManagerError, ManagerState};

/// One thing the startup pass did, or found, for one component.
///
/// It is only produced when something was attempted or found wrong, so an
/// empty report means there was nothing to do — not that the pass failed to
/// notice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserUnitHealing {
    pub component: ComponentId,
    pub units: Vec<String>,
    /// `true` when the units were enabled and started, `false` when the attempt
    /// failed or the unit is masked, and [`failure`](Self::failure) says why.
    pub started: bool,
    pub failure: Option<UnitFailure>,
    /// The unit is masked. Nothing was attempted, because enabling a masked
    /// unit fails until someone unmasks it.
    pub masked: bool,
}

impl UserUnitHealing {
    /// The activity entry's evidence.
    fn evidence(&self) -> String {
        if self.masked {
            format!("user_units.masked:{}", self.units.join(","))
        } else if let Some(failure) = &self.failure {
            format!("user_units.failed:{}", failure.unit)
        } else {
            format!("user_units.started:{}", self.units.join(","))
        }
    }
}

impl Manager {
    /// Enables and starts the user units of every component recorded as
    /// installed and enabled, and reports what it did and found. It reads the
    /// state and changes none of it; [`record_user_unit_healing`] does that.
    ///
    /// Nothing happens while a transaction is in progress: that transaction
    /// owns its components' units, and starting or stopping one underneath it
    /// would race the driver. A component whose file list cannot be read is
    /// skipped: saying nothing about a package the host will not describe is
    /// not the same as claiming it has no units. When there is no user session
    /// to ask — the bus cannot be reached, or `systemctl` is not there — the
    /// pass stops and reports nothing more, because nothing was learned about
    /// any unit and no component is any less healthy for it.
    ///
    /// [`record_user_unit_healing`]: Self::record_user_unit_healing
    pub fn run_user_unit_healing(
        &self,
        state: &ManagerState,
        packages: &dyn PackageStateProbe,
        units: &dyn UserUnitController,
    ) -> Result<Vec<UserUnitHealing>, ManagerError> {
        self.validate_state(state)?;
        let mut report = Vec::new();
        if state.active_operation.is_some() {
            return Ok(report);
        }

        for manifest in self.manifests() {
            let Some(record) = state.components.get(&manifest.id) else {
                continue;
            };
            if !(record.enabled && record.installed_version.is_some()) {
                continue;
            }
            let Ok(files) = packages.installed_files(manifest.id.as_str()) else {
                // The host will not say what the package installed. The health
                // check is where an unreadable dpkg gets reported.
                continue;
            };
            let names = service_units(&files);
            if names.is_empty() {
                continue;
            }
            let component = manifest.id.clone();

            let states = match units.states(&names) {
                Ok(states) => states,
                Err(failure) if failure.is_no_session() => return Ok(report),
                Err(failure) => {
                    report.push(UserUnitHealing {
                        component,
                        units: names,
                        started: false,
                        failure: Some(failure),
                        masked: false,
                    });
                    continue;
                }
            };

            let masked = states
                .iter()
                .filter(|state| state.masked)
                .map(|state| state.unit.clone())
                .collect::<Vec<_>>();
            // A unit that exited successfully on its own is working as
            // designed, and one that is still activating is not ours to
            // interrupt.
            let needing = states
                .into_iter()
                .filter(|state| {
                    !state.masked
                        && !state.is_satisfied()
                        && state.activity != UnitActivity::Starting
                })
                .map(|state| state.unit)
                .collect::<Vec<_>>();

            if let Some(first) = masked.first() {
                report.push(UserUnitHealing {
                    component: component.clone(),
                    failure: Some(UnitFailure::new(first, "the unit is masked")),
                    units: masked,
                    started: false,
                    masked: true,
                });
            }
            if needing.is_empty() {
                continue;
            }
            match units.enable_now(&needing) {
                Ok(()) => report.push(UserUnitHealing {
                    component,
                    units: needing,
                    started: true,
                    failure: None,
                    masked: false,
                }),
                Err(failure) if failure.is_no_session() => return Ok(report),
                Err(failure) => report.push(UserUnitHealing {
                    component,
                    units: needing,
                    started: false,
                    failure: Some(failure),
                    masked: false,
                }),
            }
        }
        Ok(report)
    }

    /// Writes what [`run_user_unit_healing`] reported into `state`, and returns
    /// the entries that changed it.
    ///
    /// The state may have moved on while the units were being started, so each
    /// entry is checked against it again: a component that is no longer
    /// enabled, or a state with a transaction now in progress, takes nothing. A
    /// start records a success and returns a component that is `Degraded` with
    /// no failure of its own to `Healthy`; a failure records a warning and
    /// leaves the component `Degraded`. A masked unit is recorded once: when
    /// the component's last unit entry already says so, nothing is written.
    /// Nothing changed means nothing written, not even the revision.
    ///
    /// [`run_user_unit_healing`]: Self::run_user_unit_healing
    pub fn record_user_unit_healing(
        &self,
        state: &mut ManagerState,
        report: &[UserUnitHealing],
    ) -> Vec<UserUnitHealing> {
        let mut recorded = Vec::new();
        if state.active_operation.is_some() {
            return recorded;
        }
        for healing in report {
            let Some(record) = state.components.get(&healing.component) else {
                continue;
            };
            if !(record.enabled && record.installed_version.is_some()) {
                continue;
            }
            let evidence = healing.evidence();
            if healing.masked
                && last_unit_evidence(state, &healing.component) == Some(evidence.as_str())
            {
                continue;
            }
            let Some(record) = state.components.get_mut(&healing.component) else {
                continue;
            };
            let kind = if healing.started {
                if record.health == HealthState::Degraded && record.failure.is_none() {
                    record.health = HealthState::Healthy;
                }
                ActivityKind::Success
            } else {
                record.health = HealthState::Degraded;
                ActivityKind::Warning
            };
            state.record(
                kind,
                Some(healing.component.clone()),
                None,
                None,
                Some(evidence),
            );
            recorded.push(healing.clone());
        }
        if !recorded.is_empty() {
            state.bump_revision();
        }
        recorded
    }

    /// Both halves of the startup pass, in order, and the entries that changed
    /// the state.
    pub fn heal_user_units(
        &self,
        state: &mut ManagerState,
        packages: &dyn PackageStateProbe,
        units: &dyn UserUnitController,
    ) -> Result<Vec<UserUnitHealing>, ManagerError> {
        let report = self.run_user_unit_healing(state, packages, units)?;
        Ok(self.record_user_unit_healing(state, &report))
    }
}

/// The evidence of the newest activity entry about this component's units.
fn last_unit_evidence<'a>(state: &'a ManagerState, component: &ComponentId) -> Option<&'a str> {
    state
        .activity
        .iter()
        .rev()
        .filter(|entry| entry.component.as_ref() == Some(component))
        .filter_map(|entry| entry.evidence.as_deref())
        .find(|evidence| evidence.starts_with("user_units."))
}
