//! What the Defaults screens show, decided without drawing anything.
//!
//! Every rule this file holds is testable without a window: which of the eight
//! aggregate states a component is in, what its row says, which entries a
//! review screen preselects, what the bottom summary counts, and what each
//! per-entry result means afterwards.
//!
//! One rule is structural rather than presentational. [`ApprovedPlan`] is the
//! only thing the execution path accepts, and the only way to build one is
//! [`ReviewModel::approve`], which exists only once a review screen has been
//! built from a plan. There is deliberately no constructor that turns a plan
//! into an approved plan directly, so no code path can run one without the
//! review that produced it.

use std::collections::{BTreeMap, BTreeSet};

use better_core::defaults::{
    DefaultsValue, IntegrationId, IntegrationKind, ObservedValue, SessionEffect,
};
use better_core::{ComponentIcon, ComponentId};
use defaults_core::{
    AggregateState, DefaultsOutcome, DefaultsPlan, DefaultsReport, EntryOutcome, IntegrationState,
    IntegrationStatus, PlanAction, PlanEntry, PlanKind, PlanWarning, SkipReason,
};
use defaults_store::SnapshotHistory;

use crate::i18n::{Locale, copy};

/// The compact summary above the component list: how many components are
/// defaults, how many can be changed, how many were changed elsewhere.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DefaultsSummary {
    pub(crate) total: usize,
    pub(crate) are_default: usize,
    pub(crate) can_change: usize,
    pub(crate) changed_externally: usize,
}

impl DefaultsSummary {
    pub(crate) fn of(report: &DefaultsReport) -> Self {
        let mut summary = Self {
            total: report.components.len(),
            ..Self::default()
        };
        for component in &report.components {
            match component.aggregate {
                AggregateState::Default => summary.are_default += 1,
                AggregateState::NotDefault | AggregateState::PartiallyDefault => {
                    summary.can_change += 1
                }
                AggregateState::ChangedExternally => {
                    summary.changed_externally += 1;
                    summary.can_change += 1;
                }
                _ => {}
            }
        }
        summary
    }
}

/// The one action a component row leads with. A component that is already the
/// default gets a status, never a switch that would mean nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrimaryAction {
    MakeDefault,
    AlreadyDefault,
    Verify,
}

impl PrimaryAction {
    pub(crate) fn of(aggregate: &AggregateState) -> Self {
        match aggregate {
            AggregateState::Default => Self::AlreadyDefault,
            AggregateState::NotDefault
            | AggregateState::PartiallyDefault
            | AggregateState::ChangedExternally => Self::MakeDefault,
            AggregateState::Unavailable { .. }
            | AggregateState::Conflict { .. }
            | AggregateState::Unknown { .. }
            | AggregateState::NeedsSignOut => Self::Verify,
        }
    }

    pub(crate) fn label(self, locale: Locale) -> &'static str {
        let c = copy(locale);
        match self {
            Self::MakeDefault => c.make_default,
            Self::AlreadyDefault => c.state_default,
            Self::Verify => c.verify_again,
        }
    }
}

/// What a row offers besides its primary action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecondaryAction {
    ReviewChanges,
    RestorePreviousDefault,
    VerifyAgain,
}

/// One integration, as the detail view lists it. A partial state is never
/// hidden: every declared integration gets its own line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct IntegrationRow {
    pub(crate) integration: IntegrationId,
    pub(crate) kind: IntegrationKind,
    pub(crate) state: IntegrationState,
    pub(crate) current_owner: String,
    pub(crate) target_owner: String,
    pub(crate) session_effect: SessionEffect,
    pub(crate) restore_available: bool,
    pub(crate) last_verified: Option<u64>,
}

/// One component, as the Defaults screen lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DefaultsRow {
    pub(crate) component: ComponentId,
    pub(crate) name: String,
    pub(crate) icon: ComponentIcon,
    pub(crate) kinds: Vec<IntegrationKind>,
    pub(crate) aggregate: AggregateState,
    pub(crate) current_owner: String,
    pub(crate) target_owner: String,
    pub(crate) last_verified: Option<u64>,
    pub(crate) restore_available: bool,
    pub(crate) primary: PrimaryAction,
    pub(crate) secondary: Vec<SecondaryAction>,
    pub(crate) integrations: Vec<IntegrationRow>,
}

impl DefaultsRow {
    /// A stable element-id fragment. Component ids are already restricted to
    /// lowercase ASCII, digits, and dashes by the manifest parser.
    pub(crate) fn element_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.component)
    }
}

/// When each integration was last read back, taken from the snapshot history.
///
/// There is no per-entry timestamp in a snapshot, so this is the moment the
/// newest snapshot that records a verified value for this integration was
/// written. That is the last time Better Manager confirmed what the setting
/// said, which is exactly what the row claims.
pub(crate) fn last_verified_times(
    history: &SnapshotHistory,
) -> BTreeMap<(ComponentId, IntegrationId), u64> {
    let mut times = BTreeMap::new();
    for snapshot in history.snapshots() {
        for entry in &snapshot.entries {
            if entry.last_verified_value.is_some() {
                times.insert(
                    (entry.component_id.clone(), entry.integration_id.clone()),
                    snapshot.created_at,
                );
            }
        }
    }
    times
}

/// Builds the rows the Defaults screen draws.
pub(crate) fn component_rows(
    locale: Locale,
    report: &DefaultsReport,
    verified: &BTreeMap<(ComponentId, IntegrationId), u64>,
    name_of: &dyn Fn(&ComponentId) -> String,
    icon_of: &dyn Fn(&ComponentId) -> ComponentIcon,
) -> Vec<DefaultsRow> {
    report
        .components
        .iter()
        .map(|component| {
            let integrations: Vec<IntegrationRow> = component
                .integrations
                .iter()
                .map(|status| IntegrationRow {
                    integration: status.integration.clone(),
                    kind: status.kind,
                    state: status.state.clone(),
                    current_owner: observed_label(locale, &status.current),
                    target_owner: value_label(locale, &status.desired),
                    session_effect: status.session_effect,
                    restore_available: status.restore_available,
                    last_verified: verified
                        .get(&(component.component.clone(), status.integration.clone()))
                        .copied(),
                })
                .collect();
            let restore_available = integrations.iter().any(|row| row.restore_available);
            let mut secondary = vec![SecondaryAction::ReviewChanges];
            if restore_available {
                secondary.push(SecondaryAction::RestorePreviousDefault);
            }
            secondary.push(SecondaryAction::VerifyAgain);
            DefaultsRow {
                component: component.component.clone(),
                name: name_of(&component.component),
                icon: icon_of(&component.component),
                kinds: distinct_kinds(&component.integrations),
                current_owner: shared_label(
                    locale,
                    integrations.iter().map(|row| &row.current_owner),
                ),
                target_owner: shared_label(
                    locale,
                    integrations.iter().map(|row| &row.target_owner),
                ),
                last_verified: integrations
                    .iter()
                    .filter_map(|row| row.last_verified)
                    .max(),
                restore_available,
                primary: PrimaryAction::of(&component.aggregate),
                secondary,
                aggregate: component.aggregate.clone(),
                integrations,
            }
        })
        .collect()
}

fn distinct_kinds(statuses: &[IntegrationStatus]) -> Vec<IntegrationKind> {
    let mut kinds: Vec<IntegrationKind> = Vec::new();
    for status in statuses {
        if !kinds.contains(&status.kind) {
            kinds.push(status.kind);
        }
    }
    kinds
}

/// One label when every integration says the same thing, and an explicit
/// "several different values" when they do not. Collapsing a mixed state into
/// one owner is exactly what the detail view exists to prevent.
fn shared_label<'a>(locale: Locale, mut labels: impl Iterator<Item = &'a String>) -> String {
    let Some(first) = labels.next() else {
        return copy(locale).none.to_string();
    };
    if labels.all(|label| label == first) {
        first.clone()
    } else {
        copy(locale).value_mixed.to_string()
    }
}

pub(crate) fn value_label(locale: Locale, value: &DefaultsValue) -> String {
    let c = copy(locale);
    match value {
        DefaultsValue::DesktopEntry(entry) => entry.clone(),
        DefaultsValue::Text(text) => text.clone(),
        DefaultsValue::TextList(values) => values.join(", "),
        DefaultsValue::Boolean(true) => c.value_on.to_string(),
        DefaultsValue::Boolean(false) => c.value_off.to_string(),
    }
}

/// What the system said, in words. The machine keys an adapter returns are
/// diagnostics, not user-facing text, so none of them reach the screen.
pub(crate) fn observed_label(locale: Locale, observed: &ObservedValue) -> String {
    let c = copy(locale);
    match observed {
        ObservedValue::Set { value } => value_label(locale, value),
        ObservedValue::Unset => c.value_none.to_string(),
        ObservedValue::Unknown { .. } => c.value_unknown.to_string(),
        ObservedValue::Unsupported { .. } => c.value_unsupported.to_string(),
        ObservedValue::PermissionDenied { .. } => c.value_permission_denied.to_string(),
    }
}

pub(crate) fn aggregate_label(locale: Locale, aggregate: &AggregateState) -> &'static str {
    let c = copy(locale);
    match aggregate {
        AggregateState::Default => c.state_default,
        AggregateState::NotDefault => c.state_not_default,
        AggregateState::PartiallyDefault => c.state_partially_default,
        AggregateState::ChangedExternally => c.state_changed_externally,
        AggregateState::Unavailable { .. } => c.state_unavailable,
        AggregateState::Conflict { .. } => c.state_conflict,
        AggregateState::Unknown { .. } => c.state_unknown,
        AggregateState::NeedsSignOut => c.state_needs_sign_out,
    }
}

pub(crate) fn integration_state_label(locale: Locale, state: &IntegrationState) -> &'static str {
    let c = copy(locale);
    match state {
        IntegrationState::Default => c.state_default,
        IntegrationState::NotDefault => c.state_not_default,
        IntegrationState::ChangedExternally { .. } => c.state_changed_externally,
        IntegrationState::Unavailable { .. } => c.state_unavailable,
        IntegrationState::Conflict { .. } => c.state_conflict,
        IntegrationState::Unknown { .. } => c.state_unknown,
        IntegrationState::NeedsSignOut => c.state_needs_sign_out,
    }
}

pub(crate) fn kind_label(locale: Locale, kind: IntegrationKind) -> &'static str {
    let c = copy(locale);
    match kind {
        IntegrationKind::ApplicationHandler => c.kind_application_handler,
        IntegrationKind::MimeUriHandlerGroup => c.kind_mime_group,
        IntegrationKind::DesktopLauncherEntry => c.kind_launcher_entry,
        IntegrationKind::GlobalShortcut => c.kind_global_shortcut,
        IntegrationKind::InputMethod => c.kind_input_method,
        IntegrationKind::Autostart => c.kind_autostart,
        IntegrationKind::UserService => c.kind_user_service,
        IntegrationKind::ToolEntryPoint => c.kind_tool_entry_point,
        IntegrationKind::ComponentDesktopSetting => c.kind_component_setting,
    }
}

pub(crate) fn session_effect_label(locale: Locale, effect: SessionEffect) -> &'static str {
    let c = copy(locale);
    match effect {
        SessionEffect::Immediate => c.effect_immediate,
        SessionEffect::SignOut => c.state_needs_sign_out,
        SessionEffect::Restart => c.effect_restart,
    }
}

pub(crate) fn skip_reason_label(locale: Locale, reason: &SkipReason) -> &'static str {
    let c = copy(locale);
    match reason {
        SkipReason::AlreadyDefault => c.skip_already_default,
        SkipReason::AlreadyRestored => c.skip_already_restored,
        SkipReason::NotApplicableHere => c.skip_not_applicable,
        SkipReason::PrerequisiteNotMet { .. } => c.skip_prerequisite,
        SkipReason::RequiresAdministrator => c.skip_requires_administrator,
        SkipReason::NoProductionAdapter { .. } => c.skip_no_adapter,
        SkipReason::NothingCaptured => c.skip_nothing_captured,
        SkipReason::EffectiveValueUnknown { .. } => c.skip_value_unknown,
        SkipReason::ChangedExternallyWithoutConfirmation { .. } => c.skip_changed_externally,
        SkipReason::Conflict { .. } => c.skip_conflict,
    }
}

pub(crate) fn warning_label(locale: Locale, warning: &PlanWarning) -> &'static str {
    let c = copy(locale);
    match warning {
        PlanWarning::NeedsSignOut => c.state_needs_sign_out,
        PlanWarning::NeedsRestart => c.effect_restart,
        PlanWarning::PreviousValueIndeterminate => c.previous_value_indeterminate,
        PlanWarning::OverwritesExternalChange { .. } => c.overwrites_external_change,
    }
}

/// How long ago something was read back, in words, without a date library.
pub(crate) fn relative_time(locale: Locale, moment: Option<u64>, now: u64) -> String {
    let c = copy(locale);
    let Some(moment) = moment else {
        return c.never_verified.to_string();
    };
    let elapsed = now.saturating_sub(moment);
    match elapsed {
        0..=59 => c.time_just_now.to_string(),
        60..=3599 => format!("{} {}", elapsed / 60, c.time_minutes),
        3600..=86_399 => format!("{} {}", elapsed / 3600, c.time_hours),
        _ => format!("{} {}", elapsed / 86_400, c.time_days),
    }
}

/// Where a restore entry stands, in the words the restore-all screen has to
/// keep apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestoreClass {
    Safe,
    AlreadyRestored,
    ChangedExternally,
    NothingCaptured,
    ManualAction,
    NotApplicable,
}

impl RestoreClass {
    pub(crate) fn of(entry: &PlanEntry) -> Self {
        match &entry.action {
            PlanAction::Restore { .. } => Self::Safe,
            PlanAction::Apply { .. } => Self::Safe,
            PlanAction::Skip { reason } => match reason {
                SkipReason::AlreadyRestored => Self::AlreadyRestored,
                SkipReason::ChangedExternallyWithoutConfirmation { .. } => Self::ChangedExternally,
                SkipReason::NothingCaptured => Self::NothingCaptured,
                SkipReason::NoProductionAdapter { .. } | SkipReason::RequiresAdministrator => {
                    Self::ManualAction
                }
                _ => Self::NotApplicable,
            },
        }
    }

    pub(crate) fn label(self, locale: Locale) -> &'static str {
        let c = copy(locale);
        match self {
            Self::Safe => c.restore_safe,
            Self::AlreadyRestored => c.skip_already_restored,
            Self::ChangedExternally => c.state_changed_externally,
            Self::NothingCaptured => c.skip_nothing_captured,
            Self::ManualAction => c.manual_action_required,
            Self::NotApplicable => c.skip_not_applicable,
        }
    }
}

/// What the review screen says about elevated access, before it would be asked
/// for.
///
/// `requested` is false and is not a guess. Applying and restoring go through
/// the declared adapters, none of which escalates, and an integration that
/// declares administrator scope never becomes an action at all: the planner
/// skips it, because no privileged executor for defaults is built. So the
/// honest statement is that nothing here will ask, and that these settings are
/// the ones being left alone because they would have.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ElevationNotice {
    pub(crate) requested: bool,
    pub(crate) excluded_needing_administrator: usize,
}

impl ElevationNotice {
    fn of<'a>(entries: impl Iterator<Item = &'a PlanEntry>) -> Self {
        Self {
            requested: false,
            excluded_needing_administrator: entries
                .filter(|entry| {
                    matches!(
                        entry.action,
                        PlanAction::Skip {
                            reason: SkipReason::RequiresAdministrator
                        }
                    )
                })
                .count(),
        }
    }
}

/// What the bottom of a review screen counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReviewSummary {
    pub(crate) components_selected: usize,
    pub(crate) settings_affected: usize,
    pub(crate) needs_sign_out: usize,
    pub(crate) needs_restart: usize,
    pub(crate) manual_actions: usize,
    pub(crate) awaiting_confirmation: usize,
    pub(crate) will_capture: usize,
    pub(crate) damaged_snapshots: usize,
}

/// One component's block on a review screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewComponent {
    pub(crate) component: ComponentId,
    pub(crate) name: String,
    pub(crate) selected: bool,
    /// Whether this component has anything that would change. A component with
    /// nothing to do is shown, not hidden, and cannot be selected into a run.
    pub(crate) changes: usize,
    pub(crate) entries: Vec<ReviewEntry>,
}

/// One proposed change on a review screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewEntry {
    pub(crate) component: ComponentId,
    pub(crate) integration: IntegrationId,
    pub(crate) kind: IntegrationKind,
    pub(crate) current_owner: String,
    pub(crate) new_owner: String,
    pub(crate) captured_previous: Option<String>,
    pub(crate) session_effect: SessionEffect,
    pub(crate) restorable: bool,
    pub(crate) restore_class: RestoreClass,
    pub(crate) changes_something: bool,
    pub(crate) skipped: Option<SkipReason>,
    pub(crate) warnings: Vec<PlanWarning>,
    /// Set for an entry that changed outside Better Manager. It is held back
    /// until this exact entry is confirmed.
    pub(crate) requires_confirmation: bool,
    pub(crate) confirmed: bool,
}

impl ReviewEntry {
    pub(crate) fn element_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}-{}", self.component, self.integration)
    }
}

/// A plan with a selection over it, which is what a review screen is.
#[derive(Clone, Debug)]
pub(crate) struct ReviewModel {
    kind: PlanKind,
    plan: DefaultsPlan,
    locale: Locale,
    names: BTreeMap<ComponentId, String>,
    selected: BTreeSet<ComponentId>,
    confirmed: BTreeSet<(ComponentId, IntegrationId)>,
}

impl ReviewModel {
    /// Builds the review a plan deserves. Every component with something to
    /// change starts selected, and every component in the plan is listed
    /// whether or not it has anything to do.
    pub(crate) fn new(
        locale: Locale,
        plan: DefaultsPlan,
        name_of: &dyn Fn(&ComponentId) -> String,
    ) -> Self {
        let mut names = BTreeMap::new();
        let mut selected = BTreeSet::new();
        for entry in &plan.entries {
            names
                .entry(entry.component.clone())
                .or_insert_with(|| name_of(&entry.component));
            if entry.action.changes_something() {
                selected.insert(entry.component.clone());
            }
        }
        Self {
            kind: plan.kind,
            plan,
            locale,
            names,
            selected,
            confirmed: BTreeSet::new(),
        }
    }

    /// The same review over a freshly built plan, keeping what the user chose.
    /// Confirming an externally changed entry has to re-plan, because whether
    /// it is held back is decided while the plan is built.
    pub(crate) fn rebuilt(&self, plan: DefaultsPlan) -> Self {
        let mut rebuilt = Self {
            kind: plan.kind,
            plan,
            locale: self.locale,
            names: self.names.clone(),
            selected: self.selected.clone(),
            confirmed: self.confirmed.clone(),
        };
        rebuilt
            .selected
            .retain(|component| rebuilt.names.contains_key(component));
        rebuilt
    }

    pub(crate) fn kind(&self) -> PlanKind {
        self.kind
    }

    /// The entries the user agreed to replace despite an external change.
    /// Planning has to be redone with these, because whether an entry is held
    /// back is decided while the plan is built.
    pub(crate) fn confirmed_entries(&self) -> Vec<(ComponentId, IntegrationId)> {
        self.confirmed.iter().cloned().collect()
    }

    pub(crate) fn is_selected(&self, component: &ComponentId) -> bool {
        self.selected.contains(component)
    }

    pub(crate) fn toggle(&mut self, component: &ComponentId) {
        if !self.selected.remove(component) {
            self.selected.insert(component.clone());
        }
    }

    pub(crate) fn is_confirmed(
        &self,
        component: &ComponentId,
        integration: &IntegrationId,
    ) -> bool {
        self.confirmed
            .contains(&(component.clone(), integration.clone()))
    }

    pub(crate) fn toggle_confirmation(
        &mut self,
        component: &ComponentId,
        integration: &IntegrationId,
    ) {
        let key = (component.clone(), integration.clone());
        if !self.confirmed.remove(&key) {
            self.confirmed.insert(key);
        }
    }

    pub(crate) fn components(&self) -> Vec<ReviewComponent> {
        let mut components: Vec<ReviewComponent> = Vec::new();
        for entry in &self.plan.entries {
            let review = self.entry(entry);
            match components
                .iter_mut()
                .find(|component| component.component == entry.component)
            {
                Some(component) => {
                    component.changes += usize::from(review.changes_something);
                    component.entries.push(review);
                }
                None => components.push(ReviewComponent {
                    component: entry.component.clone(),
                    name: self.name(&entry.component),
                    selected: self.is_selected(&entry.component),
                    changes: usize::from(review.changes_something),
                    entries: vec![review],
                }),
            }
        }
        components
    }

    fn name(&self, component: &ComponentId) -> String {
        self.names
            .get(component)
            .cloned()
            .unwrap_or_else(|| component.to_string())
    }

    fn entry(&self, entry: &PlanEntry) -> ReviewEntry {
        let new_owner = match &entry.action {
            PlanAction::Apply { to } => value_label(self.locale, to),
            PlanAction::Restore { to } => observed_label(self.locale, to),
            PlanAction::Skip { .. } => match self.kind {
                PlanKind::Apply => value_label(self.locale, &entry.desired),
                PlanKind::Restore => entry
                    .captured_previous
                    .as_ref()
                    .map(|value| observed_label(self.locale, value))
                    .unwrap_or_else(|| copy(self.locale).none.to_string()),
            },
        };
        ReviewEntry {
            component: entry.component.clone(),
            integration: entry.integration.clone(),
            kind: entry.kind,
            current_owner: observed_label(self.locale, &entry.current),
            new_owner,
            captured_previous: entry
                .captured_previous
                .as_ref()
                .map(|value| observed_label(self.locale, value)),
            session_effect: entry.session_effect,
            restorable: entry
                .captured_previous
                .as_ref()
                .is_some_and(ObservedValue::is_determinate),
            restore_class: RestoreClass::of(entry),
            changes_something: entry.action.changes_something(),
            skipped: match &entry.action {
                PlanAction::Skip { reason } => Some(reason.clone()),
                _ => None,
            },
            warnings: entry.warnings.clone(),
            requires_confirmation: entry.requires_confirmation,
            confirmed: entry.confirmed,
        }
    }

    /// Only the entries a selected component owns.
    fn selected_entries(&self) -> impl Iterator<Item = &PlanEntry> {
        self.plan
            .entries
            .iter()
            .filter(|entry| self.selected.contains(&entry.component))
    }

    pub(crate) fn summary(&self) -> ReviewSummary {
        let mut summary = ReviewSummary {
            components_selected: self.selected.len(),
            damaged_snapshots: self.plan.damaged_snapshots.len(),
            ..ReviewSummary::default()
        };
        for entry in self.selected_entries() {
            match &entry.action {
                PlanAction::Skip { reason } => match reason {
                    SkipReason::NoProductionAdapter { .. } | SkipReason::RequiresAdministrator => {
                        summary.manual_actions += 1
                    }
                    SkipReason::ChangedExternallyWithoutConfirmation { .. } => {
                        summary.awaiting_confirmation += 1
                    }
                    _ => {}
                },
                _ => {
                    summary.settings_affected += 1;
                    match entry.session_effect {
                        SessionEffect::SignOut => summary.needs_sign_out += 1,
                        SessionEffect::Restart => summary.needs_restart += 1,
                        SessionEffect::Immediate => {}
                    }
                    if entry.captured_previous.is_none() {
                        summary.will_capture += 1;
                    }
                }
            }
        }
        summary
    }

    pub(crate) fn elevation(&self) -> ElevationNotice {
        ElevationNotice::of(self.selected_entries())
    }

    /// The plan the user agreed to, or nothing when they deselected everything.
    ///
    /// This is the only way to build an [`ApprovedPlan`], and an approved plan
    /// is the only thing the execution path takes.
    pub(crate) fn approve(&self) -> Option<ApprovedPlan> {
        let entries: Vec<PlanEntry> = self.selected_entries().cloned().collect();
        if !entries.iter().any(|entry| entry.action.changes_something()) {
            return None;
        }
        Some(ApprovedPlan {
            plan: DefaultsPlan::new(self.kind, entries, self.plan.damaged_snapshots.clone()),
        })
    }
}

/// A plan a review screen produced and a person confirmed.
///
/// The field is private and there is no other constructor, so a plan cannot
/// reach the execution path without the review that produced it.
#[derive(Clone, Debug)]
pub(crate) struct ApprovedPlan {
    plan: DefaultsPlan,
}

impl ApprovedPlan {
    pub(crate) fn plan(&self) -> &DefaultsPlan {
        &self.plan
    }
}

/// How honest a per-entry result is allowed to look.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResultTone {
    Success,
    Pending,
    Warning,
    Failure,
    Neutral,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResultRow {
    pub(crate) component: ComponentId,
    pub(crate) name: String,
    pub(crate) integration: IntegrationId,
    pub(crate) label: &'static str,
    pub(crate) detail: Option<String>,
    pub(crate) tone: ResultTone,
}

/// What each entry actually did. A write that could not be confirmed is its own
/// outcome here, exactly as the engine reported it.
pub(crate) fn result_rows(
    locale: Locale,
    outcome: &DefaultsOutcome,
    name_of: &dyn Fn(&ComponentId) -> String,
) -> Vec<ResultRow> {
    let c = copy(locale);
    outcome
        .results
        .iter()
        .map(|result| {
            let (label, tone, detail) = match &result.outcome {
                EntryOutcome::Applied { value } => (
                    c.result_applied,
                    ResultTone::Success,
                    Some(value_label(locale, value)),
                ),
                EntryOutcome::AppliedNeedsSignOut { value } => (
                    c.state_needs_sign_out,
                    ResultTone::Pending,
                    Some(value_label(locale, value)),
                ),
                EntryOutcome::Restored { value } => (
                    c.result_restored,
                    ResultTone::Success,
                    Some(observed_label(locale, value)),
                ),
                EntryOutcome::AlreadyCorrect => {
                    (c.result_already_correct, ResultTone::Success, None)
                }
                EntryOutcome::NotVerified { observed } => (
                    c.result_not_verified,
                    ResultTone::Failure,
                    Some(observed_label(locale, observed)),
                ),
                EntryOutcome::VerificationInconclusive { observed } => (
                    c.result_inconclusive,
                    ResultTone::Warning,
                    Some(observed_label(locale, observed)),
                ),
                EntryOutcome::Skipped { reason } => (
                    c.result_skipped,
                    ResultTone::Neutral,
                    Some(skip_reason_label(locale, reason).to_string()),
                ),
                EntryOutcome::ManualActionRequired { .. } => {
                    (c.manual_action_required, ResultTone::Warning, None)
                }
                EntryOutcome::Failed { .. } => (c.result_failed, ResultTone::Failure, None),
            };
            ResultRow {
                component: result.component.clone(),
                name: name_of(&result.component),
                integration: result.integration.clone(),
                label,
                detail,
                tone,
            }
        })
        .collect()
}

/// The one line above the per-entry results.
pub(crate) fn outcome_headline(locale: Locale, outcome: &DefaultsOutcome) -> &'static str {
    let c = copy(locale);
    if outcome.has_failures() {
        if outcome.succeeded() > 0 {
            c.result_partial
        } else {
            c.result_failed
        }
    } else if outcome.kind == PlanKind::Restore {
        c.result_restored
    } else {
        c.result_applied
    }
}
