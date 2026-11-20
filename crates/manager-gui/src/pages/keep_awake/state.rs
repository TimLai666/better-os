//! What Keep awake holds between interactions, and every change it can ask
//! for. Each change is one request to Better Awake's service, sent off the
//! UI thread; the page then reads the service again and shows what it says,
//! so a refused change never leaves a screen claiming it happened.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use awake_core::{
    Combine, Condition, ConditionGroup, DEFAULT_PRIORITY, InterfaceName, ProcessMatchKind,
    ProcessMatcher, Reason, Rule, RuleId, Schedule, SessionPolicy, WatchedPath, Weekday,
};
use awake_ipc::{QuickLength, QuickSessions, RequestBody, WireEnd};
use better_ui::fluent::list::SortState;
use better_ui::fluent::time_picker::Time;
use gpui::{AppContext as _, Context, Entity, Task, Window};
use gpui_component::input::InputState;

use super::bus::{self, Outcome, Read, Target};
use super::copy::{copy, page_copy};
use super::decide::{self, Facts, ModifyDraft, Presence, Service, SubPage};
use super::model::{HistoryRow, ProviderRow, RuleTestView, RuleView};
use crate::app::ManagerApp;
use better_core::local_time::TimeZone;

/// The component's id in Better Manager's catalog.
pub(crate) const COMPONENT: &str = "better-awake";

/// One condition being edited, with the fields its operands are typed in.
pub(crate) struct DraftCondition {
    pub(crate) condition: Condition,
    /// The typed text, for the three conditions whose operand is text.
    pub(crate) text: Option<Entity<InputState>>,
    /// The number boxes, in the order the editor draws them.
    pub(crate) numbers: Vec<Entity<InputState>>,
}

pub(crate) struct DraftGroup {
    pub(crate) combine: Combine,
    pub(crate) conditions: Vec<DraftCondition>,
}

/// A rule as it is being edited. Nothing reaches the service until Save.
pub(crate) struct RuleDraft {
    /// `None` while creating a rule the service has not given an id yet.
    pub(crate) rule_id: Option<u64>,
    pub(crate) name: Entity<InputState>,
    pub(crate) enabled: bool,
    pub(crate) priority: u8,
    pub(crate) priority_input: Entity<InputState>,
    pub(crate) combine: Combine,
    pub(crate) groups: Vec<DraftGroup>,
    pub(crate) policy: SessionPolicy,
    pub(crate) battery_stop_percent: Option<u8>,
    /// Why the last save was refused, already in words.
    pub(crate) error: Option<String>,
}

/// A dialog the page shows over the window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Dialog {
    Until(Time),
    Modify(ModifyDraft),
    DeleteRule { rule_id: u64, name: String },
    OverrideAll,
}

#[derive(Default)]
pub(crate) struct KeepAwakeState {
    pub(crate) sub: SubPage,
    pub(crate) read: Option<Read>,
    /// What the battery is doing, read from sysfs with each read of the
    /// service, for the charge card (`hardware_core::charge_state`, ticket
    /// 170): the service's status says only "on AC power".
    pub(crate) charge: Option<hardware_core::charge_state::ChargeReadings>,
    /// When the read in flight started, for the 300 ms ring.
    pub(crate) reading_since: Option<Instant>,
    pub(crate) busy: bool,
    /// The last change or read that failed, as (summary, details).
    pub(crate) failure: Option<(&'static str, String)>,
    pub(crate) dialog: Option<Dialog>,
    /// A reason whose End was pressed: its card says what remains and asks
    /// once more, in place.
    pub(crate) pending_end: Option<u64>,
    pub(crate) conflicts_open: bool,
    pub(crate) quick_open: bool,
    pub(crate) open_rules: HashSet<u64>,
    pub(crate) priority_inputs: HashMap<u64, Entity<InputState>>,
    pub(crate) battery_input: Option<Entity<InputState>>,
    pub(crate) test: Option<RuleTestView>,
    pub(crate) draft: Option<RuleDraft>,
    pub(crate) history_sort: Option<SortState>,
    pub(crate) following: bool,
    /// A read was asked for while one ran; it runs once that one lands.
    again: bool,
    task: Option<Task<()>>,
}

impl KeepAwakeState {
    pub(crate) fn status(&self) -> Option<&awake_ipc::StatusDocument> {
        self.read
            .as_ref()
            .and_then(|read| read.status.as_ref().ok())
    }

    pub(crate) fn offer(&self) -> Option<&awake_ipc::QuickSessionsDocument> {
        self.read.as_ref().and_then(|read| read.offer.as_ref())
    }

    pub(crate) fn providers(&self) -> Vec<ProviderRow> {
        self.status()
            .map(|status| {
                status
                    .providers
                    .iter()
                    .map(ProviderRow::from_wire)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn rule_views(&self, locale: crate::i18n::Locale) -> Vec<RuleView> {
        let c = copy(locale);
        let providers = self.providers();
        self.read
            .as_ref()
            .and_then(|read| read.rules.as_ref())
            .map(|rules| {
                rules
                    .rules
                    .iter()
                    .map(|rule| RuleView::present(rule, &rules.matching_rule_ids, &providers, c))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn raw_rule(&self, rule_id: u64) -> Option<&Rule> {
        self.read
            .as_ref()
            .and_then(|read| read.rules.as_ref())
            .and_then(|rules| rules.rules.iter().find(|rule| rule.id.0 == rule_id))
    }

    pub(crate) fn history(&self) -> Vec<HistoryRow> {
        self.read
            .as_ref()
            .and_then(|read| read.history.as_ref())
            .map(|history| history.entries.iter().map(HistoryRow::from_wire).collect())
            .unwrap_or_default()
    }

    /// Whether the first read is still out and has taken long enough for a
    /// ring (spec section 2: nothing under 300 ms).
    pub(crate) fn shows_ring(&self) -> bool {
        self.read.is_none()
            && self.reading_since.is_some_and(|since| {
                since.elapsed()
                    >= Duration::from_millis(crate::pages::states::LOADING_RING_AFTER_MS)
            })
    }

    /// Whether a restart has to wait: a dialog or an editor holds typing a
    /// restart cannot carry, or a change is on its way.
    pub(crate) fn blocks_restart(&self) -> bool {
        self.draft.is_some() || self.dialog.is_some() || self.busy
    }
}

impl ManagerApp {
    /// Better Awake as Better Manager's record has it.
    pub(crate) fn keep_awake_presence(&self) -> Presence {
        self.components()
            .into_iter()
            .find(|component| component.core_id.as_str() == COMPONENT)
            .map(|component| Presence {
                installed: component.installed_version.is_some(),
                enabled: component.enabled,
                version: component.installed_version,
            })
            .unwrap_or_default()
    }

    pub(crate) fn keep_awake_service(&self) -> Service {
        let read = self
            .keep_awake
            .read
            .as_ref()
            .map(|read| (&read.status, read.version));
        Service::decide(read, &self.keep_awake_presence())
    }

    pub(crate) fn keep_awake_facts(&self) -> Facts {
        decide::facts(
            &self.keep_awake_service(),
            self.keep_awake.status(),
            page_copy(self.locale),
            copy(self.locale),
        )
    }

    /// Reads the service again, off the UI thread. A read asked for while
    /// one runs happens once that one lands.
    pub(crate) fn refresh_keep_awake(&mut self, cx: &mut Context<Self>) {
        if self.keep_awake.task.is_some() {
            self.keep_awake.again = true;
            return;
        }
        self.keep_awake.reading_since = Some(Instant::now());
        if !self.keep_awake.following {
            self.keep_awake.following = true;
            self.follow_keep_awake(cx);
        }
        self.keep_awake.task = Some(cx.spawn(async move |this, cx| {
            // A ring is drawn only for a read that takes 300 ms or more.
            let ring = cx.background_executor().timer(Duration::from_millis(
                crate::pages::states::LOADING_RING_AFTER_MS,
            ));
            let work = cx.background_spawn(async move {
                let read = match Target::session() {
                    Ok(target) => bus::read(&target),
                    Err(error) => Read {
                        version: None,
                        status: Err(decide::ReadError::NotOnBus(error)),
                        rules: None,
                        history: None,
                        offer: None,
                    },
                };
                let charge =
                    hardware_platform::sysfs::charge_readings(&crate::pages::system::state::root());
                (read, charge)
            });
            ring.await;
            let _ = this.update(cx, |_, cx| cx.notify());
            let (read, charge) = work.await;
            let _ = this.update(cx, |app, cx| {
                app.keep_awake.task = None;
                app.keep_awake.reading_since = None;
                app.keep_awake.read = Some(read);
                app.keep_awake.charge = charge;
                if app.keep_awake.again {
                    app.keep_awake.again = false;
                    app.refresh_keep_awake(cx);
                }
                cx.notify();
            });
        }));
    }

    /// Follows the service so Home, the pane's dot, and an open page change
    /// as soon as the service does, without polling.
    fn follow_keep_awake(&mut self, cx: &mut Context<Self>) {
        let (sender, receiver) = smol::channel::unbounded::<()>();
        std::thread::spawn(move || {
            if let Ok(target) = Target::session() {
                bus::follow(target, move || sender.send_blocking(()).is_ok());
            }
        });
        cx.spawn(async move |this, cx| {
            while receiver.recv().await.is_ok() {
                // A burst of events is one read.
                while receiver.try_recv().is_ok() {}
                if this
                    .update(cx, |app, cx| app.refresh_keep_awake(cx))
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }

    /// Sends one change and reads everything again.
    pub(crate) fn keep_awake_send(&mut self, body: RequestBody, cx: &mut Context<Self>) {
        self.keep_awake.busy = true;
        self.keep_awake.failure = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    match Target::session() {
                        Ok(target) => bus::perform(&target, body),
                        Err(error) => Outcome::Failed(error),
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.keep_awake.busy = false;
                let w = page_copy(app.locale);
                app.keep_awake.failure = match outcome {
                    Outcome::Done => None,
                    Outcome::Refused(key) => Some((w.service_refused, key)),
                    Outcome::Failed(details) => Some((w.action_failed, details)),
                };
                app.refresh_keep_awake(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// "Start service": `manager-core`'s pass over the installed components'
    /// user units starts Better Awake's (AGENTS.md: a window never starts a
    /// unit itself), and the page reads the service once that pass lands.
    pub(crate) fn start_keep_awake_service(&mut self, cx: &mut Context<Self>) {
        self.heal_user_units(cx);
        self.refresh_keep_awake(cx);
    }

    pub(crate) fn set_keep_awake_sub(&mut self, sub: SubPage, cx: &mut Context<Self>) {
        if self.keep_awake.sub != sub {
            self.keep_awake.sub = sub;
            self.keep_awake.test = None;
            self.keep_awake.pending_end = None;
            self.focused_card = None;
            self.content_scroll.set_offset(gpui::Point::default());
        }
        cx.notify();
    }

    /// Opens the page the way `keep-awake/until` asks: Status, with the time
    /// dialog open.
    pub(crate) fn open_until_dialog(&mut self, cx: &mut Context<Self>) {
        let now = self
            .keep_awake
            .status()
            .map(|status| status.now_unix_seconds)
            .unwrap_or_else(manager_core::catalog::now_unix_seconds);
        self.keep_awake.sub = SubPage::Status;
        self.keep_awake.dialog = Some(Dialog::Until(decide::suggested_until(
            now,
            TimeZone::System,
        )));
        cx.notify();
    }

    pub(crate) fn close_keep_awake_dialog(&mut self, cx: &mut Context<Self>) {
        self.keep_awake.dialog = None;
        cx.notify();
    }

    fn offer_or_shipped(&self) -> QuickSessions {
        self.keep_awake
            .offer()
            .map(|document| document.quick_sessions.clone())
            .unwrap_or_default()
    }

    fn session_reason(&self) -> String {
        page_copy(self.locale).until_reason.to_string()
    }

    // ---- Sessions ------------------------------------------------------

    pub(crate) fn start_keep_awake_default(&mut self, cx: &mut Context<Self>) {
        let offer = self.offer_or_shipped();
        if let Some(length) = offer.default_length() {
            let body = decide::start_request(&offer, length, &self.session_reason());
            self.keep_awake_send(body, cx);
        }
    }

    pub(crate) fn start_keep_awake_length(&mut self, length: QuickLength, cx: &mut Context<Self>) {
        let offer = self.offer_or_shipped();
        let body = decide::start_request(&offer, length, &self.session_reason());
        self.keep_awake_send(body, cx);
    }

    pub(crate) fn start_keep_awake_until(&mut self, time: Time, cx: &mut Context<Self>) {
        let now = self
            .keep_awake
            .status()
            .map(|status| status.now_unix_seconds)
            .unwrap_or_else(manager_core::catalog::now_unix_seconds);
        let until = decide::until(now, TimeZone::System, time);
        let offer = self.offer_or_shipped();
        let body = decide::start_with_end(
            &offer,
            WireEnd::UntilUnixSeconds {
                unix_seconds: until.unix_seconds,
            },
            &self.session_reason(),
        );
        self.keep_awake.dialog = None;
        self.keep_awake_send(body, cx);
    }

    /// The Home toggle: on starts the default length, off ends the person's
    /// own session and leaves the rules' running.
    pub(crate) fn toggle_keep_awake(&mut self, on: bool, cx: &mut Context<Self>) {
        if on {
            self.start_keep_awake_default(cx);
        } else {
            self.keep_awake_send(RequestBody::EndManualSession, cx);
        }
    }

    pub(crate) fn extend_keep_awake(
        &mut self,
        session_id: u64,
        minutes: u64,
        cx: &mut Context<Self>,
    ) {
        self.keep_awake_send(
            RequestBody::ExtendSession {
                session_id,
                by_seconds: minutes * 60,
            },
            cx,
        );
    }

    pub(crate) fn end_keep_awake_reason(&mut self, session_id: u64, cx: &mut Context<Self>) {
        self.keep_awake.pending_end = None;
        self.keep_awake_send(RequestBody::EndSession { session_id }, cx);
    }

    pub(crate) fn ask_end_keep_awake_reason(
        &mut self,
        session_id: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        self.keep_awake.pending_end = session_id;
        cx.notify();
    }

    pub(crate) fn end_keep_awake_manual(&mut self, cx: &mut Context<Self>) {
        self.keep_awake_send(RequestBody::EndManualSession, cx);
    }

    pub(crate) fn open_modify(&mut self, session_id: u64, cx: &mut Context<Self>) {
        let Some(status) = self.keep_awake.status() else {
            return;
        };
        let Some(session) = status.session(awake_core::SessionId(session_id)) else {
            return;
        };
        let offer = self.offer_or_shipped();
        let length = offer
            .lengths
            .iter()
            .position(|length| length.end() == session.end)
            .or(Some(offer.default_index as usize));
        let now = status.now_unix_seconds;
        let draft = ModifyDraft {
            session_id,
            length,
            until: decide::suggested_until(now, TimeZone::System),
            policy: session.policy,
            battery_stop_percent: session.battery_stop_percent,
        };
        self.keep_awake.dialog = Some(Dialog::Modify(draft));
        cx.notify();
    }

    pub(crate) fn update_modify(
        &mut self,
        change: impl FnOnce(&mut ModifyDraft),
        cx: &mut Context<Self>,
    ) {
        if let Some(Dialog::Modify(draft)) = self.keep_awake.dialog.as_mut() {
            change(draft);
        }
        cx.notify();
    }

    pub(crate) fn save_modify(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Modify(draft)) = self.keep_awake.dialog.clone() else {
            return;
        };
        let Some(status) = self.keep_awake.status() else {
            return;
        };
        let reason = status
            .session(awake_core::SessionId(draft.session_id))
            .map(|session| session.reason.clone())
            .unwrap_or_else(|| self.session_reason());
        let now = status.now_unix_seconds;
        let body = decide::modify_request(
            &draft,
            &self.offer_or_shipped(),
            &reason,
            now,
            TimeZone::System,
        );
        self.keep_awake.dialog = None;
        self.keep_awake_send(body, cx);
    }

    // ---- Rules, as a whole --------------------------------------------

    pub(crate) fn pause_keep_awake_rules(&mut self, seconds: Option<u64>, cx: &mut Context<Self>) {
        self.keep_awake_send(RequestBody::PauseRules { seconds }, cx);
    }

    pub(crate) fn resume_keep_awake_rules(&mut self, cx: &mut Context<Self>) {
        self.keep_awake_send(RequestBody::ResumeRules, cx);
    }

    pub(crate) fn override_keep_awake_rules(&mut self, cx: &mut Context<Self>) {
        self.keep_awake.dialog = None;
        // The consequence was shown in the dialog; the flag is what makes
        // the service accept it.
        self.keep_awake_send(RequestBody::OverrideAllRules { confirmed: true }, cx);
    }

    // ---- The quick-session offer --------------------------------------

    /// Changes the offer and sends the whole of it, which the service keeps
    /// and pushes to every tray.
    pub(crate) fn change_offer(
        &mut self,
        change: impl FnOnce(&mut QuickSessions) -> bool,
        cx: &mut Context<Self>,
    ) {
        let mut offer = self.offer_or_shipped();
        if !change(&mut offer) || offer.validate().is_err() {
            return;
        }
        self.keep_awake_send(
            RequestBody::SetQuickSessions {
                quick_sessions: offer,
            },
            cx,
        );
    }

    pub(crate) fn set_default_field(
        &mut self,
        field: decide::DefaultField,
        on: bool,
        cx: &mut Context<Self>,
    ) {
        self.change_offer(
            |offer| {
                let before = field.get(&offer.defaults.policy);
                field.set(&mut offer.defaults.policy, on);
                before != on
            },
            cx,
        );
    }

    pub(crate) fn set_default_battery(&mut self, percent: Option<u8>, cx: &mut Context<Self>) {
        self.change_offer(
            |offer| {
                let changed = offer.defaults.battery_stop_percent != percent;
                offer.defaults.battery_stop_percent = percent;
                changed
            },
            cx,
        );
    }

    pub(crate) fn battery_input(
        &mut self,
        percent: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.keep_awake.battery_input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(percent.to_string()));
        self.keep_awake.battery_input = Some(input.clone());
        input
    }

    pub(crate) fn set_lid_limit(&mut self, minutes: Option<u32>, cx: &mut Context<Self>) {
        self.keep_awake_send(
            RequestBody::SetLidBatteryLimit {
                limit: decide::lid_limit(minutes),
            },
            cx,
        );
    }

    // ---- One rule -------------------------------------------------------

    pub(crate) fn set_rule_enabled(&mut self, rule_id: u64, enabled: bool, cx: &mut Context<Self>) {
        self.keep_awake_send(RequestBody::SetRuleEnabled { rule_id, enabled }, cx);
    }

    pub(crate) fn set_rule_priority(&mut self, rule_id: u64, priority: u8, cx: &mut Context<Self>) {
        self.keep_awake_send(RequestBody::SetRulePriority { rule_id, priority }, cx);
    }

    pub(crate) fn duplicate_rule(&mut self, rule_id: u64, cx: &mut Context<Self>) {
        self.keep_awake_send(RequestBody::DuplicateRule { rule_id }, cx);
    }

    pub(crate) fn move_rule(&mut self, rule_id: u64, delta: isize, cx: &mut Context<Self>) {
        let rules = self.keep_awake.rule_views(self.locale);
        let Some(index) = rules.iter().position(|rule| rule.rule_id == rule_id) else {
            return;
        };
        let target = index as isize + delta;
        if target < 0 || target as usize >= rules.len() {
            return;
        }
        self.keep_awake_send(
            RequestBody::ReorderRule {
                rule_id,
                to_index: target as u32,
            },
            cx,
        );
    }

    pub(crate) fn ask_delete_rule(&mut self, rule_id: u64, name: String, cx: &mut Context<Self>) {
        self.keep_awake.dialog = Some(Dialog::DeleteRule { rule_id, name });
        cx.notify();
    }

    pub(crate) fn delete_rule(&mut self, rule_id: u64, cx: &mut Context<Self>) {
        self.keep_awake.dialog = None;
        self.keep_awake.open_rules.remove(&rule_id);
        self.keep_awake_send(RequestBody::DeleteRule { rule_id }, cx);
    }

    pub(crate) fn priority_input(
        &mut self,
        rule_id: u64,
        priority: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        self.keep_awake
            .priority_inputs
            .entry(rule_id)
            .or_insert_with(|| {
                cx.new(|cx| InputState::new(window, cx).default_value(priority.to_string()))
            })
            .clone()
    }

    /// Tests one rule and keeps the answer under it. Acquires nothing.
    pub(crate) fn test_keep_awake_rule(&mut self, rule_id: u64, cx: &mut Context<Self>) {
        self.keep_awake.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    match Target::session() {
                        Ok(target) => bus::test_rule(&target, rule_id),
                        Err(error) => Err(Outcome::Failed(error)),
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.keep_awake.busy = false;
                let w = page_copy(app.locale);
                match outcome {
                    Ok(document) => {
                        app.keep_awake.failure = None;
                        app.keep_awake.test = Some(RuleTestView::from_document(&document));
                    }
                    Err(Outcome::Refused(key)) => {
                        app.keep_awake.test = None;
                        app.keep_awake.failure = Some((w.service_refused, key));
                    }
                    Err(Outcome::Failed(details)) => {
                        app.keep_awake.test = None;
                        app.keep_awake.failure = Some((w.action_failed, details));
                    }
                    Err(Outcome::Done) => app.keep_awake.test = None,
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---- The rule editor --------------------------------------------------

    pub(crate) fn edit_rule(&mut self, rule_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rule) = self.keep_awake.raw_rule(rule_id).cloned() else {
            return;
        };
        let name =
            cx.new(|cx| InputState::new(window, cx).default_value(rule.name.as_str().to_string()));
        let priority_input =
            cx.new(|cx| InputState::new(window, cx).default_value(rule.priority.to_string()));
        let groups = rule
            .groups
            .iter()
            .map(|group| DraftGroup {
                combine: group.combine,
                conditions: group
                    .conditions
                    .iter()
                    .cloned()
                    .map(|condition| draft_condition(condition, window, cx))
                    .collect(),
            })
            .collect();
        self.keep_awake.draft = Some(RuleDraft {
            rule_id: Some(rule_id),
            name,
            enabled: rule.enabled,
            priority: rule.priority,
            priority_input,
            combine: rule.combine,
            groups,
            policy: rule.policy,
            battery_stop_percent: rule.battery_stop_percent,
            error: None,
        });
        self.content_scroll.set_offset(gpui::Point::default());
        cx.notify();
    }

    pub(crate) fn new_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = copy(self.locale);
        let name = cx.new(|cx| InputState::new(window, cx).default_value(c.new_rule));
        let priority_input =
            cx.new(|cx| InputState::new(window, cx).default_value(DEFAULT_PRIORITY.to_string()));
        // A new rule starts with a condition that needs no typed operand, so
        // the editor opens on something that can be saved.
        let condition = draft_condition(Condition::AcPower { connected: true }, window, cx);
        self.keep_awake.sub = SubPage::Rules;
        self.keep_awake.draft = Some(RuleDraft {
            rule_id: None,
            name,
            enabled: true,
            priority: DEFAULT_PRIORITY,
            priority_input,
            combine: Combine::All,
            groups: vec![DraftGroup {
                combine: Combine::All,
                conditions: vec![condition],
            }],
            // Rules leave the lid alone unless they say so (ticket 109).
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: self.offer_or_shipped().defaults.battery_stop_percent,
            error: None,
        });
        self.content_scroll.set_offset(gpui::Point::default());
        cx.notify();
    }

    pub(crate) fn close_draft(&mut self, cx: &mut Context<Self>) {
        self.keep_awake.draft = None;
        cx.notify();
    }

    pub(crate) fn update_draft(
        &mut self,
        change: impl FnOnce(&mut RuleDraft),
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.keep_awake.draft.as_mut() {
            change(draft);
            draft.error = None;
        }
        cx.notify();
    }

    pub(crate) fn update_condition(
        &mut self,
        group: usize,
        index: usize,
        change: impl FnOnce(&mut Condition),
        cx: &mut Context<Self>,
    ) {
        self.update_draft(
            |draft| {
                if let Some(condition) = draft
                    .groups
                    .get_mut(group)
                    .and_then(|group| group.conditions.get_mut(index))
                {
                    change(&mut condition.condition);
                }
            },
            cx,
        );
    }

    pub(crate) fn add_condition(
        &mut self,
        group: usize,
        provider: awake_core::ProviderKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let condition = draft_condition(default_condition(provider), window, cx);
        self.update_draft(
            |draft| {
                if let Some(group) = draft.groups.get_mut(group)
                    && group.conditions.len() < awake_core::MAX_CONDITIONS_PER_GROUP
                {
                    group.conditions.push(condition);
                }
            },
            cx,
        );
    }

    pub(crate) fn remove_condition(&mut self, group: usize, index: usize, cx: &mut Context<Self>) {
        self.update_draft(
            |draft| {
                if let Some(group) = draft.groups.get_mut(group)
                    // A group with nothing in it is vacuously true, which the
                    // core refuses.
                    && group.conditions.len() > 1
                    && index < group.conditions.len()
                {
                    group.conditions.remove(index);
                }
            },
            cx,
        );
    }

    pub(crate) fn add_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let condition = draft_condition(Condition::AcPower { connected: true }, window, cx);
        self.update_draft(
            |draft| {
                if draft.groups.len() < awake_core::MAX_GROUPS_PER_RULE {
                    draft.groups.push(DraftGroup {
                        combine: Combine::All,
                        conditions: vec![condition],
                    });
                }
            },
            cx,
        );
    }

    pub(crate) fn remove_group(&mut self, index: usize, cx: &mut Context<Self>) {
        self.update_draft(
            |draft| {
                if draft.groups.len() > 1 && index < draft.groups.len() {
                    draft.groups.remove(index);
                }
            },
            cx,
        );
    }

    pub(crate) fn save_draft(&mut self, cx: &mut Context<Self>) {
        let rule = match self.draft_rule(cx) {
            Ok(rule) => rule,
            Err(message) => {
                if let Some(draft) = self.keep_awake.draft.as_mut() {
                    draft.error = Some(message);
                }
                cx.notify();
                return;
            }
        };
        let body = match self
            .keep_awake
            .draft
            .as_ref()
            .and_then(|draft| draft.rule_id)
        {
            Some(rule_id) => {
                self.keep_awake.priority_inputs.remove(&rule_id);
                RequestBody::UpdateRule {
                    rule_id,
                    rule: Box::new(rule),
                }
            }
            None => RequestBody::CreateRule {
                rule: Box::new(rule),
            },
        };
        self.keep_awake.draft = None;
        self.keep_awake_send(body, cx);
    }

    /// The draft as a rule, or why it cannot be one, in words.
    pub(crate) fn draft_rule(&self, cx: &gpui::App) -> Result<Rule, String> {
        let c = copy(self.locale);
        let draft = self
            .keep_awake
            .draft
            .as_ref()
            .ok_or_else(|| c.unknown.to_string())?;
        let invalid = |error: awake_core::RuleError| format!("{}: {error}", c.rule_invalid_input);
        let name = Reason::new(draft.name.read(cx).value().to_string())
            .map_err(|error| format!("{}: {error}", c.rule_invalid_input))?;
        let mut groups = Vec::new();
        for group in &draft.groups {
            let mut conditions = Vec::new();
            for condition in &group.conditions {
                conditions.push(resolved_condition(condition, cx).map_err(invalid)?);
            }
            groups.push(ConditionGroup::new(group.combine, conditions).map_err(invalid)?);
        }
        let mut rule = Rule::new(
            RuleId(draft.rule_id.unwrap_or(1)),
            name,
            draft.combine,
            groups,
        )
        .map_err(invalid)?;
        rule.enabled = draft.enabled;
        rule.priority = draft
            .priority_input
            .read(cx)
            .value()
            .trim()
            .parse::<u8>()
            .unwrap_or(draft.priority);
        rule.policy = draft.policy;
        rule.battery_stop_percent = draft.battery_stop_percent;
        rule.validate().map_err(invalid)?;
        Ok(rule)
    }
}

/// A condition with the fields it is edited in.
pub(crate) fn draft_condition<T>(
    condition: Condition,
    window: &mut Window,
    cx: &mut Context<T>,
) -> DraftCondition {
    let text = match &condition {
        Condition::ProcessRunning { matcher } => Some(matcher.as_str().to_string()),
        Condition::NetworkInterfaceUp { interface } => Some(interface.as_str().to_string()),
        Condition::WatchedPathActive { path, .. } => Some(path.as_path().display().to_string()),
        _ => None,
    };
    let numbers: Vec<String> = match &condition {
        Condition::BatteryPercent { at_least, at_most } => {
            vec![at_least.to_string(), at_most.to_string()]
        }
        Condition::CpuUtilizationAtLeast { percent } => vec![percent.to_string()],
        Condition::NetworkThroughputAtLeast {
            kibibytes_per_second,
        } => vec![kibibytes_per_second.to_string()],
        Condition::WatchedPathActive { within_seconds, .. } => vec![within_seconds.to_string()],
        _ => Vec::new(),
    };
    DraftCondition {
        condition,
        text: text.map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value))),
        numbers: numbers
            .into_iter()
            .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value)))
            .collect(),
    }
}

/// A condition with its typed operands read back out of their fields and
/// checked by `awake-core`, so the service never has to refuse what the
/// editor could see.
pub(crate) fn resolved_condition(
    draft: &DraftCondition,
    cx: &gpui::App,
) -> Result<Condition, awake_core::RuleError> {
    let typed = draft
        .text
        .as_ref()
        .map(|input| input.read(cx).value().to_string());
    let number = |index: usize| -> Option<u64> {
        draft
            .numbers
            .get(index)
            .and_then(|input| input.read(cx).value().trim().parse::<u64>().ok())
    };
    let percent = |index: usize, fallback: u8| -> u8 {
        number(index)
            .map(|value| value.min(100) as u8)
            .unwrap_or(fallback)
    };
    Ok(match &draft.condition {
        Condition::ProcessRunning { matcher } => Condition::ProcessRunning {
            matcher: ProcessMatcher::new(matcher.kind, typed.unwrap_or_default())?,
        },
        Condition::NetworkInterfaceUp { .. } => Condition::NetworkInterfaceUp {
            interface: InterfaceName::new(typed.unwrap_or_default())?,
        },
        Condition::WatchedPathActive { within_seconds, .. } => Condition::WatchedPathActive {
            path: WatchedPath::new(typed.unwrap_or_default())?,
            within_seconds: number(0).unwrap_or(*within_seconds),
        },
        Condition::BatteryPercent { at_least, at_most } => Condition::BatteryPercent {
            at_least: percent(0, *at_least),
            at_most: percent(1, *at_most),
        },
        Condition::CpuUtilizationAtLeast { percent: current } => Condition::CpuUtilizationAtLeast {
            percent: percent(0, *current),
        },
        Condition::NetworkThroughputAtLeast {
            kibibytes_per_second,
        } => Condition::NetworkThroughputAtLeast {
            kibibytes_per_second: number(0).unwrap_or(*kibibytes_per_second),
        },
        condition => condition.clone(),
    })
}

/// The condition a provider adds, with an operand that already validates.
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
