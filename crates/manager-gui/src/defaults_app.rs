//! The Defaults section's state and the work it runs off the UI thread.
//!
//! Every read and every change goes through `defaults-core`. This file builds
//! the engine's inputs, hands them to a background task, and adopts what comes
//! back; it never touches a setting itself, and there is no adapter type named
//! anywhere in `manager-gui`.
//!
//! The one invariant worth reading the code for: [`ApprovedPlan::run`] is the
//! only place a plan is executed, and an `ApprovedPlan` can only be built by
//! [`ReviewModel::approve`]. A button that applied defaults without a review
//! screen would have nothing to call.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use better_core::defaults::IntegrationId;
use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use defaults_core::{
    AdapterMode, AdapterSession, ComponentReadiness, Confirmations, DefaultsEngine,
    DefaultsOutcome, DefaultsPlan, DefaultsReport, PlanKind, Selection, SystemContext,
};
use defaults_store::{SnapshotHistory, SnapshotStore};
use gpui::{AppContext as _, Context, Task};
use manager_core::HealthState;

use crate::app::ManagerApp;
use crate::defaults_model::{ApprovedPlan, ReviewModel, last_verified_times};
use crate::model::Page;

/// Everything the Defaults screens hold between interactions.
#[derive(Default)]
pub(crate) struct DefaultsState {
    pub(crate) report: Option<DefaultsReport>,
    pub(crate) verified: BTreeMap<(ComponentId, IntegrationId), u64>,
    pub(crate) review: Option<ReviewModel>,
    pub(crate) outcome: Option<DefaultsOutcome>,
    /// What the current review is about, so confirming an entry can plan again
    /// over the same components.
    pub(crate) scope: Option<(PlanKind, Selection)>,
    pub(crate) busy: bool,
    pub(crate) failed: bool,
    pub(crate) job: Option<Task<()>>,
}

/// What the background task is being asked to do.
pub(crate) enum DefaultsJob {
    Inspect,
    Verify(Selection),
    Plan {
        kind: PlanKind,
        selection: Selection,
        confirmed: Vec<(ComponentId, IntegrationId)>,
    },
    Run(ApprovedPlan),
}

/// What it came back with.
pub(crate) enum DefaultsEvent {
    Report(Box<Reading>),
    Planned(Box<DefaultsPlan>),
    Finished(Box<DefaultsOutcome>),
    Failed,
}

pub(crate) struct Reading {
    pub(crate) report: DefaultsReport,
    pub(crate) verified: BTreeMap<(ComponentId, IntegrationId), u64>,
}

/// Everything the engine needs, in a form that can cross a thread.
pub(crate) struct DefaultsInputs {
    pub(crate) manifests: Vec<ComponentManifest>,
    pub(crate) readiness: Vec<(ComponentId, ComponentReadiness)>,
    pub(crate) system: SystemContext,
    pub(crate) snapshot_directory: PathBuf,
    pub(crate) mode: AdapterMode,
}

impl ManagerApp {
    /// Opens the Defaults section, reading the current settings if this window
    /// has not read them yet.
    pub(crate) fn open_defaults(&mut self, cx: &mut Context<Self>) {
        self.navigate(Page::Defaults, cx);
        if self.defaults.report.is_none() && !self.defaults.busy {
            self.refresh_defaults(cx);
        }
    }

    /// Reads what every declared integration currently says.
    pub(crate) fn refresh_defaults(&mut self, cx: &mut Context<Self>) {
        self.start_defaults_job(DefaultsJob::Inspect, cx);
    }

    /// Reads every integration again and records what was seen.
    pub(crate) fn verify_defaults(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.start_defaults_job(DefaultsJob::Verify(selection), cx);
    }

    /// Opens a review screen. Both top-level actions and both per-component
    /// actions land here; they differ only in the selection.
    pub(crate) fn review_defaults(
        &mut self,
        kind: PlanKind,
        selection: Selection,
        cx: &mut Context<Self>,
    ) {
        self.defaults.review = None;
        self.defaults.outcome = None;
        self.defaults.scope = Some((kind, selection.clone()));
        self.start_defaults_job(
            DefaultsJob::Plan {
                kind,
                selection,
                confirmed: Vec::new(),
            },
            cx,
        );
    }

    pub(crate) fn toggle_defaults_component(
        &mut self,
        component: &ComponentId,
        cx: &mut Context<Self>,
    ) {
        if let Some(review) = self.defaults.review.as_mut() {
            review.toggle(component);
        }
        cx.notify();
    }

    /// Agrees, or stops agreeing, to replace one entry that changed outside
    /// Better Manager. The plan is built again, because whether an entry is
    /// held back is decided while it is built.
    pub(crate) fn toggle_defaults_confirmation(
        &mut self,
        component: &ComponentId,
        integration: &IntegrationId,
        cx: &mut Context<Self>,
    ) {
        let Some(review) = self.defaults.review.as_mut() else {
            return;
        };
        review.toggle_confirmation(component, integration);
        let confirmed = review.confirmed_entries();
        let Some((kind, selection)) = self.defaults.scope.clone() else {
            return;
        };
        self.start_defaults_job(
            DefaultsJob::Plan {
                kind,
                selection,
                confirmed,
            },
            cx,
        );
    }

    /// Applies what the review screen shows. This is the only path to a change.
    pub(crate) fn apply_defaults_review(&mut self, cx: &mut Context<Self>) {
        let Some(approved) = self.defaults.review.as_ref().and_then(ReviewModel::approve) else {
            cx.notify();
            return;
        };
        self.start_defaults_job(DefaultsJob::Run(approved), cx);
    }

    pub(crate) fn cancel_defaults_review(&mut self, cx: &mut Context<Self>) {
        self.defaults.review = None;
        self.defaults.scope = None;
        self.navigate(Page::Defaults, cx);
    }

    pub(crate) fn open_defaults_component(
        &mut self,
        component: &ComponentId,
        cx: &mut Context<Self>,
    ) {
        self.navigate(Page::DefaultsComponent(component.clone()), cx);
    }

    fn start_defaults_job(&mut self, job: DefaultsJob, cx: &mut Context<Self>) {
        let inputs = self.defaults_inputs();
        self.defaults.busy = true;
        self.defaults.failed = false;
        let task = cx.spawn(async move |this, cx| {
            let event = cx
                .background_spawn(async move { run_job(&inputs, job) })
                .await;
            let _ = this.update(cx, |app, cx| app.apply_defaults_event(event, cx));
        });
        self.defaults.job = Some(task);
        cx.notify();
    }

    fn apply_defaults_event(&mut self, event: DefaultsEvent, cx: &mut Context<Self>) {
        self.defaults.busy = false;
        match event {
            DefaultsEvent::Report(reading) => {
                self.defaults.report = Some(reading.report);
                self.defaults.verified = reading.verified;
                cx.notify();
            }
            DefaultsEvent::Planned(plan) => {
                let locale = self.locale;
                let names = self.defaults_names();
                let resolve = |component: &ComponentId| component_name(&names, component);
                let review = match self.defaults.review.take() {
                    Some(review) => review.rebuilt(*plan),
                    None => ReviewModel::new(locale, *plan, &resolve),
                };
                self.defaults.review = Some(review);
                self.navigate(Page::DefaultsReview, cx);
            }
            DefaultsEvent::Finished(outcome) => {
                self.defaults.outcome = Some(*outcome);
                self.defaults.review = None;
                self.defaults.scope = None;
                self.navigate(Page::DefaultsResults, cx);
                // What the screen shows next has to come from a fresh read, not
                // from what the run believed it wrote.
                self.refresh_defaults(cx);
            }
            DefaultsEvent::Failed => {
                self.defaults.failed = true;
                cx.notify();
            }
        }
    }

    /// The names components are shown under, for the models that need one.
    pub(crate) fn defaults_names(&self) -> BTreeMap<ComponentId, String> {
        self.manager
            .manifests()
            .map(|manifest| (manifest.id.clone(), self.plan_component_name(&manifest.id)))
            .collect()
    }

    fn defaults_inputs(&self) -> DefaultsInputs {
        DefaultsInputs {
            manifests: self.manager.manifests().cloned().collect(),
            readiness: self
                .manager
                .manifests()
                .map(|manifest| (manifest.id.clone(), self.readiness_of(&manifest.id)))
                .collect(),
            system: SystemContext::new(
                self.manager.profile().distribution.clone(),
                defaults_core::adapters::desktop_session(),
            ),
            snapshot_directory: SnapshotStore::from_default_path().directory().to_path_buf(),
            mode: if self.is_demo() {
                AdapterMode::Simulated {
                    desktop_path: Some(defaults_core::adapters::default_simulated_desktop_path()),
                }
            } else {
                AdapterMode::Production
            },
        }
    }

    /// What Better Manager knows about a component, which is what decides
    /// whether an integration's health prerequisites are met.
    fn readiness_of(&self, component: &ComponentId) -> ComponentReadiness {
        match self.state.component(component) {
            None => ComponentReadiness::default(),
            Some(record) => ComponentReadiness {
                installed: record.installed_version.is_some(),
                enabled: record.enabled,
                healthy: record.health == HealthState::Healthy,
            },
        }
    }
}

impl ApprovedPlan {
    /// Runs a plan a person confirmed on a review screen.
    ///
    /// This is the only call to the engine's execution path in this crate, and
    /// it cannot be reached without an approved plan.
    fn run(
        &self,
        engine: &DefaultsEngine<'_>,
        session: &mut AdapterSession,
        store: &SnapshotStore,
    ) -> Option<DefaultsOutcome> {
        let outcome = engine
            .execute(self.plan(), session.adapters_mut(), store)
            .ok()?;
        session.persist().ok()?;
        Some(outcome)
    }
}

/// The whole of one Defaults job, on a background thread.
pub(crate) fn run_job(inputs: &DefaultsInputs, job: DefaultsJob) -> DefaultsEvent {
    let Ok(catalog) = ComponentCatalog::from_manifests(inputs.manifests.clone()) else {
        return DefaultsEvent::Failed;
    };
    let mut engine = DefaultsEngine::new(&catalog, inputs.system.clone());
    for (component, readiness) in &inputs.readiness {
        engine = engine.with_readiness(component.clone(), *readiness);
    }
    let store = SnapshotStore::at_path(inputs.snapshot_directory.clone());
    let Ok(mut session) = AdapterSession::open(&inputs.mode) else {
        return DefaultsEvent::Failed;
    };

    match job {
        DefaultsJob::Inspect => {
            let Ok(history) = store.history() else {
                return DefaultsEvent::Failed;
            };
            let report = engine.inspect(&Selection::All, session.adapters(), &history);
            DefaultsEvent::Report(Box::new(reading(report, &history)))
        }
        DefaultsJob::Verify(selection) => {
            match engine.verify(&selection, session.adapters(), &store) {
                Ok(report) => match store.history() {
                    Ok(history) => DefaultsEvent::Report(Box::new(reading(report, &history))),
                    Err(_) => DefaultsEvent::Failed,
                },
                Err(_) => DefaultsEvent::Failed,
            }
        }
        DefaultsJob::Plan {
            kind,
            selection,
            confirmed,
        } => {
            let Ok(history) = store.history() else {
                return DefaultsEvent::Failed;
            };
            let confirmations = confirmed.into_iter().fold(
                Confirmations::none(),
                |confirmations, (component, integration)| {
                    confirmations.with(component, integration)
                },
            );
            let plan = match kind {
                PlanKind::Apply => {
                    engine.plan_apply(&selection, session.adapters(), &history, &confirmations)
                }
                PlanKind::Restore => {
                    engine.plan_restore(&selection, session.adapters(), &history, &confirmations)
                }
            };
            DefaultsEvent::Planned(Box::new(plan))
        }
        DefaultsJob::Run(approved) => match approved.run(&engine, &mut session, &store) {
            Some(outcome) => DefaultsEvent::Finished(Box::new(outcome)),
            None => DefaultsEvent::Failed,
        },
    }
}

fn reading(report: DefaultsReport, history: &SnapshotHistory) -> Reading {
    Reading {
        verified: last_verified_times(history),
        report,
    }
}

/// The name a component is shown under, falling back to its own id.
pub(crate) fn component_name(
    names: &BTreeMap<ComponentId, String>,
    component: &ComponentId,
) -> String {
    names
        .get(component)
        .cloned()
        .unwrap_or_else(|| component.to_string())
}

/// Now, in the units a snapshot records.
pub(crate) fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}
