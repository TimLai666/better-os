//! The window's state, and every change it is allowed to make.
//!
//! Two rules hold everywhere in this file. The window never holds an inhibitor
//! and never runs a command: every mutation is one `awake-ipc` request, and the
//! service's reply is what the window then shows. And nothing is decided here
//! that could be decided in `model.rs`: this type holds documents and drafts,
//! and the presented values come from the view model.

use awake_core::{
    Combine, Condition, ConditionGroup, DEFAULT_PRIORITY, InterfaceName, ProcessMatchKind,
    ProcessMatcher, Reason, Rule, RuleId, Schedule, SessionPolicy, WatchedPath, Weekday,
};
use awake_ipc::{MAX_HISTORY_PAGE, RequestBody, WireEnd, WireSuppression};
use gpui::*;
use gpui_component::input::InputState;
use gpui_component::{Theme, ThemeMode};

use crate::client::{ClientError, ServiceClient, Snapshot};
use crate::i18n::{Locale, copy};
use crate::localtime::UtcOffset;
use crate::model::{HistoryRow, ProviderRow, RuleTestView, RuleView, Section, StatusView};
use crate::settings::{Preferences, PreferencesStore, PresetLength, StoredTheme};

/// How many recorded sessions the History section asks for. The protocol caps a
/// page at [`MAX_HISTORY_PAGE`]; asking for the cap means the retention note is
/// the only reason a session is missing.
pub(crate) const HISTORY_PAGE: u32 = MAX_HISTORY_PAGE;

/// One condition being edited, with the text field it needs when its operand is
/// a string rather than a number or a switch.
pub(crate) struct DraftCondition {
    pub(crate) condition: Condition,
    /// Present only for the three conditions whose operand is typed text.
    pub(crate) text: Option<Entity<InputState>>,
}

pub(crate) struct DraftGroup {
    pub(crate) combine: Combine,
    pub(crate) conditions: Vec<DraftCondition>,
}

/// A rule as it is being edited. Nothing here has reached the service yet, so
/// abandoning the editor changes nothing.
pub(crate) struct RuleDraft {
    /// `None` while creating a rule the service has not assigned an id to.
    pub(crate) rule_id: Option<u64>,
    pub(crate) name: Entity<InputState>,
    pub(crate) enabled: bool,
    pub(crate) priority: u8,
    pub(crate) combine: Combine,
    pub(crate) groups: Vec<DraftGroup>,
    pub(crate) policy: SessionPolicy,
    pub(crate) battery_stop_percent: Option<u8>,
    /// Why the last save attempt was refused, as text already localized.
    pub(crate) error: Option<String>,
}

pub(crate) struct AwakeApp {
    pub(crate) section: Section,
    pub(crate) locale: Locale,
    pub(crate) preferences: Preferences,
    pub(crate) store: PreferencesStore,
    /// False once a write failed or a stored file could not be understood, which
    /// the Settings section says out loud rather than pretending it saved.
    pub(crate) preferences_saved: bool,

    pub(crate) status: Option<StatusView>,
    pub(crate) rules: Vec<RuleView>,
    /// The rules exactly as the service reported them. The presented rows carry
    /// localized sentences rather than operands, so the editor is opened from
    /// these rather than by parsing what is on screen.
    pub(crate) raw_rules: Vec<Rule>,
    pub(crate) rules_suppression: Option<WireSuppression>,
    pub(crate) providers: Vec<ProviderRow>,
    pub(crate) history: Vec<HistoryRow>,
    pub(crate) history_total: u32,
    pub(crate) history_retention: u32,
    pub(crate) protocol_version: Option<u32>,

    /// Why the service could not be read. Set means every section shows the
    /// unreachable state rather than stale numbers.
    pub(crate) connection_error: Option<String>,
    /// Why the last action was refused. Cleared by the next successful action.
    pub(crate) action_error: Option<String>,
    pub(crate) test: Option<RuleTestView>,
    pub(crate) draft: Option<RuleDraft>,
    pub(crate) offset: UtcOffset,
    pub(crate) busy: bool,
    _work: Option<Task<()>>,
}

impl AwakeApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = PreferencesStore::from_default_path();
        let (preferences, readable) = store.load();
        let locale = preferences.locale();
        apply_theme(preferences.theme, window, cx);

        let mut app = Self {
            section: Section::Status,
            locale,
            preferences,
            store,
            preferences_saved: readable,
            status: None,
            rules: Vec::new(),
            raw_rules: Vec::new(),
            rules_suppression: None,
            providers: Vec::new(),
            history: Vec::new(),
            history_total: 0,
            history_retention: 0,
            protocol_version: None,
            connection_error: None,
            action_error: None,
            test: None,
            draft: None,
            offset: UtcOffset::UTC,
            busy: true,
            _work: None,
        };
        app.refresh(cx);
        app
    }

    // ---- Navigation ----------------------------------------------------

    pub(crate) fn navigate(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            // A test result belongs to the screen it was asked for on.
            if section != Section::Rules {
                self.test = None;
            }
            cx.notify();
        }
    }

    // ---- Reading -------------------------------------------------------

    /// Reads every section's document in one round, off the main thread.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        let work = cx.background_spawn(async move { Snapshot::read(HISTORY_PAGE) });
        self._work = Some(cx.spawn(async move |this, cx| {
            let snapshot = work.await;
            this.update(cx, |this, cx| {
                this.apply(snapshot);
                cx.notify();
            })
            .ok();
        }));
    }

    fn apply(&mut self, snapshot: Snapshot) {
        self.busy = false;
        self.protocol_version = snapshot.protocol_version;
        match snapshot.status {
            Ok(status) => {
                self.offset = UtcOffset::for_system(status.now_unix_seconds);
                self.providers = status
                    .providers
                    .iter()
                    .map(ProviderRow::from_wire)
                    .collect();
                self.status = Some(StatusView::from_status(&status));
                self.connection_error = None;
            }
            Err(error) => {
                self.status = None;
                self.providers.clear();
                self.connection_error = Some(error.to_string());
            }
        }

        let c = copy(self.locale);
        match snapshot.rules {
            Ok(document) => {
                self.rules = document
                    .rules
                    .iter()
                    .map(|rule| {
                        RuleView::present(rule, &document.matching_rule_ids, &self.providers, c)
                    })
                    .collect();
                self.raw_rules = document.rules;
                self.rules_suppression = document.suppression;
            }
            Err(_) => {
                self.rules.clear();
                self.raw_rules.clear();
                self.rules_suppression = None;
            }
        }

        match snapshot.history {
            Ok(document) => {
                self.history = document.entries.iter().map(HistoryRow::from_wire).collect();
                self.history_total = document.total;
                self.history_retention = document.retention_limit;
            }
            Err(_) => {
                self.history.clear();
                self.history_total = 0;
                self.history_retention = 0;
            }
        }
    }

    // ---- Writing -------------------------------------------------------

    /// Sends one request and re-reads everything the reply could have changed.
    ///
    /// The window never assumes its own request succeeded. What it shows next
    /// is what the service reports afterwards, so an action the service refused
    /// never leaves a screen claiming it happened.
    pub(crate) fn dispatch(&mut self, body: RequestBody, cx: &mut Context<Self>) {
        self.busy = true;
        let work = cx.background_spawn(async move {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => return Err(ClientError::Transport(error.to_string())),
            };
            runtime.block_on(async move {
                let client = ServiceClient::connect().await?;
                client.status_request(body).await.map(|_| ())
            })
        });
        self._work = Some(cx.spawn(async move |this, cx| {
            let outcome = work.await;
            this.update(cx, |this, cx| {
                this.action_error = outcome.err().map(|error| error.to_string());
                this.refresh(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Tests one rule and keeps the answer on screen. Acquires nothing.
    pub(crate) fn test_rule(&mut self, rule_id: u64, cx: &mut Context<Self>) {
        self.busy = true;
        let work = cx.background_spawn(async move {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| ClientError::Transport(error.to_string()))?;
            runtime.block_on(async move {
                let client = ServiceClient::connect().await?;
                client.test_rule(rule_id).await
            })
        });
        self._work = Some(cx.spawn(async move |this, cx| {
            let outcome = work.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match outcome {
                    Ok(document) => {
                        this.action_error = None;
                        this.test = Some(RuleTestView::from_document(&document));
                    }
                    Err(error) => {
                        this.test = None;
                        this.action_error = Some(error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    // ---- Preferences ---------------------------------------------------

    fn commit_preferences(&mut self, cx: &mut Context<Self>) {
        self.preferences_saved = self.store.save(&self.preferences).is_ok();
        cx.notify();
    }

    pub(crate) fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        self.preferences.locale = locale.as_key().to_string();
        // Rule rows carry localized condition sentences, so they are rebuilt
        // rather than left in the previous language until the next refresh.
        self.commit_preferences(cx);
        self.refresh(cx);
    }

    pub(crate) fn set_theme(
        &mut self,
        theme: StoredTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preferences.theme = theme;
        apply_theme(theme, window, cx);
        self.commit_preferences(cx);
    }

    pub(crate) fn move_preset(&mut self, index: usize, delta: isize, cx: &mut Context<Self>) {
        if self.preferences.move_preset(index, delta) {
            self.commit_preferences(cx);
        }
    }

    pub(crate) fn remove_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.preferences.remove_preset(index) {
            self.commit_preferences(cx);
        }
    }

    pub(crate) fn add_preset(&mut self, length: PresetLength, cx: &mut Context<Self>) {
        if self.preferences.add_preset(length) {
            self.commit_preferences(cx);
        }
    }

    pub(crate) fn set_default_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.preferences.presets.len() {
            self.preferences.default_preset = index;
            self.commit_preferences(cx);
        }
    }

    pub(crate) fn restore_default_presets(&mut self, cx: &mut Context<Self>) {
        self.preferences.restore_default_presets();
        self.commit_preferences(cx);
    }

    pub(crate) fn set_default_policy(&mut self, policy: SessionPolicy, cx: &mut Context<Self>) {
        self.preferences.defaults.policy = policy;
        self.commit_preferences(cx);
    }

    pub(crate) fn set_default_battery(&mut self, percent: Option<u8>, cx: &mut Context<Self>) {
        self.preferences.defaults.battery_stop_percent = percent;
        self.commit_preferences(cx);
    }

    // ---- Sessions ------------------------------------------------------

    pub(crate) fn start_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(length) = self.preferences.presets.get(index).copied() else {
            return;
        };
        let defaults = self.preferences.defaults;
        let end = match length {
            PresetLength::Indefinite => WireEnd::Indefinite,
            PresetLength::Minutes { minutes } => WireEnd::Duration {
                seconds: minutes * 60,
            },
        };
        self.dispatch(
            RequestBody::StartSession {
                reason: copy(self.locale).application_name.to_string(),
                policy: defaults.policy,
                battery_stop_percent: defaults.battery_stop_percent,
                end,
                // The consequence is stated in Session Defaults and in Battery
                // & Safety, which is where this policy was chosen.
                security_confirmed: defaults.policy.needs_security_confirmation(),
            },
            cx,
        );
    }

    pub(crate) fn end_session(&mut self, session_id: u64, cx: &mut Context<Self>) {
        self.dispatch(RequestBody::EndSession { session_id }, cx);
    }

    pub(crate) fn end_manual_session(&mut self, cx: &mut Context<Self>) {
        self.dispatch(RequestBody::EndManualSession, cx);
    }

    pub(crate) fn extend_session(&mut self, session_id: u64, minutes: u64, cx: &mut Context<Self>) {
        self.dispatch(
            RequestBody::ExtendSession {
                session_id,
                by_seconds: minutes * 60,
            },
            cx,
        );
    }

    /// Replaces the mutable part of the running manual session with the current
    /// session defaults, which is what Modify offers: the policy the person just
    /// chose, applied to the session already running.
    pub(crate) fn modify_session(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let defaults = self.preferences.defaults;
        let reason = self
            .status
            .as_ref()
            .and_then(|status| {
                status
                    .reasons
                    .iter()
                    .find(|reason| reason.session_id == session_id)
            })
            .map(|reason| reason.reason.clone())
            .unwrap_or_else(|| copy(self.locale).application_name.to_string());
        self.dispatch(
            RequestBody::ChangeSession {
                session_id,
                reason,
                policy: defaults.policy,
                battery_stop_percent: defaults.battery_stop_percent,
                end: WireEnd::Indefinite,
                security_confirmed: defaults.policy.needs_security_confirmation(),
            },
            cx,
        );
    }

    // ---- Rules ---------------------------------------------------------

    pub(crate) fn set_rule_enabled(&mut self, rule_id: u64, enabled: bool, cx: &mut Context<Self>) {
        self.dispatch(RequestBody::SetRuleEnabled { rule_id, enabled }, cx);
    }

    pub(crate) fn duplicate_rule(&mut self, rule_id: u64, cx: &mut Context<Self>) {
        self.dispatch(RequestBody::DuplicateRule { rule_id }, cx);
    }

    pub(crate) fn delete_rule(&mut self, rule_id: u64, cx: &mut Context<Self>) {
        self.dispatch(RequestBody::DeleteRule { rule_id }, cx);
    }

    pub(crate) fn move_rule(&mut self, rule_id: u64, delta: isize, cx: &mut Context<Self>) {
        let Some(index) = self.rules.iter().position(|rule| rule.rule_id == rule_id) else {
            return;
        };
        let target = index as isize + delta;
        if target < 0 || target as usize >= self.rules.len() {
            return;
        }
        self.dispatch(
            RequestBody::ReorderRule {
                rule_id,
                to_index: target as u32,
            },
            cx,
        );
    }

    pub(crate) fn set_rule_priority(&mut self, rule_id: u64, priority: u8, cx: &mut Context<Self>) {
        self.dispatch(RequestBody::SetRulePriority { rule_id, priority }, cx);
    }

    pub(crate) fn pause_rules(&mut self, seconds: Option<u64>, cx: &mut Context<Self>) {
        self.dispatch(RequestBody::PauseRules { seconds }, cx);
    }

    pub(crate) fn resume_rules(&mut self, cx: &mut Context<Self>) {
        self.dispatch(RequestBody::ResumeRules, cx);
    }

    pub(crate) fn override_all_rules(&mut self, cx: &mut Context<Self>) {
        // The consequence is shown next to the control; the flag is what makes
        // the service accept it at all.
        self.dispatch(RequestBody::OverrideAllRules { confirmed: true }, cx);
    }

    // ---- The rule editor ------------------------------------------------

    pub(crate) fn edit_rule(&mut self, rule_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rule) = self.rules.iter().find(|rule| rule.rule_id == rule_id) else {
            return;
        };
        let name = rule.name.clone();
        let enabled = rule.enabled;
        let priority = rule.priority;
        let combine = rule.combine;
        let policy = rule.policy;
        let battery = rule.battery_stop_percent;
        // The presented rule carries sentences, not operands, so the draft is
        // built from the conditions the presenter was given.
        let conditions = self.rule_conditions(rule_id);
        let name_input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let groups = conditions
            .into_iter()
            .map(|(group_combine, group_conditions)| DraftGroup {
                combine: group_combine,
                conditions: group_conditions
                    .into_iter()
                    .map(|condition| Self::draft_condition(condition, window, cx))
                    .collect(),
            })
            .collect();
        self.draft = Some(RuleDraft {
            rule_id: Some(rule_id),
            name: name_input,
            enabled,
            priority,
            combine,
            groups,
            policy,
            battery_stop_percent: battery,
            error: None,
        });
        cx.notify();
    }

    pub(crate) fn new_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = copy(self.locale);
        let name = cx.new(|cx| InputState::new(window, cx).default_value(c.new_rule));
        // A new rule starts with one condition that needs no operand and no
        // provider that can be missing, so the editor opens on something valid.
        let condition = Condition::AcPower { connected: true };
        self.draft = Some(RuleDraft {
            rule_id: None,
            name,
            enabled: true,
            priority: DEFAULT_PRIORITY,
            combine: Combine::All,
            groups: vec![DraftGroup {
                combine: Combine::All,
                conditions: vec![Self::draft_condition(condition, window, cx)],
            }],
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: self.preferences.defaults.battery_stop_percent,
            error: None,
        });
        cx.notify();
    }

    pub(crate) fn close_draft(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        cx.notify();
    }

    /// The conditions of one rule, read back out of the service's last reply.
    ///
    /// The presenter deliberately keeps sentences rather than operands, so the
    /// editor asks the source documents again. Keeping the raw rules alongside
    /// the presented ones is what makes that possible without a round trip.
    fn rule_conditions(&self, rule_id: u64) -> Vec<(Combine, Vec<Condition>)> {
        self.raw_rules
            .iter()
            .find(|rule| rule.id.0 == rule_id)
            .map(|rule| {
                rule.groups
                    .iter()
                    .map(|group| (group.combine, group.conditions.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn draft_condition(
        condition: Condition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DraftCondition {
        let text = match &condition {
            Condition::ProcessRunning { matcher } => Some(matcher.as_str().to_string()),
            Condition::NetworkInterfaceUp { interface } => Some(interface.as_str().to_string()),
            Condition::WatchedPathActive { path, .. } => Some(path.as_path().display().to_string()),
            _ => None,
        };
        DraftCondition {
            condition,
            text: text.map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value))),
        }
    }

    /// Turns the draft into a rule, or explains why it cannot be one.
    pub(crate) fn draft_rule(&self, cx: &App) -> Result<Rule, String> {
        let c = copy(self.locale);
        let draft = self.draft.as_ref().ok_or_else(|| c.unknown.to_string())?;
        let name = Reason::new(draft.name.read(cx).value().to_string())
            .map_err(|error| format!("{}: {error}", c.rule_invalid_input))?;

        let mut groups = Vec::new();
        for group in &draft.groups {
            let mut conditions = Vec::new();
            for condition in &group.conditions {
                conditions.push(Self::resolved_condition(condition, cx, c)?);
            }
            groups.push(
                ConditionGroup::new(group.combine, conditions)
                    .map_err(|error| format!("{}: {error}", c.rule_invalid_input))?,
            );
        }

        // The service assigns the identity; the id sent with a create is
        // ignored, and an update is aimed by the request's own `rule_id`.
        let mut rule = Rule::new(
            RuleId(draft.rule_id.unwrap_or(1)),
            name,
            draft.combine,
            groups,
        )
        .map_err(|error| format!("{}: {error}", c.rule_invalid_input))?;
        rule.enabled = draft.enabled;
        rule.priority = draft.priority;
        rule.policy = draft.policy;
        rule.battery_stop_percent = draft.battery_stop_percent;
        rule.validate()
            .map_err(|error| format!("{}: {error}", c.rule_invalid_input))?;
        Ok(rule)
    }

    /// A condition with its typed operand read back out of its text field and
    /// validated by `awake-core`. An invalid value is refused here rather than
    /// sent, so the service never has to reject what the editor could see.
    fn resolved_condition(
        draft: &DraftCondition,
        cx: &App,
        c: &'static crate::i18n::Copy,
    ) -> Result<Condition, String> {
        let typed = draft
            .text
            .as_ref()
            .map(|input| input.read(cx).value().to_string());
        let invalid = |error: awake_core::RuleError| format!("{}: {error}", c.rule_invalid_input);
        Ok(match (&draft.condition, typed) {
            (Condition::ProcessRunning { matcher }, Some(value)) => Condition::ProcessRunning {
                matcher: ProcessMatcher::new(matcher.kind, value).map_err(invalid)?,
            },
            (Condition::NetworkInterfaceUp { .. }, Some(value)) => Condition::NetworkInterfaceUp {
                interface: InterfaceName::new(value).map_err(invalid)?,
            },
            (Condition::WatchedPathActive { within_seconds, .. }, Some(value)) => {
                Condition::WatchedPathActive {
                    path: WatchedPath::new(value).map_err(invalid)?,
                    within_seconds: *within_seconds,
                }
            }
            (condition, _) => condition.clone(),
        })
    }

    pub(crate) fn save_draft(&mut self, cx: &mut Context<Self>) {
        let rule = match self.draft_rule(cx) {
            Ok(rule) => rule,
            Err(message) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.error = Some(message);
                }
                cx.notify();
                return;
            }
        };
        let body = match self.draft.as_ref().and_then(|draft| draft.rule_id) {
            Some(rule_id) => RequestBody::UpdateRule {
                rule_id,
                rule: Box::new(rule),
            },
            None => RequestBody::CreateRule {
                rule: Box::new(rule),
            },
        };
        self.draft = None;
        self.dispatch(body, cx);
    }

    /// Adds a condition for one provider, with an operand that is already valid.
    pub(crate) fn add_condition(
        &mut self,
        group: usize,
        condition: Condition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft_condition = Self::draft_condition(condition, window, cx);
        if let Some(draft) = self.draft.as_mut()
            && let Some(group) = draft.groups.get_mut(group)
            && group.conditions.len() < awake_core::MAX_CONDITIONS_PER_GROUP
        {
            group.conditions.push(draft_condition);
        }
        cx.notify();
    }

    pub(crate) fn remove_condition(&mut self, group: usize, index: usize, cx: &mut Context<Self>) {
        if let Some(draft) = self.draft.as_mut()
            && let Some(group) = draft.groups.get_mut(group)
            // A group with no conditions is vacuously true, which the core
            // refuses; the editor refuses to create one in the first place.
            && group.conditions.len() > 1
        {
            group.conditions.remove(index);
        }
        cx.notify();
    }

    pub(crate) fn add_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let condition = Self::draft_condition(Condition::AcPower { connected: true }, window, cx);
        if let Some(draft) = self.draft.as_mut()
            && draft.groups.len() < awake_core::MAX_GROUPS_PER_RULE
        {
            draft.groups.push(DraftGroup {
                combine: Combine::All,
                conditions: vec![condition],
            });
        }
        cx.notify();
    }

    pub(crate) fn remove_group(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(draft) = self.draft.as_mut()
            && draft.groups.len() > 1
        {
            draft.groups.remove(index);
        }
        cx.notify();
    }

    /// Changes one condition's operand in place. The change is applied to the
    /// draft only; nothing reaches the service until Save.
    pub(crate) fn update_condition(
        &mut self,
        group: usize,
        index: usize,
        change: impl FnOnce(&mut Condition),
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.draft.as_mut()
            && let Some(group) = draft.groups.get_mut(group)
            && let Some(condition) = group.conditions.get_mut(index)
        {
            change(&mut condition.condition);
            draft.error = None;
        }
        cx.notify();
    }

    pub(crate) fn update_draft(
        &mut self,
        change: impl FnOnce(&mut RuleDraft),
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.draft.as_mut() {
            change(draft);
            draft.error = None;
        }
        cx.notify();
    }

    /// The condition a provider contributes when it is added to a group, with a
    /// default operand that already validates.
    pub(crate) fn default_condition(provider: awake_core::ProviderKind) -> Condition {
        use awake_core::ProviderKind::*;
        match provider {
            ProcessRunning => Condition::ProcessRunning {
                matcher: ProcessMatcher::new(ProcessMatchKind::ExecutableName, "process")
                    .expect("a plain executable name is a valid matcher"),
            },
            AcPower => Condition::AcPower { connected: true },
            BatteryPercent => Condition::BatteryPercent {
                at_least: 20,
                at_most: 100,
            },
            ExternalDisplay => Condition::ExternalDisplay { connected: true },
            AudioPlayback => Condition::AudioPlayback { playing: true },
            CpuUtilization => Condition::CpuUtilizationAtLeast { percent: 50 },
            NetworkThroughput => Condition::NetworkThroughputAtLeast {
                kibibytes_per_second: 100,
            },
            NetworkInterface => Condition::NetworkInterfaceUp {
                interface: InterfaceName::new("eth0").expect("a plain interface name is valid"),
            },
            TimeSchedule => Condition::TimeSchedule {
                schedule: Schedule::new(Weekday::ALL.to_vec(), 9 * 60, 18 * 60)
                    .expect("a whole-week nine-to-six window is valid"),
            },
            WatchedPath => Condition::WatchedPathActive {
                path: awake_core::WatchedPath::new("/tmp").expect("an absolute path is valid"),
                within_seconds: 300,
            },
            Fullscreen => Condition::Fullscreen { active: true },
        }
    }
}

pub(crate) fn apply_theme(theme: StoredTheme, window: &mut Window, cx: &mut App) {
    match theme {
        StoredTheme::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        StoredTheme::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        StoredTheme::System => Theme::sync_system_appearance(Some(window), cx),
    }
}
