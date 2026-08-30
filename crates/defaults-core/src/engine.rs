//! One planning and verification path, used by every operation.
//!
//! Inspecting, planning one component, planning all of them, applying,
//! verifying, and restoring all go through the same three steps: read the
//! effective value now, compare it with what Better Manager last wrote or
//! verified, and only then decide. That is what makes an external change
//! impossible to miss — there is no second path that skips the read.

use better_core::defaults::{
    AdapterId, DefaultIntegration, DefaultsValue, IntegrationExclusivity, ObservedValue,
    RequiredPrivilege, RestorePolicy, SessionEffect,
};
use better_core::manifest::{ComponentCatalog, ComponentId, ComponentManifest};
use defaults_platform::{AdapterRequest, AdapterSet, VerifyOutcome, WriteOutcome};
use defaults_store::{
    RestoreState, Snapshot, SnapshotEntry, SnapshotError, SnapshotHistory, SnapshotStore,
    SystemIdentity,
};
use std::collections::BTreeMap;

use crate::plan::{
    Confirmations, DefaultsOutcome, DefaultsPlan, EntryOutcome, EntryResult, PlanAction, PlanEntry,
    PlanKind, PlanWarning, Selection, SkipReason,
};
use crate::status::{
    AggregateState, ComponentDefaults, ComponentReadiness, DefaultsReport, IntegrationState,
    IntegrationStatus, SystemContext,
};

/// Reads declarations, decides status, and produces plans.
pub struct DefaultsEngine<'a> {
    catalog: &'a ComponentCatalog,
    system: SystemContext,
    readiness: BTreeMap<ComponentId, ComponentReadiness>,
}

impl<'a> DefaultsEngine<'a> {
    pub fn new(catalog: &'a ComponentCatalog, system: SystemContext) -> Self {
        Self {
            catalog,
            system,
            readiness: BTreeMap::new(),
        }
    }

    /// Records what the manager knows about a component. A component nothing is
    /// recorded for counts as not installed, which is the only safe reading.
    pub fn with_readiness(mut self, component: ComponentId, readiness: ComponentReadiness) -> Self {
        self.readiness.insert(component, readiness);
        self
    }

    pub fn system(&self) -> &SystemContext {
        &self.system
    }

    fn readiness(&self, component: &ComponentId) -> ComponentReadiness {
        self.readiness.get(component).copied().unwrap_or_default()
    }

    /// Components that declare at least one integration, in a stable order.
    fn declaring(&self, selection: &Selection) -> Vec<&'a ComponentManifest> {
        let mut manifests: Vec<&ComponentManifest> = self
            .catalog
            .manifests()
            .filter(|manifest| !manifest.default_integrations.is_empty())
            .filter(|manifest| selection.covers(&manifest.id))
            .collect();
        manifests.sort_by(|left, right| left.id.cmp(&right.id));
        manifests
    }

    /// Why this integration cannot be acted on at all, if it cannot.
    fn unavailable_reason(
        &self,
        component: &ComponentId,
        integration: &DefaultIntegration,
    ) -> Option<Unavailable> {
        if !integration.applies_to(&self.system.distribution, &self.system.desktop_session) {
            return Some(Unavailable::NotApplicableHere);
        }
        if let Some(prerequisite) = self
            .readiness(component)
            .first_unmet(&integration.health_prerequisites)
        {
            return Some(Unavailable::PrerequisiteNotMet(prerequisite));
        }
        if integration.privileges == RequiredPrivilege::Administrator {
            // A privileged executor for defaults is deliberately not part of
            // this implementation, so an administrator-scope integration is
            // reported rather than attempted.
            return Some(Unavailable::RequiresAdministrator);
        }
        None
    }

    /// Another installed component that declares the same exclusive setting and
    /// currently owns it.
    fn conflicting_claimant(
        &self,
        component: &ComponentId,
        integration: &DefaultIntegration,
        current: &ObservedValue,
    ) -> Option<ComponentId> {
        if integration.exclusivity != IntegrationExclusivity::Exclusive {
            return None;
        }
        let current = current.value()?;
        let mut claimants: Vec<ComponentId> = self
            .catalog
            .manifests()
            .filter(|manifest| &manifest.id != component)
            .filter(|manifest| self.readiness(&manifest.id).installed)
            .filter(|manifest| {
                manifest.default_integrations.iter().any(|other| {
                    other.kind == integration.kind
                        && other
                            .target
                            .keys
                            .iter()
                            .any(|key| integration.target.keys.contains(key))
                        && &other.target.desired == current
                })
            })
            .map(|manifest| manifest.id.clone())
            .collect();
        claimants.sort();
        claimants.into_iter().next()
    }

    fn status_for(
        &self,
        component: &ComponentId,
        integration: &DefaultIntegration,
        adapters: &AdapterSet,
        history: &SnapshotHistory,
    ) -> IntegrationStatus {
        let record = history.latest_entry(component, &integration.id);
        let mut status = IntegrationStatus {
            integration: integration.id.clone(),
            kind: integration.kind,
            state: IntegrationState::Default,
            current: ObservedValue::Unknown {
                reason: "defaults.not_read".to_string(),
            },
            desired: integration.target.desired.clone(),
            session_effect: integration.session_effect,
            restore_available: record.is_some_and(|record| {
                record.restore_state == RestoreState::Available
                    && record.previous_value.is_determinate()
            }),
            last_verified_value: record.and_then(|record| record.last_verified_value.clone()),
        };

        if let Some(unavailable) = self.unavailable_reason(component, integration) {
            status.state = IntegrationState::Unavailable {
                reason: unavailable.machine_key(),
            };
            status.current = ObservedValue::Unknown {
                reason: unavailable.machine_key(),
            };
            return status;
        }
        let Some(adapter) = adapters.get(integration.verify_adapter) else {
            let reason = no_adapter_key(integration.verify_adapter);
            status.state = IntegrationState::Unknown {
                reason: reason.clone(),
            };
            status.current = ObservedValue::Unsupported { reason };
            return status;
        };

        let request = AdapterRequest::new(component, integration);
        let current = adapter.read(&request);
        let matches_desired = current.value() == Some(&integration.target.desired);
        status.current = current.clone();

        // Order matters here. A value Better Manager wrote that is not visible
        // yet is a pending sign-out, not somebody else's edit, so it is decided
        // before external-change detection gets to compare the two.
        let applied_but_not_effective = record.is_some_and(|record| {
            record.applied_value.as_ref() == Some(&integration.target.desired)
        }) && !matches_desired
            && integration.session_effect != SessionEffect::Immediate;
        if applied_but_not_effective {
            status.state = IntegrationState::NeedsSignOut;
            return status;
        }
        if let Some(claimant) = self.conflicting_claimant(component, integration, &current) {
            status.state = IntegrationState::Conflict { claimant };
            return status;
        }
        if let Some(last_known) = record.and_then(SnapshotEntry::last_known_value) {
            if current.is_determinate() && current.value() != Some(last_known) {
                status.state = IntegrationState::ChangedExternally {
                    last_known: Some(last_known.clone()),
                };
                return status;
            }
        }
        if !current.is_determinate() {
            status.state = IntegrationState::Unknown {
                reason: unknown_key(&current),
            };
            return status;
        }
        status.state = if matches_desired {
            IntegrationState::Default
        } else {
            IntegrationState::NotDefault
        };
        status
    }

    /// The whole Defaults view: every declaring component, its aggregate, and
    /// every integration underneath it.
    pub fn inspect(
        &self,
        selection: &Selection,
        adapters: &AdapterSet,
        history: &SnapshotHistory,
    ) -> DefaultsReport {
        let components = self
            .declaring(selection)
            .into_iter()
            .map(|manifest| {
                let integrations: Vec<IntegrationStatus> = manifest
                    .default_integrations
                    .iter()
                    .map(|integration| {
                        self.status_for(&manifest.id, integration, adapters, history)
                    })
                    .collect();
                ComponentDefaults {
                    component: manifest.id.clone(),
                    aggregate: AggregateState::derive(&integrations),
                    integrations,
                }
            })
            .collect();
        DefaultsReport {
            components,
            damaged_snapshots: damaged_paths(history),
        }
    }

    /// Plans making the selected components the defaults.
    pub fn plan_apply(
        &self,
        selection: &Selection,
        adapters: &AdapterSet,
        history: &SnapshotHistory,
        confirmations: &Confirmations,
    ) -> DefaultsPlan {
        self.plan(PlanKind::Apply, selection, adapters, history, confirmations)
    }

    /// Plans putting back what was there before Better OS changed it.
    pub fn plan_restore(
        &self,
        selection: &Selection,
        adapters: &AdapterSet,
        history: &SnapshotHistory,
        confirmations: &Confirmations,
    ) -> DefaultsPlan {
        self.plan(
            PlanKind::Restore,
            selection,
            adapters,
            history,
            confirmations,
        )
    }

    fn plan(
        &self,
        kind: PlanKind,
        selection: &Selection,
        adapters: &AdapterSet,
        history: &SnapshotHistory,
        confirmations: &Confirmations,
    ) -> DefaultsPlan {
        let mut entries = Vec::new();
        for manifest in self.declaring(selection) {
            for integration in &manifest.default_integrations {
                entries.push(self.plan_entry(
                    kind,
                    &manifest.id,
                    integration,
                    adapters,
                    history,
                    confirmations,
                ));
            }
        }
        DefaultsPlan::new(kind, entries, damaged_paths(history))
    }

    fn plan_entry(
        &self,
        kind: PlanKind,
        component: &ComponentId,
        integration: &DefaultIntegration,
        adapters: &AdapterSet,
        history: &SnapshotHistory,
        confirmations: &Confirmations,
    ) -> PlanEntry {
        let status = self.status_for(component, integration, adapters, history);
        let record = history.latest_entry(component, &integration.id);
        let mut entry = PlanEntry {
            component: component.clone(),
            integration: integration.id.clone(),
            kind: integration.kind,
            adapter: match kind {
                PlanKind::Apply => integration.apply_adapter,
                PlanKind::Restore => integration.apply_adapter,
            },
            action: PlanAction::Skip {
                reason: SkipReason::AlreadyDefault,
            },
            current: status.current.clone(),
            desired: integration.target.desired.clone(),
            captured_previous: record.map(|record| record.previous_value.clone()),
            session_effect: integration.session_effect,
            requires_confirmation: false,
            confirmed: false,
            warnings: Vec::new(),
        };

        // Anything that stops the entry being acted on at all comes first, in
        // the same order status derivation uses, so a plan can never disagree
        // with the status it was built from.
        let skip = match &status.state {
            IntegrationState::Unavailable { reason } => Some(unavailable_skip(reason)),
            IntegrationState::Conflict { claimant } => Some(SkipReason::Conflict {
                claimant: claimant.clone(),
            }),
            IntegrationState::Unknown { reason }
                if !adapters.contains(integration.verify_adapter) =>
            {
                let _ = reason;
                Some(SkipReason::NoProductionAdapter {
                    adapter: integration.verify_adapter,
                })
            }
            IntegrationState::Unknown { reason } => Some(SkipReason::EffectiveValueUnknown {
                reason: reason.clone(),
            }),
            _ => None,
        };
        if let Some(reason) = skip {
            entry.action = PlanAction::Skip { reason };
            return entry;
        }
        if !adapters.contains(integration.apply_adapter) {
            entry.action = PlanAction::Skip {
                reason: SkipReason::NoProductionAdapter {
                    adapter: integration.apply_adapter,
                },
            };
            return entry;
        }

        // The value changed after Better Manager last wrote or verified it.
        // Overwriting it needs this entry, specifically, to be confirmed.
        if matches!(status.state, IntegrationState::ChangedExternally { .. }) {
            entry.requires_confirmation = true;
            if !confirmations.contains(component, &integration.id) {
                entry.action = PlanAction::Skip {
                    reason: SkipReason::ChangedExternallyWithoutConfirmation {
                        current: status.current.clone(),
                    },
                };
                return entry;
            }
            entry.confirmed = true;
            entry.warnings.push(PlanWarning::OverwritesExternalChange {
                current: status.current.clone(),
            });
        }

        match kind {
            PlanKind::Apply => {
                if status.state == IntegrationState::Default {
                    entry.action = PlanAction::Skip {
                        reason: SkipReason::AlreadyDefault,
                    };
                    return entry;
                }
                if entry.captured_previous.is_none() && !status.current.is_determinate() {
                    entry.warnings.push(PlanWarning::PreviousValueIndeterminate);
                }
                entry.action = PlanAction::Apply {
                    to: integration.target.desired.clone(),
                };
            }
            PlanKind::Restore => {
                if integration.restore_policy == RestorePolicy::ManualOnly {
                    entry.action = PlanAction::Skip {
                        reason: SkipReason::NothingCaptured,
                    };
                    return entry;
                }
                let Some(captured) = record.map(|record| record.previous_value.clone()) else {
                    entry.action = PlanAction::Skip {
                        reason: SkipReason::NothingCaptured,
                    };
                    return entry;
                };
                if !captured.is_determinate() {
                    entry.action = PlanAction::Skip {
                        reason: SkipReason::NothingCaptured,
                    };
                    return entry;
                }
                if status.current == captured {
                    entry.action = PlanAction::Skip {
                        reason: SkipReason::AlreadyRestored,
                    };
                    return entry;
                }
                entry.action = PlanAction::Restore { to: captured };
            }
        }
        match integration.session_effect {
            SessionEffect::SignOut => entry.warnings.push(PlanWarning::NeedsSignOut),
            SessionEffect::Restart => entry.warnings.push(PlanWarning::NeedsRestart),
            SessionEffect::Immediate => {}
        }
        entry
    }

    /// Runs a plan: capture, change, verify, record.
    ///
    /// The baseline snapshot is written before the first change, so a crash in
    /// the middle leaves a record of a change that may not have happened rather
    /// than a change with no record.
    pub fn execute(
        &self,
        plan: &DefaultsPlan,
        adapters: &mut AdapterSet,
        store: &SnapshotStore,
    ) -> Result<DefaultsOutcome, SnapshotError> {
        let history = store.history()?;
        let mut baseline_snapshot = None;

        let captures: Vec<SnapshotEntry> = plan
            .changes()
            .filter(|entry| {
                history
                    .latest_entry(&entry.component, &entry.integration)
                    .is_none()
            })
            .map(|entry| SnapshotEntry {
                component_id: entry.component.clone(),
                integration_id: entry.integration.clone(),
                previous_value: entry.current.clone(),
                better_value: entry.desired.clone(),
                applied_value: None,
                last_verified_value: None,
                restore_state: if entry.current.is_determinate() {
                    RestoreState::Available
                } else {
                    RestoreState::NotCaptured
                },
            })
            .collect();
        let mut latest = history.latest().cloned();
        if !captures.is_empty() {
            let snapshot = match &latest {
                Some(previous) => previous.evolve(captures),
                None => Snapshot::new(self.identity(), captures),
            };
            store.write(&snapshot)?;
            baseline_snapshot = Some(snapshot.snapshot_id.as_str().to_string());
            latest = Some(snapshot);
        }

        let mut results = Vec::new();
        let mut updates = Vec::new();
        for entry in &plan.entries {
            let (outcome, update) = self.run_entry(entry, adapters, latest.as_ref());
            results.push(EntryResult {
                component: entry.component.clone(),
                integration: entry.integration.clone(),
                outcome,
            });
            if let Some(update) = update {
                updates.push(update);
            }
        }

        let mut recorded_snapshot = None;
        if !updates.is_empty() {
            let snapshot = match &latest {
                Some(previous) => previous.evolve(updates),
                None => Snapshot::new(self.identity(), updates),
            };
            store.write(&snapshot)?;
            recorded_snapshot = Some(snapshot.snapshot_id.as_str().to_string());
        }

        Ok(DefaultsOutcome {
            kind: plan.kind,
            results,
            baseline_snapshot,
            recorded_snapshot,
        })
    }

    fn identity(&self) -> SystemIdentity {
        SystemIdentity {
            distribution: self.system.distribution.clone(),
            desktop_session: self.system.desktop_session.clone(),
        }
    }

    fn run_entry(
        &self,
        entry: &PlanEntry,
        adapters: &mut AdapterSet,
        latest: Option<&Snapshot>,
    ) -> (EntryOutcome, Option<SnapshotEntry>) {
        if let PlanAction::Skip { reason } = &entry.action {
            return (
                EntryOutcome::Skipped {
                    reason: reason.clone(),
                },
                None,
            );
        }

        let Some(manifest) = self.catalog.get(&entry.component) else {
            return (
                EntryOutcome::Failed {
                    reason: "defaults.component_left_the_catalog".to_string(),
                    detail: None,
                },
                None,
            );
        };
        let Some(integration) = manifest
            .default_integrations
            .iter()
            .find(|integration| integration.id == entry.integration)
        else {
            return (
                EntryOutcome::Failed {
                    reason: "defaults.integration_left_the_manifest".to_string(),
                    detail: None,
                },
                None,
            );
        };

        let request = AdapterRequest::new(&entry.component, integration);
        let target = match &entry.action {
            PlanAction::Apply { to } => ObservedValue::Set { value: to.clone() },
            PlanAction::Restore { to } => to.clone(),
            PlanAction::Skip { .. } => unreachable!("handled above"),
        };

        let Some(adapter) = adapters.get_mut(integration.apply_adapter) else {
            return (
                EntryOutcome::Skipped {
                    reason: SkipReason::NoProductionAdapter {
                        adapter: integration.apply_adapter,
                    },
                },
                None,
            );
        };
        let write = match &entry.action {
            PlanAction::Apply { .. } => adapter.apply(&request),
            PlanAction::Restore { to } => adapter.restore(&request, to),
            PlanAction::Skip { .. } => unreachable!("handled above"),
        };
        match write {
            WriteOutcome::ManualActionRequired { reason, detail } => {
                return (EntryOutcome::ManualActionRequired { reason, detail }, None);
            }
            WriteOutcome::Failed { reason, detail } => {
                return (EntryOutcome::Failed { reason, detail }, None);
            }
            WriteOutcome::Written | WriteOutcome::AlreadyCorrect => {}
        }

        // Every change is verified by reading it back through the declared
        // verify adapter, which may not be the one that wrote it.
        let verifier = adapters
            .get(integration.verify_adapter)
            .or_else(|| adapters.get(integration.apply_adapter));
        let verified = match verifier {
            Some(verifier) => verifier.verify(&request, &target),
            None => VerifyOutcome::Indeterminate {
                observed: ObservedValue::Unsupported {
                    reason: no_adapter_key(integration.verify_adapter),
                },
            },
        };

        let previous = latest
            .and_then(|snapshot| snapshot.entry(&entry.component, &entry.integration))
            .cloned();
        let outcome = match (&entry.action, &verified) {
            (PlanAction::Apply { to }, VerifyOutcome::Matches { .. }) => {
                EntryOutcome::Applied { value: to.clone() }
            }
            (PlanAction::Apply { to }, VerifyOutcome::Differs { observed })
                if integration.session_effect != SessionEffect::Immediate =>
            {
                let _ = observed;
                EntryOutcome::AppliedNeedsSignOut { value: to.clone() }
            }
            (PlanAction::Apply { .. }, VerifyOutcome::Differs { observed }) => {
                EntryOutcome::NotVerified {
                    observed: observed.clone(),
                }
            }
            (PlanAction::Restore { to }, VerifyOutcome::Matches { .. }) => {
                EntryOutcome::Restored { value: to.clone() }
            }
            (PlanAction::Restore { .. }, VerifyOutcome::Differs { observed }) => {
                EntryOutcome::NotVerified {
                    observed: observed.clone(),
                }
            }
            (_, VerifyOutcome::Indeterminate { observed }) => {
                EntryOutcome::VerificationInconclusive {
                    observed: observed.clone(),
                }
            }
            (PlanAction::Skip { .. }, _) => unreachable!("handled above"),
        };

        let update = self.snapshot_update(entry, integration, &outcome, previous);
        (outcome, update)
    }

    fn snapshot_update(
        &self,
        entry: &PlanEntry,
        integration: &DefaultIntegration,
        outcome: &EntryOutcome,
        previous: Option<SnapshotEntry>,
    ) -> Option<SnapshotEntry> {
        let mut record = previous.unwrap_or(SnapshotEntry {
            component_id: entry.component.clone(),
            integration_id: entry.integration.clone(),
            previous_value: entry.current.clone(),
            better_value: integration.target.desired.clone(),
            applied_value: None,
            last_verified_value: None,
            restore_state: if entry.current.is_determinate() {
                RestoreState::Available
            } else {
                RestoreState::NotCaptured
            },
        });
        match outcome {
            EntryOutcome::Applied { value } | EntryOutcome::AppliedNeedsSignOut { value } => {
                record.applied_value = Some(value.clone());
                record.last_verified_value = Some(value.clone());
            }
            EntryOutcome::Restored { value } => {
                record.applied_value = None;
                record.last_verified_value = value.value().cloned();
                record.restore_state = RestoreState::AlreadyRestored;
            }
            EntryOutcome::AlreadyCorrect => {
                record.applied_value = Some(integration.target.desired.clone());
                record.last_verified_value = Some(integration.target.desired.clone());
            }
            // A write whose effect could not be confirmed must not update the
            // record of what Better Manager believes it owns, or the next run
            // would compare against a value that was never observed.
            _ => return None,
        }
        Some(record)
    }

    /// Reads every selected integration again and records what it saw, so the
    /// next external-change comparison is against a value that was actually
    /// observed rather than one that was merely written.
    pub fn verify(
        &self,
        selection: &Selection,
        adapters: &AdapterSet,
        store: &SnapshotStore,
    ) -> Result<DefaultsReport, SnapshotError> {
        let history = store.history()?;
        let report = self.inspect(selection, adapters, &history);

        let mut updates = Vec::new();
        for component in &report.components {
            for status in &component.integrations {
                let Some(record) = history.latest_entry(&component.component, &status.integration)
                else {
                    continue;
                };
                if !status.current.is_determinate() {
                    continue;
                }
                let observed = status.current.value().cloned();
                if record.last_verified_value == observed {
                    continue;
                }
                updates.push(SnapshotEntry {
                    last_verified_value: observed,
                    ..record.clone()
                });
            }
        }
        if !updates.is_empty() {
            let snapshot = match history.latest() {
                Some(previous) => previous.evolve(updates),
                None => Snapshot::new(self.identity(), updates),
            };
            store.write(&snapshot)?;
        }
        Ok(report)
    }
}

enum Unavailable {
    NotApplicableHere,
    PrerequisiteNotMet(better_core::defaults::HealthPrerequisite),
    RequiresAdministrator,
}

impl Unavailable {
    fn machine_key(&self) -> String {
        match self {
            Self::NotApplicableHere => "defaults.not_supported_on_this_system".to_string(),
            Self::PrerequisiteNotMet(prerequisite) => {
                format!("defaults.prerequisite_not_met:{prerequisite:?}")
            }
            Self::RequiresAdministrator => "defaults.requires_administrator".to_string(),
        }
    }
}

fn unavailable_skip(reason: &str) -> SkipReason {
    use better_core::defaults::HealthPrerequisite::{Enabled, Healthy, Installed};

    match reason {
        "defaults.requires_administrator" => SkipReason::RequiresAdministrator,
        "defaults.prerequisite_not_met:Installed" => SkipReason::PrerequisiteNotMet {
            prerequisite: Installed,
        },
        "defaults.prerequisite_not_met:Enabled" => SkipReason::PrerequisiteNotMet {
            prerequisite: Enabled,
        },
        "defaults.prerequisite_not_met:Healthy" => SkipReason::PrerequisiteNotMet {
            prerequisite: Healthy,
        },
        _ => SkipReason::NotApplicableHere,
    }
}

fn no_adapter_key(adapter: AdapterId) -> String {
    format!("defaults.no_production_adapter:{adapter:?}")
}

fn unknown_key(observed: &ObservedValue) -> String {
    match observed {
        ObservedValue::Unknown { reason }
        | ObservedValue::Unsupported { reason }
        | ObservedValue::PermissionDenied { reason } => reason.clone(),
        _ => "defaults.effective_value_unknown".to_string(),
    }
}

fn damaged_paths(history: &SnapshotHistory) -> Vec<String> {
    history
        .damaged()
        .iter()
        .map(|damaged| damaged.path.display().to_string())
        .collect()
}

/// The desired value a component wants for one integration, for callers that
/// only need to show it.
pub fn desired_value(integration: &DefaultIntegration) -> &DefaultsValue {
    &integration.target.desired
}
