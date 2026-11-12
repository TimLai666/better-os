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
//! This is not asked again, because the record already is the answer. A
//! disabled component is left alone, a component dpkg cannot describe is
//! skipped rather than guessed at, and a failure is reported as a warning and a
//! degraded component instead of being retried in a loop.

use manager_platform::dpkg::PackageStateProbe;
use manager_platform::user_units::{UnitFailure, UserUnitController, service_units};

use crate::{ActivityKind, ComponentId, HealthState, Manager, ManagerError, ManagerState};

/// One component the startup pass acted on.
///
/// It is only produced when something was attempted, so an empty report means
/// there was nothing to do — not that the pass failed to notice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserUnitHealing {
    pub component: ComponentId,
    pub units: Vec<String>,
    /// `true` when the units were enabled and started, `false` when the attempt
    /// failed and [`failure`](Self::failure) says why.
    pub started: bool,
    pub failure: Option<UnitFailure>,
}

impl Manager {
    /// Enables and starts the user units of every component recorded as
    /// installed and enabled, and reports only the components it acted on.
    ///
    /// A component whose file list cannot be read is skipped: saying nothing
    /// about a package the host will not describe is not the same as claiming
    /// it has no units, and the health check reports the unreadable dpkg state
    /// where a person is already looking. A unit that fails leaves the
    /// component degraded and its activity entry names the unit and what
    /// systemctl said; a pass that has nothing to do changes nothing, not even
    /// the revision.
    pub fn heal_user_units(
        &self,
        state: &mut ManagerState,
        packages: &dyn PackageStateProbe,
        units: &dyn UserUnitController,
    ) -> Result<Vec<UserUnitHealing>, ManagerError> {
        self.validate_state(state)?;
        let mut healed = Vec::new();
        let mut changed = false;

        for manifest in self.manifests() {
            let Some(record) = state.components.get(&manifest.id) else {
                continue;
            };
            let enabled = record.enabled && record.installed_version.is_some();
            if !enabled {
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

            let needing = match units.states(&names) {
                Ok(states) => states
                    .into_iter()
                    .filter(|state| !(state.enabled && state.active))
                    .map(|state| state.unit)
                    .collect::<Vec<_>>(),
                Err(failure) => {
                    let component = manifest.id.clone();
                    state.record(
                        ActivityKind::Warning,
                        Some(component.clone()),
                        None,
                        None,
                        Some(format!("user_units.failed:{}", failure.unit)),
                    );
                    if let Some(record) = state.components.get_mut(&component) {
                        record.health = HealthState::Degraded;
                    }
                    healed.push(UserUnitHealing {
                        component,
                        units: names,
                        started: false,
                        failure: Some(failure),
                    });
                    changed = true;
                    continue;
                }
            };

            if needing.is_empty() {
                // Already running: nothing happened, so nothing is recorded.
                continue;
            }

            let component = manifest.id.clone();
            match units.enable_now(&needing) {
                Ok(()) => {
                    state.record(
                        ActivityKind::Success,
                        Some(component.clone()),
                        None,
                        None,
                        Some(format!("user_units.started:{}", needing.join(","))),
                    );
                    healed.push(UserUnitHealing {
                        component,
                        units: needing,
                        started: true,
                        failure: None,
                    });
                }
                Err(failure) => {
                    state.record(
                        ActivityKind::Warning,
                        Some(component.clone()),
                        None,
                        None,
                        Some(format!("user_units.failed:{}", failure.unit)),
                    );
                    if let Some(record) = state.components.get_mut(&component) {
                        record.health = HealthState::Degraded;
                    }
                    healed.push(UserUnitHealing {
                        component,
                        units: needing,
                        started: false,
                        failure: Some(failure),
                    });
                }
            }
            changed = true;
        }

        if changed {
            state.bump_revision();
        }
        Ok(healed)
    }
}
