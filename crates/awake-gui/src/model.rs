//! What the window shows, decided without a display server.
//!
//! Everything on screen is derived here first, the way `monitor-views` and
//! `manager-gui::model` do it: a document from the service goes in, a plain
//! Rust value comes out, and the GPUI layer only turns that value into
//! elements. That separation is what makes the acceptance criteria testable —
//! "an unavailable provider renders its explanation and not an enabled control"
//! is a property of `ConditionView`, not of a pixel.

use awake_core::{
    Combine, Condition, ProviderKind, RESOLUTION_EARLIEST_BATTERY_STOP, RESOLUTION_STRONGEST_WINS,
    Rule, SessionOrigin, SessionPolicy,
};
use awake_ipc::{
    RuleTestDocument, StatusDocument, WireBatteryProtection, WireConflict, WireHistoryEntry,
    WireIndicator, WireProvider, WireRemaining, WireSuppression, WireTruth,
};

use crate::i18n::{Copy, fill};
use crate::localtime::{UtcOffset, calendar_time, clock_time};

/// The eight sections ticket 26 names. Every one of them is a sidebar entry and
/// a keyboard shortcut; there is no section reachable only by clicking through
/// another one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Section {
    Status,
    QuickSessions,
    Rules,
    SessionDefaults,
    Battery,
    History,
    Diagnostics,
    Settings,
}

impl Section {
    pub(crate) const ALL: [Section; 8] = [
        Section::Status,
        Section::QuickSessions,
        Section::Rules,
        Section::SessionDefaults,
        Section::Battery,
        Section::History,
        Section::Diagnostics,
        Section::Settings,
    ];

    /// A stable key, used for element ids and for the shortcut binding names.
    ///
    /// Test-only: the rendering names its elements from `Section` directly, and
    /// this exists so a test can assert the eight keys are distinct and stable
    /// without reaching into the view.
    #[cfg(test)]
    pub(crate) fn as_key(self) -> &'static str {
        match self {
            Section::Status => "status",
            Section::QuickSessions => "quick-sessions",
            Section::Rules => "automatic-rules",
            Section::SessionDefaults => "session-defaults",
            Section::Battery => "battery-safety",
            Section::History => "history",
            Section::Diagnostics => "diagnostics",
            Section::Settings => "settings",
        }
    }

    pub(crate) fn title(self, c: &'static Copy) -> &'static str {
        match self {
            Section::Status => c.status,
            Section::QuickSessions => c.quick_sessions,
            Section::Rules => c.automatic_rules,
            Section::SessionDefaults => c.session_defaults,
            Section::Battery => c.battery_safety,
            Section::History => c.history,
            Section::Diagnostics => c.diagnostics,
            Section::Settings => c.settings,
        }
    }

    pub(crate) fn subtitle(self, c: &'static Copy) -> &'static str {
        match self {
            Section::Status => c.status_subtitle,
            Section::QuickSessions => c.quick_sessions_subtitle,
            Section::Rules => c.automatic_rules_subtitle,
            Section::SessionDefaults => c.session_defaults_subtitle,
            Section::Battery => c.battery_safety_subtitle,
            Section::History => c.history_subtitle,
            Section::Diagnostics => c.diagnostics_subtitle,
            Section::Settings => c.settings_subtitle,
        }
    }

    /// The keybinding that reaches this section from anywhere, so the window is
    /// navigable without a pointing device.
    pub(crate) fn shortcut(self) -> &'static str {
        match self {
            Section::Status => "ctrl-1",
            Section::QuickSessions => "ctrl-2",
            Section::Rules => "ctrl-3",
            Section::SessionDefaults => "ctrl-4",
            Section::Battery => "ctrl-5",
            Section::History => "ctrl-6",
            Section::Diagnostics => "ctrl-7",
            Section::Settings => "ctrl-8",
        }
    }

    /// Which section a shortcut index reaches. One-based, matching the label.
    ///
    /// Test-only: it is the written-down statement that all eight sections are
    /// reachable by number, which is what "reachable by keyboard alone" means
    /// for the sidebar.
    #[cfg(test)]
    pub(crate) fn at_index(index: usize) -> Option<Section> {
        Section::ALL.get(index).copied()
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// One line of "why is this machine awake". A rule-started session names its
/// rule, because "Video call" tells someone more than session 7 does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReasonRow {
    pub(crate) session_id: u64,
    pub(crate) origin: SessionOrigin,
    pub(crate) reason: String,
    pub(crate) rule_id: Option<u64>,
    pub(crate) rule_name: Option<String>,
    /// The moment this session started, absent when the status carries a reason
    /// whose session it did not also report.
    pub(crate) started_at_unix_seconds: Option<u64>,
    pub(crate) remaining: Option<WireRemaining>,
}

impl ReasonRow {
    pub(crate) fn origin_label(&self, c: &'static Copy) -> &'static str {
        match self.origin {
            SessionOrigin::Manual => c.manual_origin,
            SessionOrigin::Trigger => c.rule_origin,
        }
    }

    /// What the reason is called on screen: the rule's own name when a rule
    /// started it, the typed reason otherwise.
    pub(crate) fn display_name(&self) -> &str {
        self.rule_name.as_deref().unwrap_or(&self.reason)
    }
}

/// One field of the effective policy, and whether the backend actually delivers
/// it. A policy asked for but not delivered must never read as in force.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PolicyRow {
    pub(crate) field: PolicyRowField,
    pub(crate) prevented: bool,
    pub(crate) delivered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PolicyRowField {
    SystemSuspend,
    Idle,
    DisplaySleep,
    AutomaticLock,
}

impl PolicyRowField {
    pub(crate) fn label(self, c: &'static Copy) -> &'static str {
        match self {
            PolicyRowField::SystemSuspend => c.system_sleep,
            PolicyRowField::Idle => c.idle_handling,
            PolicyRowField::DisplaySleep => c.display_sleep,
            PolicyRowField::AutomaticLock => c.automatic_lock,
        }
    }
}

impl PolicyRow {
    pub(crate) fn value(self, c: &'static Copy) -> &'static str {
        match (self.prevented, self.delivered) {
            (false, _) => c.allowed,
            (true, true) => c.prevented,
            (true, false) => c.not_delivered,
        }
    }
}

/// How the conflicting rules were settled, named so a person can see which rule
/// decided the policy they are looking at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConflictView {
    pub(crate) field_key: String,
    pub(crate) winner_rule_id: u64,
    pub(crate) winner_name: String,
    pub(crate) overridden_rule_ids: Vec<u64>,
    pub(crate) resolution_key: String,
}

impl ConflictView {
    pub(crate) fn from_wire(conflict: &WireConflict) -> Self {
        Self {
            field_key: conflict.field.clone(),
            winner_rule_id: conflict.winner_rule_id,
            winner_name: conflict.winner_name.clone(),
            overridden_rule_ids: conflict.overridden_rule_ids.clone(),
            resolution_key: conflict.resolution_key.clone(),
        }
    }

    pub(crate) fn field_label(&self, c: &'static Copy) -> &'static str {
        match self.field_key.as_str() {
            "prevent_system_suspend" => c.field_system_suspend,
            "prevent_idle" => c.field_idle,
            "prevent_display_sleep" => c.field_display_sleep,
            "prevent_automatic_lock" => c.field_automatic_lock,
            "battery_stop_percent" => c.field_battery_threshold,
            // A field this build has never heard of is named by its own key
            // rather than dropped, so a newer service is still explainable.
            _ => c.unknown,
        }
    }

    pub(crate) fn resolution_label(&self, c: &'static Copy) -> &'static str {
        match self.resolution_key.as_str() {
            RESOLUTION_STRONGEST_WINS => c.resolution_strongest_wins,
            RESOLUTION_EARLIEST_BATTERY_STOP => c.resolution_earliest_battery_stop,
            _ => c.unknown,
        }
    }

    /// The sentence the Status section shows. It names the winning rule, which
    /// is the whole point: a merged policy nobody asked for looks like a bug.
    pub(crate) fn explanation(&self, c: &'static Copy) -> String {
        let sentence = fill(c.conflict_explanation, "field", self.field_label(c));
        fill(&sentence, "winner", &self.winner_name)
    }

    pub(crate) fn overridden_note(&self, c: &'static Copy) -> Option<String> {
        if self.overridden_rule_ids.is_empty() {
            return None;
        }
        Some(fill(
            c.conflict_overrode,
            "count",
            &self.overridden_rule_ids.len().to_string(),
        ))
    }
}

/// Battery protection, which on a machine with no battery is a statement rather
/// than a control. A threshold spinner on a desktop would be an inert widget
/// that promises something that can never happen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BatteryView {
    NotApplicable,
    Present {
        percent: Option<u8>,
        on_ac_power: Option<bool>,
        stop_below_percent: Option<u8>,
    },
}

impl BatteryView {
    pub(crate) fn from_wire(protection: &WireBatteryProtection) -> Self {
        if !protection.has_battery {
            return BatteryView::NotApplicable;
        }
        BatteryView::Present {
            percent: protection.percent,
            on_ac_power: protection.on_ac_power,
            stop_below_percent: protection.stop_below_percent,
        }
    }

    /// Whether a threshold control may be drawn at all.
    pub(crate) fn offers_threshold(&self) -> bool {
        matches!(self, BatteryView::Present { .. })
    }

    /// Test-only: the Battery section renders through [`BatteryView::summary`],
    /// and this exists so a test can assert the threshold a machine with no
    /// battery reports is absent rather than zero.
    #[cfg(test)]
    pub(crate) fn threshold_percent(&self) -> Option<u8> {
        match self {
            BatteryView::NotApplicable => None,
            BatteryView::Present {
                stop_below_percent, ..
            } => *stop_below_percent,
        }
    }

    pub(crate) fn summary(&self, c: &'static Copy) -> String {
        match self {
            BatteryView::NotApplicable => c.not_applicable.to_string(),
            BatteryView::Present {
                stop_below_percent: Some(percent),
                ..
            } => fill(c.battery_stops_at, "percent", &percent.to_string()),
            BatteryView::Present { .. } => c.battery_stop_off.to_string(),
        }
    }
}

/// How the inhibitor backend is doing, and what it cannot deliver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BackendView {
    pub(crate) name: String,
    pub(crate) available: bool,
    pub(crate) detail: Option<String>,
    pub(crate) can_hold_system_suspend: bool,
    pub(crate) can_hold_idle: bool,
    pub(crate) can_hold_display_sleep: bool,
    pub(crate) can_hold_automatic_lock: bool,
}

impl BackendView {
    pub(crate) fn availability_label(&self, c: &'static Copy) -> &'static str {
        if self.available {
            c.available
        } else {
            c.unavailable
        }
    }
}

/// Everything the Status section shows, decided once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatusView {
    pub(crate) indicator: WireIndicator,
    pub(crate) reasons: Vec<ReasonRow>,
    pub(crate) policy: Vec<PolicyRow>,
    pub(crate) backend: BackendView,
    pub(crate) battery: BatteryView,
    pub(crate) conflicts: Vec<ConflictView>,
    pub(crate) attention: Option<String>,
    pub(crate) interrupted_previous_session: Option<String>,
    pub(crate) suppression: Option<WireSuppression>,
    pub(crate) rules_total: u32,
    pub(crate) rules_enabled: u32,
    pub(crate) rules_refused: u32,
    pub(crate) now_unix_seconds: u64,
}

impl StatusView {
    pub(crate) fn from_status(status: &StatusDocument) -> Self {
        let capabilities = status.backend.capabilities;
        let policy = status.effective_policy;
        let reasons = status
            .reasons
            .iter()
            .map(|reason| {
                let session = status
                    .sessions
                    .iter()
                    .find(|session| session.session_id == reason.session_id);
                let rule = status
                    .active_rules
                    .iter()
                    .find(|rule| rule.session_id == reason.session_id);
                ReasonRow {
                    session_id: reason.session_id,
                    origin: reason.origin,
                    reason: reason.reason.clone(),
                    rule_id: rule.map(|rule| rule.rule_id),
                    rule_name: rule.map(|rule| rule.name.clone()),
                    started_at_unix_seconds: session.map(|s| s.started_at_unix_seconds),
                    remaining: session.map(|s| s.remaining),
                }
            })
            .collect();

        Self {
            indicator: status.indicator,
            reasons,
            policy: vec![
                PolicyRow {
                    field: PolicyRowField::SystemSuspend,
                    prevented: policy.prevent_system_suspend,
                    delivered: capabilities.system_suspend,
                },
                PolicyRow {
                    field: PolicyRowField::Idle,
                    prevented: policy.prevent_idle,
                    delivered: capabilities.idle,
                },
                PolicyRow {
                    field: PolicyRowField::DisplaySleep,
                    prevented: policy.prevent_display_sleep,
                    delivered: capabilities.display_sleep,
                },
                PolicyRow {
                    field: PolicyRowField::AutomaticLock,
                    prevented: policy.prevent_automatic_lock,
                    delivered: capabilities.automatic_lock,
                },
            ],
            backend: BackendView {
                name: status.backend.name.clone(),
                available: status.backend.available,
                detail: status.backend.detail.clone(),
                can_hold_system_suspend: capabilities.system_suspend,
                can_hold_idle: capabilities.idle,
                can_hold_display_sleep: capabilities.display_sleep,
                can_hold_automatic_lock: capabilities.automatic_lock,
            },
            battery: BatteryView::from_wire(&status.battery_protection),
            conflicts: status
                .conflicts
                .iter()
                .map(ConflictView::from_wire)
                .collect(),
            attention: status.attention.clone(),
            interrupted_previous_session: status
                .interrupted_previous_session
                .as_ref()
                .map(|interrupted| interrupted.reason.clone()),
            suppression: status.rules_suppression,
            rules_total: status.rule_summary.total,
            rules_enabled: status.rule_summary.enabled,
            rules_refused: status.rule_summary.refused,
            now_unix_seconds: status.now_unix_seconds,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !self.reasons.is_empty()
    }

    pub(crate) fn summary(&self, c: &'static Copy) -> &'static str {
        match self.indicator {
            WireIndicator::Unavailable => c.service_unreachable,
            WireIndicator::AttentionRequired => c.attention_summary,
            WireIndicator::PausedRules if !self.is_active() => c.paused_summary,
            _ if self.is_active() => c.active_summary,
            _ => c.inactive_summary,
        }
    }

    /// The manual session's id, which is the only one End session may aim at.
    ///
    /// Test-only. The End session action sends `EndManualSession`, which names
    /// no id at all precisely so it cannot aim at the wrong one; this is here so
    /// a test can assert the view still knows which session is the manual one.
    #[cfg(test)]
    pub(crate) fn manual_session_id(&self) -> Option<u64> {
        self.reasons
            .iter()
            .find(|reason| reason.origin == SessionOrigin::Manual)
            .map(|reason| reason.session_id)
    }

    /// The reasons still keeping the machine awake once `session_id` is ended.
    ///
    /// This is what the End action explains before it is pressed. A machine
    /// held awake by two things does not go to sleep because one of them was
    /// ended, and a screen that implies otherwise has told a lie.
    pub(crate) fn reasons_after_ending(&self, session_id: u64) -> Vec<&ReasonRow> {
        self.reasons
            .iter()
            .filter(|reason| reason.session_id != session_id)
            .collect()
    }

    pub(crate) fn ending_explanation(&self, session_id: u64, c: &'static Copy) -> String {
        let remaining = self.reasons_after_ending(session_id);
        if remaining.is_empty() {
            return c.ending_leaves_nothing.to_string();
        }
        let names = remaining
            .iter()
            .map(|reason| reason.display_name().to_string())
            .collect::<Vec<_>>()
            .join("、");
        let sentence = fill(c.ending_leaves, "count", &remaining.len().to_string());
        fill(&sentence, "reasons", &names)
    }
}

// ---------------------------------------------------------------------------
// Conditions and rules
// ---------------------------------------------------------------------------

/// Whether a condition can be edited here at all.
///
/// The unavailable case carries the service's own explanation. It is a separate
/// variant rather than a disabled flag so a renderer cannot accidentally draw
/// an enabled-looking control for it: there is no editable payload to draw.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConditionControl {
    Editable,
    Unavailable { explanation: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConditionView {
    pub(crate) provider: ProviderKind,
    pub(crate) summary: String,
    pub(crate) control: ConditionControl,
}

impl ConditionView {
    /// Presents one condition against what the providers actually report.
    ///
    /// A provider this status never mentioned is treated as available: the
    /// service reports the providers it knows about, and refusing to edit a
    /// condition because a report was silent would be a guess.
    pub(crate) fn present(
        condition: &Condition,
        providers: &[ProviderRow],
        c: &'static Copy,
    ) -> Self {
        let provider = condition.provider();
        let reported = providers.iter().find(|entry| entry.kind == provider);
        let control = match reported {
            Some(entry) if !entry.available => ConditionControl::Unavailable {
                explanation: entry
                    .explanation
                    .clone()
                    // A provider reported unavailable without a reason still
                    // gets an explanation rather than an inert control.
                    .unwrap_or_else(|| c.unknown.to_string()),
            },
            _ => ConditionControl::Editable,
        };
        Self {
            provider,
            summary: condition_summary(condition, c),
            control,
        }
    }

    /// Whether this condition may be drawn as an editable control. The rules
    /// editor asks this rather than inspecting the variant, so the one answer
    /// is in one place.
    pub(crate) fn is_editable(&self) -> bool {
        matches!(self.control, ConditionControl::Editable)
    }

    pub(crate) fn explanation(&self) -> Option<&str> {
        match &self.control {
            ConditionControl::Editable => None,
            ConditionControl::Unavailable { explanation } => Some(explanation),
        }
    }
}

/// One AND/OR bracket, presented.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GroupView {
    pub(crate) combine: Combine,
    pub(crate) conditions: Vec<ConditionView>,
}

impl GroupView {
    pub(crate) fn combine_label(combine: Combine, c: &'static Copy) -> &'static str {
        match combine {
            Combine::All => c.match_all,
            Combine::Any => c.match_any,
        }
    }
}

/// One row of the rules list, plus everything its editor needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuleView {
    pub(crate) rule_id: u64,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) priority: u8,
    pub(crate) combine: Combine,
    pub(crate) groups: Vec<GroupView>,
    pub(crate) policy: SessionPolicy,
    pub(crate) battery_stop_percent: Option<u8>,
    pub(crate) matching_now: bool,
    /// Set when at least one condition names a provider that cannot be read.
    pub(crate) has_unavailable_condition: bool,
}

impl RuleView {
    pub(crate) fn present(
        rule: &Rule,
        matching_rule_ids: &[u64],
        providers: &[ProviderRow],
        c: &'static Copy,
    ) -> Self {
        let groups: Vec<GroupView> = rule
            .groups
            .iter()
            .map(|group| GroupView {
                combine: group.combine,
                conditions: group
                    .conditions
                    .iter()
                    .map(|condition| ConditionView::present(condition, providers, c))
                    .collect(),
            })
            .collect();
        let has_unavailable_condition = groups
            .iter()
            .flat_map(|group| group.conditions.iter())
            .any(|condition| !condition.is_editable());
        Self {
            rule_id: rule.id.0,
            name: rule.name.as_str().to_string(),
            enabled: rule.enabled,
            priority: rule.priority,
            combine: rule.combine,
            groups,
            policy: rule.policy,
            battery_stop_percent: rule.battery_stop_percent,
            matching_now: matching_rule_ids.contains(&rule.id.0),
            has_unavailable_condition,
        }
    }

    pub(crate) fn state_label(&self, c: &'static Copy) -> &'static str {
        if self.enabled { c.enabled } else { c.disabled }
    }

    pub(crate) fn matching_label(&self, c: &'static Copy) -> &'static str {
        if self.matching_now {
            c.rule_matching_now
        } else {
            c.rule_not_matching
        }
    }
}

/// What testing one rule found, in words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuleTestView {
    pub(crate) rule_id: u64,
    pub(crate) truth: WireTruth,
    pub(crate) group_truths: Vec<WireTruth>,
    pub(crate) unavailable: Vec<ProviderRow>,
    pub(crate) would_be_active: bool,
    pub(crate) suppression: Option<WireSuppression>,
    pub(crate) rule_disabled: bool,
}

impl RuleTestView {
    pub(crate) fn from_document(document: &RuleTestDocument) -> Self {
        Self {
            rule_id: document.rule_id,
            truth: document.truth,
            group_truths: document.group_truths.clone(),
            unavailable: document
                .unavailable_providers
                .iter()
                .map(ProviderRow::from_wire)
                .collect(),
            would_be_active: document.would_be_active,
            suppression: document.suppression,
            rule_disabled: document.rule_disabled,
        }
    }

    pub(crate) fn truth_label(truth: WireTruth, c: &'static Copy) -> &'static str {
        match truth {
            WireTruth::True => c.test_true,
            WireTruth::False => c.test_false,
            WireTruth::Unknown => c.test_unknown,
        }
    }

    pub(crate) fn outcome_label(&self, c: &'static Copy) -> &'static str {
        if self.would_be_active {
            c.would_be_active
        } else {
            c.would_not_be_active
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics and history
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProviderRow {
    pub(crate) kind: ProviderKind,
    pub(crate) available: bool,
    pub(crate) poll_seconds: Option<u64>,
    pub(crate) explanation: Option<String>,
}

impl ProviderRow {
    pub(crate) fn from_wire(provider: &WireProvider) -> Self {
        Self {
            kind: provider.kind,
            available: provider.available,
            poll_seconds: provider.poll_seconds,
            explanation: provider.explanation.clone(),
        }
    }

    pub(crate) fn availability_label(&self, c: &'static Copy) -> &'static str {
        if self.available {
            c.available
        } else {
            c.unavailable
        }
    }

    pub(crate) fn cadence_label(&self, c: &'static Copy) -> String {
        match self.poll_seconds {
            Some(seconds) => fill(c.poll_every_seconds, "seconds", &seconds.to_string()),
            None => c.no_polling.to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistoryRow {
    pub(crate) session_id: u64,
    pub(crate) started_at_unix_seconds: u64,
    pub(crate) ended_at_unix_seconds: Option<u64>,
    pub(crate) origin: SessionOrigin,
    pub(crate) rule_id: Option<u64>,
    pub(crate) reasons: Vec<String>,
    pub(crate) policy: SessionPolicy,
    pub(crate) battery_stop_percent: Option<u8>,
    pub(crate) end_cause: Option<String>,
    pub(crate) backend_failure: Option<String>,
}

impl HistoryRow {
    pub(crate) fn from_wire(entry: &WireHistoryEntry) -> Self {
        Self {
            session_id: entry.session_id,
            started_at_unix_seconds: entry.started_at_unix_seconds,
            ended_at_unix_seconds: entry.ended_at_unix_seconds,
            origin: entry.origin,
            rule_id: entry.rule_id,
            reasons: entry.reasons.clone(),
            policy: entry.effective_policy,
            battery_stop_percent: entry.battery_stop_percent,
            end_cause: entry.end_cause.clone(),
            backend_failure: entry.backend_failure.clone(),
        }
    }

    pub(crate) fn origin_label(&self, c: &'static Copy) -> &'static str {
        match self.origin {
            SessionOrigin::Manual => c.manual_origin,
            SessionOrigin::Trigger => c.rule_origin,
        }
    }

    pub(crate) fn started_label(&self, offset: UtcOffset) -> String {
        calendar_time(self.started_at_unix_seconds, offset)
    }

    pub(crate) fn ended_label(&self, offset: UtcOffset, c: &'static Copy) -> String {
        match self.ended_at_unix_seconds {
            Some(ended) => calendar_time(ended, offset),
            None => c.history_still_running.to_string(),
        }
    }

    pub(crate) fn cause_label(&self, c: &'static Copy) -> &'static str {
        match self.end_cause.as_deref() {
            Some("user_request") => c.cause_user_request,
            Some("expired") => c.cause_expired,
            Some("battery_threshold") => c.cause_battery_threshold,
            Some("backend_failure") => c.cause_backend_failure,
            Some("service_shutdown") => c.cause_service_shutdown,
            Some("replaced") => c.cause_replaced,
            Some("trigger_cleared") => c.cause_trigger_cleared,
            Some("rules_suppressed") => c.cause_rules_suppressed,
            // A cause a newer service invented is named as such rather than
            // shown as "unknown", which would read like a failed read.
            Some(_) => c.cause_unrecognized,
            None => c.history_still_running,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared wording helpers
// ---------------------------------------------------------------------------

pub(crate) fn provider_label(provider: ProviderKind, c: &'static Copy) -> &'static str {
    match provider {
        ProviderKind::ProcessRunning => c.provider_process_running,
        ProviderKind::AcPower => c.provider_ac_power,
        ProviderKind::BatteryPercent => c.provider_battery_percent,
        ProviderKind::ExternalDisplay => c.provider_external_display,
        ProviderKind::AudioPlayback => c.provider_audio_playback,
        ProviderKind::CpuUtilization => c.provider_cpu_utilization,
        ProviderKind::NetworkThroughput => c.provider_network_throughput,
        ProviderKind::NetworkInterface => c.provider_network_interface,
        ProviderKind::TimeSchedule => c.provider_time_schedule,
        ProviderKind::WatchedPath => c.provider_watched_path,
        ProviderKind::Fullscreen => c.provider_fullscreen,
    }
}

/// One condition in a sentence. Nothing here formats a command; every operand
/// is a validated value out of `awake-core`.
pub(crate) fn condition_summary(condition: &Condition, c: &'static Copy) -> String {
    match condition {
        Condition::ProcessRunning { matcher } => {
            fill(c.condition_process_running, "value", matcher.as_str())
        }
        Condition::AcPower { connected: true } => c.condition_ac_connected.to_string(),
        Condition::AcPower { connected: false } => c.condition_ac_disconnected.to_string(),
        Condition::BatteryPercent { at_least, at_most } => {
            let sentence = fill(c.condition_battery_between, "low", &at_least.to_string());
            fill(&sentence, "high", &at_most.to_string())
        }
        Condition::ExternalDisplay { connected: true } => {
            c.condition_external_display_connected.to_string()
        }
        Condition::ExternalDisplay { connected: false } => {
            c.condition_external_display_disconnected.to_string()
        }
        Condition::AudioPlayback { playing: true } => c.condition_audio_playing.to_string(),
        Condition::AudioPlayback { playing: false } => c.condition_audio_silent.to_string(),
        Condition::CpuUtilizationAtLeast { percent } => {
            fill(c.condition_cpu_at_least, "percent", &percent.to_string())
        }
        Condition::NetworkThroughputAtLeast {
            kibibytes_per_second,
        } => fill(
            c.condition_network_at_least,
            "rate",
            &kibibytes_per_second.to_string(),
        ),
        Condition::NetworkInterfaceUp { interface } => {
            fill(c.condition_interface_up, "value", interface.as_str())
        }
        Condition::TimeSchedule { .. } => c.condition_schedule.to_string(),
        Condition::WatchedPathActive {
            path,
            within_seconds,
        } => {
            let sentence = fill(
                c.condition_watched_path,
                "value",
                &path.as_path().display().to_string(),
            );
            fill(&sentence, "seconds", &within_seconds.to_string())
        }
        Condition::Fullscreen { active: true } => c.condition_fullscreen_active.to_string(),
        Condition::Fullscreen { active: false } => c.condition_fullscreen_inactive.to_string(),
    }
}

pub(crate) fn duration_label(seconds: u64, c: &'static Copy) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    match (hours, minutes) {
        (0, 0) => format!("{} {}", seconds % 60, c.second_unit),
        (0, minutes) => format!("{minutes} {}", c.minute_unit),
        (hours, 0) => format!("{hours} {}", c.hour_unit),
        (hours, minutes) => format!("{hours} {} {minutes} {}", c.hour_unit, c.minute_unit),
    }
}

pub(crate) fn remaining_label(remaining: WireRemaining, c: &'static Copy) -> String {
    match remaining {
        WireRemaining::UntilEnded => c.until_ended.to_string(),
        WireRemaining::Seconds { seconds } => duration_label(seconds, c),
        WireRemaining::Elapsed => c.elapsed.to_string(),
    }
}

pub(crate) fn started_label(unix_seconds: u64, offset: UtcOffset) -> String {
    clock_time(unix_seconds, offset)
}

pub(crate) fn suppression_label(suppression: WireSuppression, c: &'static Copy) -> &'static str {
    match suppression {
        WireSuppression::PausedUntil { .. } => c.rules_paused_until,
        WireSuppression::PausedUntilResumed => c.rules_paused_until_resumed,
        WireSuppression::Overridden => c.rules_overridden,
    }
}
