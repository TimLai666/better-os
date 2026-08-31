//! Preview, confirm, capture, apply, verify.
//!
//! Issue #3's apply flow is five steps and this file is all five of them. Two
//! of the steps are enforced by types rather than by discipline:
//!
//! - **Nothing is applied without confirmation.** [`ApprovedGesturePlan`] has
//!   private fields and exactly one constructor, [`PresetPlan::approve`], which
//!   refuses without an explicit confirmation flag. The execution path takes an
//!   approved plan and nothing else, so a caller cannot apply a plan the user
//!   never saw. This is the shape `manager-gui`'s defaults review already uses.
//! - **A conflict is settled before anything moves.** `approve` refuses while
//!   any detected conflict has no resolution attached, so "disabled, remapped,
//!   or retained only after preview and explicit confirmation" is a property of
//!   the type rather than of the screen.
//!
//! Verification is a second question, never an inference. Binding an action and
//! having a binding are different claims, and the report carries both.

use std::collections::BTreeMap;

use better_actions::{ActionSupport, DesktopAction};
use thiserror::Error;
use touchpad_session::{BindOutcome, SessionAdapter, SuppressionOutcome, VerificationResult};

use crate::config::{GestureConfig, PresetId};
use crate::conflict::{BuiltInGesture, Conflict, ConflictResolution, annotate, detect};
use crate::definition::{
    ContactCount, GestureDefinition, GestureError, GestureId, VerificationRecord,
};
use crate::suppression::{SuppressionEvent, SuppressionState};

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum PlanError {
    #[error("gestures.plan.not_confirmed")]
    NotConfirmed,
    #[error("gestures.plan.unresolved_conflict:{0}")]
    UnresolvedConflict(String),
    #[error("gestures.plan.nothing_to_do")]
    NothingToDo,
    #[error(transparent)]
    Gesture(#[from] GestureError),
}

/// What one line of the preview says.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

impl ChangeKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
        }
    }
}

/// One change a plan would make, in the form the preview shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedChange {
    pub gesture: GestureId,
    pub kind: ChangeKind,
    /// What the gesture is now, as a machine-key summary. `None` when it is
    /// being added.
    pub before: Option<String>,
    /// What it would become. `None` when it is being removed.
    pub after: Option<String>,
}

/// Every difference between two configurations, in a stable order.
pub fn differences(before: &GestureConfig, after: &GestureConfig) -> Vec<PlannedChange> {
    let mut changes = Vec::new();
    for gesture in &after.gestures {
        match before.get(&gesture.id) {
            None => changes.push(PlannedChange {
                gesture: gesture.id.clone(),
                kind: ChangeKind::Added,
                before: None,
                after: Some(gesture.summary()),
            }),
            Some(existing) if comparable(existing) != comparable(gesture) => {
                changes.push(PlannedChange {
                    gesture: gesture.id.clone(),
                    kind: ChangeKind::Changed,
                    before: Some(existing.summary()),
                    after: Some(gesture.summary()),
                });
            }
            Some(_) => {}
        }
    }
    for gesture in &before.gestures {
        if after.get(&gesture.id).is_none() {
            changes.push(PlannedChange {
                gesture: gesture.id.clone(),
                kind: ChangeKind::Removed,
                before: Some(gesture.summary()),
                after: None,
            });
        }
    }
    changes
}

/// The fields that make two definitions the same binding. Conflict state and
/// the last verification are results, not configuration, so a re-run that only
/// refreshed them is not a change the preview should show.
fn comparable(gesture: &GestureDefinition) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        gesture.summary(),
        gesture.thumb_required,
        gesture.activation_threshold.get(),
        gesture.cancellation_threshold.get(),
        gesture.cooldown.as_millis(),
        gesture.animation_progress == crate::definition::AnimationProgress::WhenAvailable,
        gesture.backend.key(),
    )
}

/// A preview: every change, every conflict, and every action the adapter cannot
/// perform, worked out before anything is touched.
#[derive(Clone, Debug)]
pub struct PresetPlan {
    pub preset: PresetId,
    /// What is configured now. This is what a restore goes back to, and it is
    /// captured here rather than re-read later.
    pub previous: GestureConfig,
    pub proposed: GestureConfig,
    pub changes: Vec<PlannedChange>,
    pub conflicts: Vec<Conflict>,
    /// Actions the adapter says it cannot perform, so the preview can show a
    /// row that will not work before the user agrees to it rather than after.
    pub unsupported: Vec<(GestureId, ActionSupport)>,
}

impl PresetPlan {
    /// Builds the preview.
    pub fn build(
        current: &GestureConfig,
        proposed: &GestureConfig,
        built_ins: &[BuiltInGesture],
        adapter: &dyn SessionAdapter,
    ) -> Self {
        let mut proposed = proposed.clone();
        let conflicts = detect(&proposed, built_ins);
        annotate(&mut proposed, &conflicts);

        let unsupported = proposed
            .gestures
            .iter()
            .filter(|gesture| gesture.enabled && gesture.action.changes_something())
            .filter_map(|gesture| match adapter.support(&gesture.action) {
                ActionSupport::Supported { .. } => None,
                unsupported => Some((gesture.id.clone(), unsupported)),
            })
            .collect();

        Self {
            preset: proposed.preset,
            changes: differences(current, &proposed),
            previous: current.clone(),
            proposed,
            conflicts,
            unsupported,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The conflicts still waiting for a decision.
    pub fn unresolved(
        &self,
        resolutions: &BTreeMap<GestureId, ConflictResolution>,
    ) -> Vec<GestureId> {
        self.conflicts
            .iter()
            .filter(|conflict| !resolutions.contains_key(&conflict.gesture))
            .map(|conflict| conflict.gesture.clone())
            .collect()
    }

    /// The only way to produce an [`ApprovedGesturePlan`].
    ///
    /// `confirmed` is the user's own act. It is a separate argument rather than
    /// a flag on the plan so that building a preview cannot accidentally
    /// produce something applicable.
    pub fn approve(
        &self,
        resolutions: &BTreeMap<GestureId, ConflictResolution>,
        confirmed: bool,
    ) -> Result<ApprovedGesturePlan, PlanError> {
        // Conflicts first: there is no point asking whether the user confirmed
        // a plan that still has an undecided question in it.
        if let Some(unresolved) = self.unresolved(resolutions).first() {
            return Err(PlanError::UnresolvedConflict(unresolved.to_string()));
        }
        if !confirmed {
            return Err(PlanError::NotConfirmed);
        }

        let mut config = self.proposed.clone();
        let mut built_ins = Vec::new();
        for conflict in &self.conflicts {
            let resolution = resolutions[&conflict.gesture];
            match resolution {
                ConflictResolution::KeepBuiltIn => {
                    if let Some(gesture) = config.get_mut(&conflict.gesture) {
                        gesture.enabled = false;
                    }
                }
                ConflictResolution::DisableBuiltIn => {
                    built_ins.push((conflict.built_in.to_string(), resolution));
                }
                ConflictResolution::RemapOurs {
                    contacts,
                    direction,
                } => {
                    let mut moved = config
                        .get(&conflict.gesture)
                        .cloned()
                        .ok_or_else(|| GestureError::UnknownId(conflict.gesture.to_string()))?;
                    moved.contacts = ContactCount::new(contacts)?;
                    moved.direction = Some(direction);
                    config.replace(moved)?;
                }
            }
        }

        // Remapping moves gestures, so what conflicts is worked out again
        // rather than carried over from before the move.
        let remaining = detect(&config, crate::conflict::GNOME_46_GESTURES);
        annotate(&mut config, &remaining);

        let changes = differences(&self.previous, &config);
        if changes.is_empty() {
            return Err(PlanError::NothingToDo);
        }
        Ok(ApprovedGesturePlan {
            config,
            previous: self.previous.clone(),
            changes,
            built_ins,
        })
    }
}

/// A plan a preview produced and a person confirmed.
///
/// The fields are private and there is no other constructor, so a configuration
/// cannot reach [`ApprovedGesturePlan::apply`] without the preview and the
/// confirmation that produced it.
#[derive(Clone, Debug)]
pub struct ApprovedGesturePlan {
    config: GestureConfig,
    previous: GestureConfig,
    changes: Vec<PlannedChange>,
    built_ins: Vec<(String, ConflictResolution)>,
}

impl ApprovedGesturePlan {
    pub fn config(&self) -> &GestureConfig {
        &self.config
    }

    /// What was configured before this plan. Written to the capture before the
    /// first change, and what a restore goes back to.
    pub fn previous(&self) -> &GestureConfig {
        &self.previous
    }

    pub fn changes(&self) -> &[PlannedChange] {
        &self.changes
    }

    /// Whether any conflict in this plan was settled by taking the gesture away
    /// from the desktop.
    pub fn suppression_wanted(&self) -> bool {
        self.built_ins
            .iter()
            .any(|(_, resolution)| *resolution == ConflictResolution::DisableBuiltIn)
    }

    /// Applies the plan through the session adapter and verifies each binding.
    ///
    /// Returns the configuration as it now stands — carrying what verification
    /// actually said, per gesture — and the report. The caller stores the
    /// configuration; nothing here writes a file, because a plan that could
    /// write would be a plan that could write without being approved.
    ///
    /// This form starts from a fresh suppression state, which is right for a
    /// one-shot apply and for a test. A resident process that has to give the
    /// desktop its gestures back later keeps its own state and calls
    /// [`Self::apply_with`].
    pub fn apply(&self, adapter: &mut dyn SessionAdapter) -> (GestureConfig, ApplyReport) {
        self.apply_with(adapter, &mut SuppressionState::new())
    }

    /// The same, against a suppression state that outlives this apply.
    pub fn apply_with(
        &self,
        adapter: &mut dyn SessionAdapter,
        suppression: &mut SuppressionState,
    ) -> (GestureConfig, ApplyReport) {
        let (config, mut report) = bind_all(&self.config, adapter);
        // Every built-in gesture the user chose to take gets one call, because
        // the desktop's own trackers go off together or not at all. What that
        // call said is then reported against each of them, so a preview that
        // promised something and a desktop that still does the old thing can
        // never be the same screen.
        let outcome = if self.suppression_wanted() {
            Some(suppression.transition(SuppressionEvent::PlanApplied { wanted: true }, adapter))
        } else {
            None
        };
        for (built_in, resolution) in &self.built_ins {
            let reported = match &outcome {
                Some(SuppressionOutcome::Suppressed) => BuiltInOutcome::Suppressed,
                Some(SuppressionOutcome::Restored) | None => BuiltInOutcome::Unsupported {
                    reason: "gestures.built_in_not_changed".to_string(),
                    detail: format!(
                        "nothing asked the desktop to give up {built_in}; \
                         the resolution recorded was {}",
                        resolution.key()
                    ),
                },
                Some(SuppressionOutcome::Unsupported { reason, detail }) => {
                    BuiltInOutcome::Unsupported {
                        reason: reason.clone(),
                        detail: format!(
                            "{detail}, so {built_in} still belongs to the desktop; \
                             the resolution recorded was {}",
                            resolution.key()
                        ),
                    }
                }
                Some(SuppressionOutcome::Failed { reason, detail }) => BuiltInOutcome::Failed {
                    reason: reason.clone(),
                    detail: format!("{built_in} could not be given up: {detail}"),
                },
            };
            report.built_ins.push((built_in.clone(), reported));
        }
        (config, report)
    }
}

/// How many runs in a row an adapter has failed, and when that is enough to
/// turn the integration off by itself.
///
/// Issue #3 requires a repeatedly failing gesture adapter to be disabled
/// automatically, and "repeatedly" has to be a number somebody chose. It lives
/// here rather than in one screen because two things now run gestures — the
/// window and the resident pipeline — and a rule they could each have their own
/// copy of is a rule they can disagree about.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AdapterFailures {
    consecutive: u32,
}

impl AdapterFailures {
    /// Three rather than one: a single failure can be a session that was still
    /// starting, and turning every gesture off for that would be worse than the
    /// failure. Three in a row is a broken adapter.
    pub const BEFORE_DISABLE: u32 = 3;

    pub fn consecutive(&self) -> u32 {
        self.consecutive
    }

    /// Counts a run. Returns whether the integration should now be turned off.
    pub fn record(&mut self, state: RunState) -> bool {
        if state == RunState::Failed {
            self.consecutive += 1;
        } else {
            self.consecutive = 0;
            return false;
        }
        self.consecutive >= Self::BEFORE_DISABLE
    }

    pub fn reset(&mut self) {
        self.consecutive = 0;
    }
}

/// What happened to one gesture's binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindingOutcome {
    /// Bound, and a second look found the binding.
    Verified {
        continuous_progress: bool,
    },
    /// The adapter took the binding and could not then confirm it.
    Unverified {
        reason: String,
        detail: String,
    },
    Unsupported {
        reason: String,
        detail: String,
    },
    Failed {
        reason: String,
        detail: String,
    },
    /// Not attempted, and why.
    Skipped {
        reason: &'static str,
    },
}

impl BindingOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Verified { .. })
    }

    fn record(&self) -> VerificationRecord {
        match self {
            Self::Verified {
                continuous_progress,
            } => VerificationRecord::Verified {
                continuous_progress: *continuous_progress,
            },
            Self::Unverified { reason, detail } | Self::Failed { reason, detail } => {
                VerificationRecord::Failed {
                    reason: reason.clone(),
                    detail: detail.clone(),
                }
            }
            Self::Unsupported { reason, detail } => VerificationRecord::Unsupported {
                reason: reason.clone(),
                detail: detail.clone(),
            },
            Self::Skipped { .. } => VerificationRecord::NotRun,
        }
    }
}

/// What happened to a built-in gesture a resolution asked to change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuiltInOutcome {
    /// The desktop gave it up. Only the GNOME Shell adapter can produce this.
    Suppressed,
    Unsupported {
        reason: String,
        detail: String,
    },
    Failed {
        reason: String,
        detail: String,
    },
}

/// The single word for a whole run.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RunState {
    NothingToDo,
    Applied,
    PartiallySupported,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub results: Vec<(GestureId, BindingOutcome)>,
    pub built_ins: Vec<(String, BuiltInOutcome)>,
}

impl ApplyReport {
    pub fn outcome(&self, id: &GestureId) -> Option<&BindingOutcome> {
        self.results
            .iter()
            .find(|(gesture, _)| gesture == id)
            .map(|(_, outcome)| outcome)
    }

    /// The worst thing that happened, because that is what the user has to be
    /// told. A run where one gesture failed is a failed run.
    pub fn state(&self) -> RunState {
        let attempted: Vec<&BindingOutcome> = self
            .results
            .iter()
            .map(|(_, outcome)| outcome)
            .filter(|outcome| !matches!(outcome, BindingOutcome::Skipped { .. }))
            .collect();
        if attempted.is_empty() && self.built_ins.is_empty() {
            return RunState::NothingToDo;
        }
        if attempted.iter().any(|outcome| {
            matches!(
                outcome,
                BindingOutcome::Failed { .. } | BindingOutcome::Unverified { .. }
            )
        }) || self
            .built_ins
            .iter()
            .any(|(_, outcome)| matches!(outcome, BuiltInOutcome::Failed { .. }))
        {
            return RunState::Failed;
        }
        if attempted
            .iter()
            .any(|outcome| matches!(outcome, BindingOutcome::Unsupported { .. }))
            || self
                .built_ins
                .iter()
                .any(|(_, outcome)| matches!(outcome, BuiltInOutcome::Unsupported { .. }))
        {
            return RunState::PartiallySupported;
        }
        RunState::Applied
    }
}

/// Binds every enabled gesture and verifies each one.
///
/// Shared by applying a plan and by restoring a capture, so a restored
/// configuration is verified exactly the way an applied one is.
pub fn bind_all(
    config: &GestureConfig,
    adapter: &mut dyn SessionAdapter,
) -> (GestureConfig, ApplyReport) {
    let mut applied = config.clone();
    let mut report = ApplyReport::default();

    for gesture in &mut applied.gestures {
        let outcome = if !config.enabled {
            BindingOutcome::Skipped {
                reason: "gestures.integration_disabled",
            }
        } else if !gesture.enabled {
            BindingOutcome::Skipped {
                reason: "gestures.gesture_disabled",
            }
        } else if matches!(gesture.action, DesktopAction::Disabled) {
            BindingOutcome::Skipped {
                reason: "gestures.no_action",
            }
        } else {
            match adapter.bind(&gesture.action) {
                BindOutcome::Bound => match adapter.verify(&gesture.action) {
                    VerificationResult::Verified {
                        continuous_progress,
                    } => BindingOutcome::Verified {
                        continuous_progress: continuous_progress && gesture.can_animate(),
                    },
                    VerificationResult::Unverified { reason, detail } => {
                        BindingOutcome::Unverified { reason, detail }
                    }
                    VerificationResult::Unsupported { reason, detail } => {
                        BindingOutcome::Unsupported { reason, detail }
                    }
                },
                BindOutcome::Unsupported { reason, detail } => {
                    BindingOutcome::Unsupported { reason, detail }
                }
                BindOutcome::Failed { reason, detail } => BindingOutcome::Failed { reason, detail },
            }
        };
        gesture.last_verification = outcome.record();
        report.results.push((gesture.id.clone(), outcome));
    }
    (applied, report)
}

/// Putting a capture back.
///
/// There is no separate machinery for this: a restore is the captured
/// configuration bound and verified the same way an applied one is, which is
/// why a restored gesture carries a real verification result rather than an
/// assumption.
#[derive(Clone, Debug)]
pub struct RestorePlan {
    pub captured: GestureConfig,
    pub changes: Vec<PlannedChange>,
}

impl RestorePlan {
    pub fn from_capture(current: &GestureConfig, captured: &GestureConfig) -> Self {
        Self {
            captured: captured.clone(),
            changes: differences(current, captured),
        }
    }

    /// Turning the whole integration off.
    ///
    /// Disabling gestures restores what was captured and then leaves the
    /// subsystem switched off, so a machine whose gestures are turned off ends
    /// up where it was before Better Touchpad, not at some default.
    pub fn disable(current: &GestureConfig, captured: &GestureConfig) -> Self {
        let mut restored = captured.clone();
        restored.enabled = false;
        Self {
            changes: differences(current, &restored),
            captured: restored,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn apply(&self, adapter: &mut dyn SessionAdapter) -> (GestureConfig, ApplyReport) {
        self.apply_with(adapter, &mut SuppressionState::new())
    }

    /// The same, against a suppression state that outlives this restore.
    ///
    /// Putting the captured configuration back is also the moment the desktop
    /// gets its own gestures back, because a capture is what the machine looked
    /// like before Better Touchpad touched it.
    pub fn apply_with(
        &self,
        adapter: &mut dyn SessionAdapter,
        suppression: &mut SuppressionState,
    ) -> (GestureConfig, ApplyReport) {
        let event = if self.captured.enabled {
            SuppressionEvent::Restored
        } else {
            SuppressionEvent::Disabled
        };
        suppression.transition(event, adapter);
        bind_all(&self.captured, adapter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::GNOME_46_GESTURES;
    use crate::definition::Direction;
    use crate::preset::mac_style;
    use better_actions::{ActionCapabilities, ActionSupport};
    use touchpad_session::MockSessionAdapter;

    fn id(name: &str) -> GestureId {
        GestureId::new(name).unwrap()
    }

    fn keep_everything(plan: &PresetPlan) -> BTreeMap<GestureId, ConflictResolution> {
        plan.conflicts
            .iter()
            .map(|conflict| (conflict.gesture.clone(), ConflictResolution::DisableBuiltIn))
            .collect()
    }

    fn plan_for(adapter: &dyn SessionAdapter) -> PresetPlan {
        PresetPlan::build(
            &GestureConfig::default(),
            &mac_style(),
            GNOME_46_GESTURES,
            adapter,
        )
    }

    #[test]
    fn the_preview_lists_every_gesture_the_preset_would_add() {
        let adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        assert_eq!(plan.changes.len(), mac_style().gestures.len());
        assert!(
            plan.changes
                .iter()
                .all(|change| change.kind == ChangeKind::Added)
        );
        assert_eq!(plan.preset, PresetId::MacStyle);
        assert!(plan.previous.gestures.is_empty());
    }

    #[test]
    fn a_plan_cannot_be_applied_without_confirmation() {
        let adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        let resolutions = keep_everything(&plan);
        assert_eq!(
            plan.approve(&resolutions, false).err(),
            Some(PlanError::NotConfirmed)
        );
        assert!(plan.approve(&resolutions, true).is_ok());
    }

    #[test]
    fn a_plan_cannot_be_applied_while_a_conflict_has_no_decision() {
        let adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        assert_eq!(plan.unresolved(&BTreeMap::new()).len(), 4);
        assert!(matches!(
            plan.approve(&BTreeMap::new(), true),
            Err(PlanError::UnresolvedConflict(_))
        ));

        // Deciding three of the four is still not deciding.
        let mut partial = keep_everything(&plan);
        partial.remove(&id("workspace-previous"));
        assert_eq!(
            plan.approve(&partial, true).err(),
            Some(PlanError::UnresolvedConflict(
                "workspace-previous".to_string()
            ))
        );
    }

    #[test]
    fn keeping_the_built_in_gesture_disables_ours_instead() {
        let adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        let resolutions: BTreeMap<GestureId, ConflictResolution> = plan
            .conflicts
            .iter()
            .map(|conflict| (conflict.gesture.clone(), ConflictResolution::KeepBuiltIn))
            .collect();
        let approved = plan.approve(&resolutions, true).unwrap();

        assert!(!approved.config().get(&id("overview")).unwrap().enabled);
        // And the gestures that never conflicted are untouched.
        assert!(approved.config().get(&id("launcher")).unwrap().enabled);
        assert!(approved.built_ins.is_empty());
    }

    #[test]
    fn remapping_moves_our_gesture_out_of_the_desktops_way() {
        let adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        let mut resolutions = keep_everything(&plan);
        resolutions.insert(
            id("overview"),
            ConflictResolution::RemapOurs {
                contacts: 5,
                direction: Direction::Up,
            },
        );
        let approved = plan.approve(&resolutions, true).unwrap();
        let overview = approved.config().get(&id("overview")).unwrap();

        assert_eq!(overview.contacts.get(), 5);
        assert!(overview.enabled);
        // Five fingers is out of GNOME's reach, so the conflict is gone rather
        // than carried over from the preview.
        assert!(!overview.conflict.conflicts());
    }

    #[test]
    fn disabling_the_built_in_half_is_recorded_as_something_this_build_cannot_do() {
        let mut adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        let approved = plan.approve(&keep_everything(&plan), true).unwrap();
        let (_, report) = approved.apply(&mut adapter);

        assert_eq!(report.built_ins.len(), 4);
        assert!(matches!(
            report.built_ins[0].1,
            BuiltInOutcome::Unsupported { .. }
        ));
        assert_eq!(report.state(), RunState::PartiallySupported);
    }

    #[test]
    fn applying_binds_every_enabled_gesture_and_verifies_each_one() {
        let mut adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        let resolutions: BTreeMap<GestureId, ConflictResolution> = plan
            .conflicts
            .iter()
            .map(|conflict| (conflict.gesture.clone(), ConflictResolution::KeepBuiltIn))
            .collect();
        let approved = plan.approve(&resolutions, true).unwrap();
        let (config, report) = approved.apply(&mut adapter);

        assert_eq!(report.state(), RunState::Applied);
        assert!(
            report
                .outcome(&id("launcher"))
                .is_some_and(BindingOutcome::is_success)
        );
        assert_eq!(
            report.outcome(&id("overview")),
            Some(&BindingOutcome::Skipped {
                reason: "gestures.gesture_disabled"
            })
        );
        assert!(matches!(
            config.get(&id("launcher")).unwrap().last_verification,
            VerificationRecord::Verified {
                continuous_progress: true
            }
        ));
        assert_eq!(
            config.get(&id("overview")).unwrap().last_verification,
            VerificationRecord::NotRun
        );
        assert!(
            adapter
                .bound_keys()
                .contains(&"better-launcher.open".to_string())
        );
    }

    #[test]
    fn an_action_the_adapter_cannot_perform_is_shown_in_the_preview_not_discovered_later() {
        let adapter = MockSessionAdapter::with_capabilities(ActionCapabilities::everything().with(
            &DesktopAction::ShowOverview,
            ActionSupport::unsupported(
                "session.no_shell_adapter",
                "no adapter in this build reaches GNOME Shell",
            ),
        ));
        let plan = plan_for(&adapter);
        assert_eq!(
            plan.unsupported
                .iter()
                .map(|(gesture, _)| gesture.as_str())
                .collect::<Vec<_>>(),
            vec!["overview"]
        );
    }

    #[test]
    fn a_failing_adapter_makes_the_whole_run_a_failure() {
        let mut adapter = MockSessionAdapter::new().failing(&DesktopAction::LauncherOpen);
        let plan = plan_for(&adapter);
        let approved = plan
            .approve(
                &plan
                    .conflicts
                    .iter()
                    .map(|conflict| (conflict.gesture.clone(), ConflictResolution::KeepBuiltIn))
                    .collect(),
                true,
            )
            .unwrap();
        let (config, report) = approved.apply(&mut adapter);

        assert_eq!(report.state(), RunState::Failed);
        assert!(matches!(
            report.outcome(&id("launcher")),
            Some(BindingOutcome::Failed { .. })
        ));
        assert!(matches!(
            config.get(&id("launcher")).unwrap().last_verification,
            VerificationRecord::Failed { .. }
        ));
        // The gestures the adapter did not refuse still worked.
        assert!(
            report
                .outcome(&id("show-desktop"))
                .is_some_and(BindingOutcome::is_success)
        );
    }

    #[test]
    fn applying_the_same_preset_twice_is_nothing_to_do_rather_than_a_second_apply() {
        let adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        let approved = plan.approve(&keep_everything(&plan), true).unwrap();
        let settled = approved.config().clone();

        let again = PresetPlan::build(&settled, &settled, GNOME_46_GESTURES, &adapter);
        assert!(again.is_empty());
        assert_eq!(
            again.approve(&keep_everything(&again), true).err(),
            Some(PlanError::NothingToDo)
        );
    }

    #[test]
    fn restoring_puts_back_exactly_what_was_captured() {
        let mut adapter = MockSessionAdapter::new();
        let captured = GestureConfig::default();
        let plan = plan_for(&adapter);
        let approved = plan.approve(&keep_everything(&plan), true).unwrap();
        let (applied, _) = approved.apply(&mut adapter);
        assert_eq!(applied.gestures.len(), 10);

        let restore = RestorePlan::from_capture(&applied, &captured);
        assert_eq!(restore.changes.len(), 10);
        assert!(
            restore
                .changes
                .iter()
                .all(|change| change.kind == ChangeKind::Removed)
        );
        let (back, report) = restore.apply(&mut adapter);
        assert_eq!(back, captured);
        assert_eq!(report.state(), RunState::NothingToDo);
    }

    #[test]
    fn disabling_gestures_restores_the_capture_and_leaves_the_subsystem_off() {
        let mut adapter = MockSessionAdapter::new();
        let mut captured = GestureConfig::default();
        captured.gestures.push(
            GestureDefinition::new(
                "mine",
                crate::definition::GestureShape::Pinch,
                5,
                false,
                None,
                DesktopAction::LauncherOpen,
            )
            .unwrap(),
        );
        let plan = PresetPlan::build(&captured, &mac_style(), GNOME_46_GESTURES, &adapter);
        let approved = plan.approve(&keep_everything(&plan), true).unwrap();
        let (applied, _) = approved.apply(&mut adapter);

        let disable = RestorePlan::disable(&applied, &captured);
        let (back, report) = disable.apply(&mut adapter);
        assert!(!back.enabled);
        assert_eq!(back.gestures.len(), 1);
        assert_eq!(back.gestures[0].id.as_str(), "mine");
        // Nothing is bound while the integration is off.
        assert_eq!(report.state(), RunState::NothingToDo);
        assert!(back.active().is_empty());
    }

    #[test]
    fn a_confirmed_plan_that_takes_a_gesture_asks_the_desktop_to_give_it_up() {
        use std::sync::Arc;
        use touchpad_session::gnome::{
            FakeShellBridge, GnomeShellAdapter, SharedShellBridge, ShellRequest,
        };

        let recorded = Arc::new(FakeShellBridge::new());
        let mut adapter =
            GnomeShellAdapter::connect(Box::new(SharedShellBridge(recorded.clone()))).unwrap();
        let plan = plan_for(&adapter);
        let approved = plan.approve(&keep_everything(&plan), true).unwrap();
        assert!(approved.suppression_wanted());

        let mut suppression = SuppressionState::new();
        let (_, report) = approved.apply_with(&mut adapter, &mut suppression);
        assert_eq!(report.built_ins.len(), 4);
        assert!(
            report
                .built_ins
                .iter()
                .all(|(_, outcome)| *outcome == BuiltInOutcome::Suppressed)
        );
        // One call, not one per conflict: the desktop's trackers go together.
        assert_eq!(
            recorded.calls(),
            vec![ShellRequest::SuppressBuiltInGestures(true)]
        );
        assert!(suppression.is_suppressed());

        // And restoring the capture gives them back.
        let restore = RestorePlan::from_capture(approved.config(), approved.previous());
        restore.apply_with(&mut adapter, &mut suppression);
        assert!(!suppression.is_suppressed());
        assert_eq!(
            recorded.calls().last(),
            Some(&ShellRequest::SuppressBuiltInGestures(false))
        );
    }

    #[test]
    fn a_plan_where_the_desktop_keeps_everything_asks_for_no_suppression() {
        let adapter = MockSessionAdapter::new();
        let plan = plan_for(&adapter);
        let resolutions: BTreeMap<GestureId, ConflictResolution> = plan
            .conflicts
            .iter()
            .map(|conflict| (conflict.gesture.clone(), ConflictResolution::KeepBuiltIn))
            .collect();
        let approved = plan.approve(&resolutions, true).unwrap();
        assert!(!approved.suppression_wanted());
    }

    #[test]
    fn a_suppression_that_failed_makes_the_whole_run_a_failure() {
        use touchpad_session::gnome::{FakeShellBridge, GnomeShellAdapter, ShellError};

        let mut adapter = GnomeShellAdapter::with_reported(
            Box::new(FakeShellBridge::failing(ShellError::CallFailed(
                "the shell did not answer".to_string(),
            ))),
            FakeShellBridge::gnome_46_capabilities(),
        );
        let plan = plan_for(&adapter);
        let approved = plan.approve(&keep_everything(&plan), true).unwrap();
        let (_, report) = approved.apply(&mut adapter);
        assert_eq!(report.state(), RunState::Failed);
        assert!(matches!(
            report.built_ins[0].1,
            BuiltInOutcome::Failed { .. }
        ));
    }

    #[test]
    fn three_failed_runs_in_a_row_are_what_turns_the_integration_off() {
        let mut failures = AdapterFailures::default();
        assert!(!failures.record(RunState::Failed));
        assert!(!failures.record(RunState::Failed));
        assert_eq!(failures.consecutive(), 2);
        assert!(failures.record(RunState::Failed));
        assert_eq!(failures.consecutive(), AdapterFailures::BEFORE_DISABLE);

        // And one good run clears the count rather than decrementing it.
        let mut failures = AdapterFailures::default();
        failures.record(RunState::Failed);
        failures.record(RunState::Failed);
        assert!(!failures.record(RunState::Applied));
        assert_eq!(failures.consecutive(), 0);
        assert!(!failures.record(RunState::Failed));
    }

    #[test]
    fn a_run_state_is_the_worst_thing_that_happened() {
        let verified = (
            id("a"),
            BindingOutcome::Verified {
                continuous_progress: true,
            },
        );
        let unsupported = (
            id("b"),
            BindingOutcome::Unsupported {
                reason: "r".into(),
                detail: "d".into(),
            },
        );
        let failed = (
            id("c"),
            BindingOutcome::Failed {
                reason: "r".into(),
                detail: "d".into(),
            },
        );
        assert_eq!(ApplyReport::default().state(), RunState::NothingToDo);
        assert_eq!(
            ApplyReport {
                results: vec![verified.clone()],
                built_ins: Vec::new()
            }
            .state(),
            RunState::Applied
        );
        assert_eq!(
            ApplyReport {
                results: vec![verified.clone(), unsupported.clone()],
                built_ins: Vec::new()
            }
            .state(),
            RunState::PartiallySupported
        );
        assert_eq!(
            ApplyReport {
                results: vec![verified, unsupported, failed],
                built_ins: Vec::new()
            }
            .state(),
            RunState::Failed
        );
    }
}
