//! The compact popup menu, as a value.
//!
//! The menu is built from a status document and nothing else, so both layouts
//! Issue #13 draws can be asserted without a panel, a bus, or a running
//! service. `dbusmenu.rs` turns this tree into the wire format; nothing in this
//! file knows that D-Bus exists.

use awake_core::PolicyGap;
use awake_ipc::{
    StatusDocument, WireEnd, WireIndicator, WireRemaining, WireSession, WireSuppression,
};

use crate::labels::{Labels, Locale};
use crate::localtime::{UtcOffset, clock_time};

/// The lengths offered by Extend session.
pub const EXTEND_MINUTES: [u64; 3] = [15, 30, 60];

/// The battery threshold the `低於 20% 電量時停止` wording promises.
pub const QUICK_BATTERY_PERCENT: u8 = 20;

/// The two toggles in Quick options. They shape the next session the tray
/// starts; they never change anything on their own, which is why toggling one
/// is not a request to the service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuickOptions {
    /// Checked means the display may still turn off, which is the default.
    pub allow_display_off: bool,
    pub stop_below_battery: bool,
}

impl Default for QuickOptions {
    fn default() -> Self {
        Self {
            allow_display_off: true,
            stop_below_battery: true,
        }
    }
}

impl QuickOptions {
    pub fn policy(&self) -> awake_core::SessionPolicy {
        awake_core::SessionPolicy {
            prevent_display_sleep: !self.allow_display_off,
            ..awake_core::SessionPolicy::quick_default()
        }
    }

    pub fn battery_stop_percent(&self) -> Option<u8> {
        self.stop_below_battery.then_some(QUICK_BATTERY_PERCENT)
    }
}

/// What activating an item asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    StartIndefinite,
    StartMinutes(u64),
    /// Needs a time picker, which lives in the full window.
    StartUntilTime,
    ToggleAllowDisplayOff,
    ToggleStopBelowBattery,
    ExtendMinutes(u64),
    /// Needs the full window for the same reason.
    ChangeSession,
    /// Ends the manual session and leaves every rule's session running.
    EndSession,
    /// Pauses every rule. `None` means until resumed.
    PauseRules(Option<u64>),
    ResumeRules,
    /// The first half of the override. Changes the label and nothing else.
    ArmOverrideAllRules,
    /// The second half. Only this one reaches the service.
    ConfirmOverrideAllRules,
    OpenApplication,
    Quit,
}

/// The two pause lengths the tray offers, in seconds. The service refuses any
/// other length, so the menu offers exactly these.
pub const PAUSE_SHORT_SECONDS: u64 = awake_core::PAUSE_SHORT_SECONDS;
pub const PAUSE_LONG_SECONDS: u64 = awake_core::PAUSE_LONG_SECONDS;

/// How long an armed override stays armed, in seconds.
pub const OVERRIDE_ARM_SECONDS: u64 = 10;

/// The tray's stand-in for a confirmation dialog.
///
/// `OverrideAllRules { confirmed: true }` switches every automatic rule off
/// until someone turns them back on, and the protocol deliberately makes that
/// flag un-omittable so a client cannot send it by accident. A panel menu has
/// nowhere to show a modal, so the tray cannot ask the question the flag stands
/// for — and sending it straight from one activation would mean a single stray
/// click, a mis-aimed pointer or a host replaying an event, silently disables
/// the whole rule set.
///
/// So the control is armed first. The unarmed item carries
/// [`MenuAction::ArmOverrideAllRules`], which asks the service for nothing and
/// only changes the label; the confirming item exists in the menu at all only
/// while the arming is live, and the controller checks the arming again before
/// it builds the request. Two independent checks, so neither a stale cached
/// layout nor a duplicated activation is enough on its own.
///
/// The arming is dropped by any other menu action and expires after
/// [`OVERRIDE_ARM_SECONDS`], so an arm someone walked away from cannot be
/// completed by an unrelated click later. Time is passed in rather than read,
/// which is what keeps the whole thing testable without a clock.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverrideConfirmation {
    /// Monotonic seconds since the tray started, not a wall clock: a clock
    /// stepping backwards must not extend an arming.
    armed_at: Option<u64>,
}

impl OverrideConfirmation {
    pub fn arm(&mut self, now_seconds: u64) {
        self.armed_at = Some(now_seconds);
    }

    pub fn clear(&mut self) {
        self.armed_at = None;
    }

    pub fn is_armed(self, now_seconds: u64) -> bool {
        self.armed_at
            .is_some_and(|at| now_seconds.saturating_sub(at) < OVERRIDE_ARM_SECONDS)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemKind {
    /// A plain activatable entry.
    Standard,
    Separator,
    Checkmark {
        checked: bool,
    },
    /// A line that states something. Never activatable.
    Info,
    Submenu,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuItem {
    pub id: i32,
    pub label: String,
    pub kind: ItemKind,
    pub enabled: bool,
    pub action: Option<MenuAction>,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    fn standard(label: impl Into<String>, action: MenuAction) -> Self {
        Self {
            id: 0,
            label: label.into(),
            kind: ItemKind::Standard,
            enabled: true,
            action: Some(action),
            children: Vec::new(),
        }
    }

    fn info(label: impl Into<String>) -> Self {
        Self {
            id: 0,
            label: label.into(),
            kind: ItemKind::Info,
            enabled: false,
            action: None,
            children: Vec::new(),
        }
    }

    fn separator() -> Self {
        Self {
            id: 0,
            label: String::new(),
            kind: ItemKind::Separator,
            enabled: true,
            action: None,
            children: Vec::new(),
        }
    }

    fn submenu(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        Self {
            id: 0,
            label: label.into(),
            kind: ItemKind::Submenu,
            enabled: true,
            action: None,
            children,
        }
    }

    fn checkmark(label: impl Into<String>, checked: bool, action: MenuAction) -> Self {
        Self {
            id: 0,
            label: label.into(),
            kind: ItemKind::Checkmark { checked },
            enabled: true,
            action: Some(action),
            children: Vec::new(),
        }
    }

    fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Depth-first walk, used by the wire layer and by tests.
    pub fn walk<'a>(&'a self, visit: &mut impl FnMut(&'a MenuItem)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }
}

/// A whole menu, with ids already assigned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Menu {
    pub items: Vec<MenuItem>,
}

impl Menu {
    /// Every label in the menu, flattened. Test and layout convenience.
    pub fn labels(&self) -> Vec<&str> {
        let mut labels = Vec::new();
        for item in &self.items {
            item.walk(&mut |item| {
                if item.kind != ItemKind::Separator {
                    labels.push(item.label.as_str());
                }
            });
        }
        labels
    }

    pub fn find(&self, id: i32) -> Option<&MenuItem> {
        let mut found = None;
        for item in &self.items {
            item.walk(&mut |candidate| {
                if candidate.id == id {
                    found = Some(candidate);
                }
            });
            if found.is_some() {
                return found;
            }
        }
        found
    }

    /// Finds an item by the text it shows. Used by the wire layer's tests and
    /// by anything that needs to point at a specific entry without hardcoding
    /// the id numbering.
    pub fn find_by_label(&self, label: &str) -> Option<&MenuItem> {
        let mut found = None;
        for item in &self.items {
            item.walk(&mut |candidate| {
                if candidate.label == label {
                    found = Some(candidate);
                }
            });
            if found.is_some() {
                return found;
            }
        }
        found
    }

    pub fn action(&self, id: i32) -> Option<MenuAction> {
        self.find(id).and_then(|item| item.action)
    }
}

/// Builds the popup for the state the service reported.
///
/// `override_armed` is the only thing here that is not read from the status: it
/// is the tray-local half of the two-step override, and it decides whether the
/// confirming item exists at all.
pub fn build(
    status: &StatusDocument,
    options: QuickOptions,
    locale: Locale,
    offset: UtcOffset,
    override_armed: bool,
) -> Menu {
    let labels = locale.labels();
    let mut items = if status.is_active() {
        active_items(status, labels, offset)
    } else {
        inactive_items(status, options, labels)
    };

    items.push(MenuItem::separator());
    items.push(automatic_rules(status, labels, offset, override_armed));
    items.push(MenuItem::standard(
        labels.open_application,
        MenuAction::OpenApplication,
    ));
    items.push(MenuItem::standard(labels.quit, MenuAction::Quit));

    let mut menu = Menu { items };
    assign_ids(&mut menu);
    menu
}

fn inactive_items(
    status: &StatusDocument,
    options: QuickOptions,
    labels: &Labels,
) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::info(labels.application_name),
        MenuItem::info(labels.inactive_summary),
    ];
    items.extend(problem_items(status, labels));
    items.push(MenuItem::separator());

    let backend_ready = status.backend.available;
    let mut start = MenuItem::submenu(
        labels.start_a_session,
        vec![
            MenuItem::standard(labels.indefinitely, MenuAction::StartIndefinite),
            MenuItem::standard(labels.minutes_15, MenuAction::StartMinutes(15)),
            MenuItem::standard(labels.minutes_30, MenuAction::StartMinutes(30)),
            MenuItem::standard(labels.hour_1, MenuAction::StartMinutes(60)),
            MenuItem::standard(labels.hours_2, MenuAction::StartMinutes(120)),
            MenuItem::standard(labels.until_a_time, MenuAction::StartUntilTime),
        ],
    );
    if !backend_ready {
        // Offering a one-click session that cannot be enforced would be the
        // silent failure Issue #13 forbids.
        start.enabled = false;
        for child in &mut start.children {
            child.enabled = false;
        }
    }
    items.push(start);

    items.push(MenuItem::separator());
    items.push(MenuItem::submenu(
        labels.quick_options,
        vec![
            MenuItem::checkmark(
                labels.allow_display_off,
                options.allow_display_off,
                MenuAction::ToggleAllowDisplayOff,
            ),
            MenuItem::checkmark(
                labels.stop_below_battery,
                options.stop_below_battery,
                MenuAction::ToggleStopBelowBattery,
            ),
        ],
    ));
    items
}

fn active_items(status: &StatusDocument, labels: &Labels, offset: UtcOffset) -> Vec<MenuItem> {
    let session = status
        .manual_session()
        .or_else(|| status.sessions.first())
        .expect("an active status carries at least one session");

    let mut items = vec![
        MenuItem::info(labels.active_summary),
        MenuItem::info(format!("{}: {}", labels.reason, session.reason)),
        MenuItem::info(format!(
            "{}: {}",
            labels.remaining,
            remaining_text(session, labels)
        )),
        MenuItem::info(format!(
            "{}: {}",
            labels.started,
            clock_time(session.started_at_unix_seconds, offset)
        )),
    ];

    // More than one reason is why the machine stays awake after one of them
    // ends, so it is stated rather than left to be discovered.
    if status.reasons.len() > 1 {
        items.push(MenuItem::info(
            labels
                .active_reasons
                .replace("{count}", &status.reasons.len().to_string()),
        ));
        for reason in &status.reasons {
            items.push(MenuItem::info(format!("• {}", reason.reason)));
        }
    }
    items.extend(problem_items(status, labels));

    items.push(MenuItem::separator());
    items.push(policy_row(
        labels.system_sleep,
        status.effective_policy.prevent_system_suspend,
        status.unmet_policy.contains(&PolicyGap::SystemSuspend),
        labels,
    ));
    items.push(policy_row(
        labels.display_sleep,
        status.effective_policy.prevent_display_sleep,
        status.unmet_policy.contains(&PolicyGap::DisplaySleep),
        labels,
    ));
    items.push(policy_row(
        labels.automatic_lock,
        status.effective_policy.prevent_automatic_lock,
        status.unmet_policy.contains(&PolicyGap::AutomaticLock),
        labels,
    ));
    items.push(MenuItem::info(format!(
        "{}: {}",
        labels.battery_protection,
        match status.battery_stop_percent {
            Some(percent) => labels
                .battery_stops_at
                .replace("{percent}", &percent.to_string()),
            None => labels.battery_off.to_string(),
        }
    )));

    items.push(MenuItem::separator());
    let mut extend = MenuItem::submenu(
        labels.extend_session,
        EXTEND_MINUTES
            .iter()
            .map(|minutes| {
                MenuItem::standard(
                    duration_label(minutes * 60, labels),
                    MenuAction::ExtendMinutes(*minutes),
                )
            })
            .collect(),
    );
    if session.end == WireEnd::Indefinite {
        // There is no end to push out, and the state machine would refuse, so
        // the menu says so rather than offering an action that always fails.
        extend = extend.disabled();
        for child in &mut extend.children {
            child.enabled = false;
        }
    }
    items.push(extend);
    items.push(MenuItem::standard(
        labels.change_session,
        MenuAction::ChangeSession,
    ));
    if status.manual_session().is_some() {
        // End session ends the manual session and nothing else. When a rule is
        // also holding a session the label says so, because "End session" next
        // to a machine that stays awake afterwards reads as a failure.
        let end = if status.active_rules.is_empty() {
            labels.end_session
        } else {
            labels.end_manual_session_keep_rules
        };
        items.push(MenuItem::standard(end, MenuAction::EndSession));
    }
    items
}

/// The rows that explain a degraded state, shown in both layouts.
fn problem_items(status: &StatusDocument, labels: &Labels) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if status.indicator == WireIndicator::Unavailable || !status.backend.available {
        items.push(MenuItem::info(labels.backend_unavailable));
    }
    if status.attention.is_some() {
        items.push(MenuItem::info(labels.attention));
    }
    if status.interrupted_previous_session.is_some() {
        items.push(MenuItem::info(labels.interrupted_previous_session));
    }
    items
}

fn policy_row(name: &str, prevented: bool, unmet: bool, labels: &Labels) -> MenuItem {
    let value = if unmet {
        // The session asked, the backend cannot, so this says neither
        // "Prevented" nor a bare "Allowed".
        labels.not_supported
    } else if prevented {
        labels.prevented
    } else {
        labels.allowed
    };
    MenuItem::info(format!("{name}: {value}"))
}

/// What the `Automatic rules` line reports, from the suppression first and the
/// rule counts second. A pause applies whatever the counts say, so it is read
/// first; "no rules yet" and "every rule switched off" are kept apart, because
/// one of them is answered by writing a rule and the other by switching one on.
fn rules_state(status: &StatusDocument, labels: &Labels, offset: UtcOffset) -> String {
    match status.rules_suppression {
        Some(WireSuppression::PausedUntil { unix_seconds }) => labels
            .rules_paused_until
            .replace("{time}", &clock_time(unix_seconds, offset)),
        Some(WireSuppression::PausedUntilResumed) => labels.rules_paused_until_resumed.to_string(),
        Some(WireSuppression::Overridden) => labels.rules_overridden.to_string(),
        None if status.rule_summary.total == 0 => labels.no_rules_yet.to_string(),
        None if status.rule_summary.enabled == 0 => labels.rules_off.to_string(),
        None => labels.rules_on.to_string(),
    }
}

/// The rule section: what the rules are doing, and the controls that change it.
///
/// With nothing to control — no rule written yet, or every rule switched off
/// and nothing suspended — this is one stated line rather than a submenu whose
/// Pause and Override would act on nothing.
fn automatic_rules(
    status: &StatusDocument,
    labels: &Labels,
    offset: UtcOffset,
    override_armed: bool,
) -> MenuItem {
    let header = format!(
        "{}: {}",
        labels.automatic_rules,
        rules_state(status, labels, offset)
    );
    let suppressed = status.rules_suppression.is_some();
    if !suppressed && status.rule_summary.enabled == 0 {
        return MenuItem::info(header);
    }

    let mut children = Vec::new();
    if status.active_rules.is_empty() {
        if !suppressed {
            children.push(MenuItem::info(labels.no_rules_match));
        }
    } else {
        // By name. A session id says nothing about which rule is holding the
        // machine awake, which is the question this list answers.
        for rule in &status.active_rules {
            children.push(MenuItem::info(format!("• {}", rule.name)));
        }
    }
    if !children.is_empty() {
        children.push(MenuItem::separator());
    }

    if suppressed {
        children.push(MenuItem::standard(
            labels.resume_automatic_rules,
            MenuAction::ResumeRules,
        ));
    } else {
        children.push(MenuItem::submenu(
            labels.pause_automatic_rules,
            vec![
                MenuItem::standard(
                    labels.pause_15_minutes,
                    MenuAction::PauseRules(Some(PAUSE_SHORT_SECONDS)),
                ),
                MenuItem::standard(
                    labels.pause_1_hour,
                    MenuAction::PauseRules(Some(PAUSE_LONG_SECONDS)),
                ),
                MenuItem::standard(labels.pause_until_resumed, MenuAction::PauseRules(None)),
            ],
        ));
        children.push(if override_armed {
            MenuItem::standard(
                labels.confirm_override_all_rules,
                MenuAction::ConfirmOverrideAllRules,
            )
        } else {
            MenuItem::standard(labels.override_all_rules, MenuAction::ArmOverrideAllRules)
        });
    }

    MenuItem::submenu(header, children)
}

fn remaining_text(session: &WireSession, labels: &Labels) -> String {
    match session.remaining {
        WireRemaining::UntilEnded => labels.until_ended.to_string(),
        WireRemaining::Seconds { seconds } => duration_label(seconds, labels),
        // The service reaps on its next tick; saying "0" is more honest than
        // showing a negative countdown.
        WireRemaining::Elapsed => duration_label(0, labels),
    }
}

/// A short, rounded duration. Panel menus have no room for `01:29:57`, and a
/// second-by-second countdown would need a timer the tray deliberately does not
/// run.
pub fn duration_label(seconds: u64, labels: &Labels) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    match (hours, minutes) {
        (0, 0) => format!("{} {}", seconds % 60, labels.second_unit),
        (0, minutes) => format!("{minutes} {}", labels.minute_unit),
        (hours, 0) => format!("{hours} {}", labels.hour_unit),
        (hours, minutes) => format!(
            "{hours} {} {minutes} {}",
            labels.hour_unit, labels.minute_unit
        ),
    }
}

/// Numbers every item depth-first, starting at 1. Zero is the dbusmenu root.
fn assign_ids(menu: &mut Menu) {
    let mut next = 1;
    fn number(item: &mut MenuItem, next: &mut i32) {
        item.id = *next;
        *next += 1;
        for child in &mut item.children {
            number(child, next);
        }
    }
    for item in &mut menu.items {
        number(item, &mut next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::menu_request;
    use awake_core::{BackendCapabilities, SessionOrigin, SessionPolicy};
    use awake_ipc::{
        RequestBody, WireActiveRule, WireBackend, WireBatteryProtection, WireInterrupted,
        WireReason, WireRuleSummary,
    };

    const NOW: u64 = 1_700_000_000;

    fn backend(available: bool) -> WireBackend {
        WireBackend {
            name: "logind".to_string(),
            available,
            capabilities: BackendCapabilities {
                system_suspend: true,
                idle: true,
                display_sleep: false,
                automatic_lock: false,
            },
            detail: (!available).then(|| "awake.backend.unavailable:no logind".to_string()),
        }
    }

    fn inactive() -> StatusDocument {
        StatusDocument {
            indicator: WireIndicator::Inactive,
            effective_policy: SessionPolicy::default(),
            unmet_policy: Vec::new(),
            battery_stop_percent: None,
            sessions: Vec::new(),
            reasons: Vec::new(),
            backend: backend(true),
            attention: None,
            interrupted_previous_session: None,
            reduced_security_confirmed: false,
            active_rules: Vec::new(),
            rule_summary: WireRuleSummary::default(),
            rules_suppression: None,
            conflicts: Vec::new(),
            providers: Vec::new(),
            battery_protection: WireBatteryProtection::default(),
            now_unix_seconds: NOW,
        }
    }

    /// A status with rules written and switched on, and none of them matching.
    fn with_rules(total: u32, enabled: u32) -> StatusDocument {
        StatusDocument {
            rule_summary: WireRuleSummary {
                total,
                enabled,
                refused: 0,
            },
            ..inactive()
        }
    }

    fn active_rule(rule_id: u64, name: &str) -> WireActiveRule {
        WireActiveRule {
            rule_id,
            name: name.to_string(),
            session_id: rule_id + 10,
            priority: 50,
        }
    }

    /// Every action the menu can produce, depth-first.
    fn actions(menu: &Menu) -> Vec<MenuAction> {
        let mut found = Vec::new();
        for item in &menu.items {
            item.walk(&mut |candidate| {
                if let Some(action) = candidate.action {
                    found.push(action);
                }
            });
        }
        found
    }

    fn session(end: WireEnd, remaining: WireRemaining) -> WireSession {
        WireSession {
            session_id: 1,
            reason: "Android Studio build is running".to_string(),
            origin: SessionOrigin::Manual,
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
            end,
            started_at_unix_seconds: NOW,
            remaining,
        }
    }

    fn active(end: WireEnd, remaining: WireRemaining) -> StatusDocument {
        let session = session(end, remaining);
        StatusDocument {
            indicator: WireIndicator::ActiveManual,
            effective_policy: SessionPolicy::quick_default(),
            unmet_policy: Vec::new(),
            battery_stop_percent: Some(20),
            reasons: vec![WireReason {
                session_id: 1,
                origin: SessionOrigin::Manual,
                reason: session.reason.clone(),
            }],
            sessions: vec![session],
            ..inactive()
        }
    }

    fn menu(status: &StatusDocument, locale: Locale) -> Menu {
        build(
            status,
            QuickOptions::default(),
            locale,
            UtcOffset::UTC,
            false,
        )
    }

    fn armed_menu(status: &StatusDocument, locale: Locale) -> Menu {
        build(
            status,
            QuickOptions::default(),
            locale,
            UtcOffset::UTC,
            true,
        )
    }

    #[test]
    fn the_inactive_menu_matches_the_layout_issue_13_draws() {
        let menu = menu(&inactive(), Locale::EnUs);
        assert_eq!(
            menu.labels(),
            vec![
                "Better Awake",
                "Not keeping this computer awake",
                "Start a session",
                "Indefinitely",
                "15 minutes",
                "30 minutes",
                "1 hour",
                "2 hours",
                "Until…",
                "Quick options",
                "Allow display to turn off",
                "Stop below 20% battery",
                "Automatic rules: No rules yet",
                "Open Better Awake…",
                "Quit Better Awake",
            ]
        );
    }

    #[test]
    fn the_inactive_menu_starts_a_session_in_one_action() {
        let menu = menu(&inactive(), Locale::EnUs);
        let start = menu
            .items
            .iter()
            .find(|item| item.label == "Start a session")
            .unwrap();

        assert_eq!(
            start
                .children
                .iter()
                .map(|child| child.action.unwrap())
                .collect::<Vec<_>>(),
            vec![
                MenuAction::StartIndefinite,
                MenuAction::StartMinutes(15),
                MenuAction::StartMinutes(30),
                MenuAction::StartMinutes(60),
                MenuAction::StartMinutes(120),
                MenuAction::StartUntilTime,
            ]
        );
    }

    #[test]
    fn the_zh_tw_menu_uses_the_wording_the_issue_fixes() {
        let menu = menu(&with_rules(2, 2), Locale::ZhTw);
        let labels = menu.labels();
        for expected in [
            "保持清醒",
            "目前未保持清醒",
            "開始一段工作階段",
            "持續保持清醒",
            "直到指定時間",
            "允許螢幕關閉",
            "低於 20% 電量時停止",
            "自動規則: 已啟用",
            "暫停自動規則",
            "暫停 15 分鐘",
            "暫停 1 小時",
            "暫停到手動恢復",
            "覆寫所有自動規則",
            "開啟 Better Awake…",
        ] {
            assert!(labels.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn the_active_menu_shows_the_reason_the_remaining_time_and_the_start() {
        let status = active(
            WireEnd::Duration { seconds: 7_200 },
            WireRemaining::Seconds { seconds: 5_400 },
        );
        let menu = menu(&status, Locale::EnUs);
        let labels = menu.labels();

        assert_eq!(labels[0], "Keeping this computer awake");
        assert_eq!(labels[1], "Reason: Android Studio build is running");
        assert_eq!(labels[2], "Remaining: 1 hr 30 min");
        assert_eq!(labels[3], "Started: 22:13");
    }

    #[test]
    fn an_indefinite_session_says_until_ended_instead_of_inventing_a_time() {
        let status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        let menu = menu(&status, Locale::EnUs);
        assert!(menu.labels().contains(&"Remaining: Until ended"));

        let extend = menu
            .items
            .iter()
            .find(|item| item.label == "Extend session")
            .unwrap();
        assert!(
            !extend.enabled,
            "there is no end to extend, so the action is not offered"
        );
    }

    #[test]
    fn the_active_menu_reports_the_effective_sleep_display_and_lock_policy() {
        let status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        let menu = menu(&status, Locale::EnUs);
        let labels = menu.labels();

        assert!(labels.contains(&"System sleep: Prevented"));
        assert!(labels.contains(&"Display sleep: Allowed"));
        assert!(labels.contains(&"Automatic lock: Allowed"));
        assert!(labels.contains(&"Battery protection: Stops at 20%"));
    }

    #[test]
    fn a_policy_the_backend_cannot_hold_is_shown_as_unsupported_not_as_in_force() {
        let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        status.effective_policy.prevent_display_sleep = true;
        status.unmet_policy = vec![PolicyGap::DisplaySleep];

        let menu = menu(&status, Locale::EnUs);
        let labels = menu.labels();
        assert!(labels.contains(&"Display sleep: Not supported here"));
        assert!(!labels.contains(&"Display sleep: Prevented"));
    }

    #[test]
    fn the_active_menu_can_extend_change_and_end() {
        let status = active(
            WireEnd::Duration { seconds: 7_200 },
            WireRemaining::Seconds { seconds: 60 },
        );
        let menu = menu(&status, Locale::EnUs);
        let actions: Vec<MenuAction> = menu
            .items
            .iter()
            .flat_map(|item| {
                let mut found = Vec::new();
                item.walk(&mut |candidate| {
                    if let Some(action) = candidate.action {
                        found.push(action);
                    }
                });
                found
            })
            .collect();

        assert!(actions.contains(&MenuAction::ExtendMinutes(15)));
        assert!(actions.contains(&MenuAction::ChangeSession));
        assert!(actions.contains(&MenuAction::EndSession));
    }

    #[test]
    fn several_reasons_are_listed_so_the_user_knows_why_it_is_still_awake() {
        let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        status.reasons.push(WireReason {
            session_id: 2,
            origin: SessionOrigin::Trigger,
            reason: "External display is connected".to_string(),
        });

        let menu = menu(&status, Locale::EnUs);
        let labels = menu.labels();
        assert!(labels.contains(&"2 active reasons"));
        assert!(labels.contains(&"• External display is connected"));
    }

    #[test]
    fn a_missing_backend_explains_itself_and_offers_no_session_it_cannot_keep() {
        let mut status = inactive();
        status.indicator = WireIndicator::Unavailable;
        status.backend = backend(false);

        let menu = menu(&status, Locale::EnUs);
        assert!(menu.labels().contains(&"No keep-awake support here"));
        let start = menu
            .items
            .iter()
            .find(|item| item.label == "Start a session")
            .unwrap();
        assert!(!start.enabled);
        assert!(start.children.iter().all(|child| !child.enabled));
    }

    #[test]
    fn an_interrupted_previous_session_is_explained_in_the_menu() {
        let mut status = inactive();
        status.interrupted_previous_session = Some(WireInterrupted {
            reason: "Build".to_string(),
            started_at_unix_seconds: NOW - 100,
            last_seen_unix_seconds: NOW - 10,
        });
        assert!(
            menu(&status, Locale::EnUs)
                .labels()
                .contains(&"Previous session ended abruptly")
        );
    }

    #[test]
    fn quick_options_shape_the_next_session_without_asking_the_service_anything() {
        let allowed = QuickOptions::default();
        assert!(!allowed.policy().prevent_display_sleep);
        assert_eq!(allowed.battery_stop_percent(), Some(20));

        let held_on = QuickOptions {
            allow_display_off: false,
            stop_below_battery: false,
        };
        assert!(held_on.policy().prevent_display_sleep);
        assert_eq!(held_on.battery_stop_percent(), None);
    }

    #[test]
    fn every_item_has_its_own_id_so_an_activation_names_one_action() {
        let menu = menu(
            &active(WireEnd::Indefinite, WireRemaining::UntilEnded),
            Locale::ZhTw,
        );
        let mut ids = Vec::new();
        for item in &menu.items {
            item.walk(&mut |candidate| ids.push(candidate.id));
        }
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "ids must be unique");
        assert!(ids.iter().all(|id| *id > 0), "0 is the dbusmenu root");
    }

    #[test]
    fn durations_round_to_something_a_panel_menu_can_show() {
        let labels = Locale::EnUs.labels();
        assert_eq!(duration_label(7_200, labels), "2 hr");
        assert_eq!(duration_label(5_400, labels), "1 hr 30 min");
        assert_eq!(duration_label(2_700, labels), "45 min");
        assert_eq!(duration_label(30, labels), "30 sec");

        let zh = Locale::ZhTw.labels();
        assert_eq!(duration_label(5_400, zh), "1 小時 30 分鐘");
    }

    // ---- Automatic rules ---------------------------------------------------

    #[test]
    fn a_status_with_two_active_rules_lists_both_rule_names_rather_than_session_ids() {
        let mut status = with_rules(3, 3);
        status.active_rules = vec![
            active_rule(1, "External display is connected"),
            active_rule(2, "Android Studio is running"),
        ];

        let menu = menu(&status, Locale::EnUs);
        let labels = menu.labels();
        assert!(labels.contains(&"• External display is connected"));
        assert!(labels.contains(&"• Android Studio is running"));
        assert!(
            !labels.iter().any(|label| label.contains("11")),
            "a session id is not an explanation: {labels:?}"
        );
    }

    #[test]
    fn the_automatic_rules_line_reads_on_when_nothing_suppresses_them() {
        let mut status = with_rules(3, 3);
        status.active_rules = vec![active_rule(1, "Presenting")];
        assert!(
            menu(&status, Locale::EnUs)
                .labels()
                .contains(&"Automatic rules: On")
        );
    }

    #[test]
    fn the_automatic_rules_line_reads_paused_while_a_pause_is_in_force() {
        let mut until = with_rules(3, 3);
        until.rules_suppression = Some(WireSuppression::PausedUntil {
            unix_seconds: NOW + 900,
        });
        assert!(
            menu(&until, Locale::EnUs)
                .labels()
                .contains(&"Automatic rules: Paused until 22:28"),
            "a pause with an end says when it ends"
        );

        let mut open_ended = with_rules(3, 3);
        open_ended.rules_suppression = Some(WireSuppression::PausedUntilResumed);
        assert!(
            menu(&open_ended, Locale::EnUs)
                .labels()
                .contains(&"Automatic rules: Paused")
        );
    }

    #[test]
    fn the_automatic_rules_line_reads_overridden_while_an_override_is_in_force() {
        let mut status = with_rules(3, 3);
        status.rules_suppression = Some(WireSuppression::Overridden);
        assert!(
            menu(&status, Locale::EnUs)
                .labels()
                .contains(&"Automatic rules: Overridden")
        );
    }

    #[test]
    fn a_paused_status_offers_resume_and_none_of_the_pause_choices() {
        let mut status = with_rules(3, 3);
        status.rules_suppression = Some(WireSuppression::PausedUntil {
            unix_seconds: NOW + 900,
        });
        let menu = menu(&status, Locale::EnUs);

        let actions = actions(&menu);
        assert!(actions.contains(&MenuAction::ResumeRules));
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, MenuAction::PauseRules(_))),
            "pausing an already paused rule set would say nothing true"
        );
        assert!(!actions.contains(&MenuAction::ArmOverrideAllRules));
        assert!(!menu.labels().contains(&"No rules match right now"));
    }

    #[test]
    fn a_running_rule_set_offers_the_three_pause_lengths_the_service_accepts() {
        let actions = actions(&menu(&with_rules(3, 3), Locale::EnUs));
        assert!(actions.contains(&MenuAction::PauseRules(Some(PAUSE_SHORT_SECONDS))));
        assert!(actions.contains(&MenuAction::PauseRules(Some(PAUSE_LONG_SECONDS))));
        assert!(actions.contains(&MenuAction::PauseRules(None)));
        assert!(!actions.contains(&MenuAction::ResumeRules));
    }

    #[test]
    fn a_rule_set_that_matches_nothing_says_so_instead_of_showing_an_empty_list() {
        let menu = menu(&with_rules(3, 3), Locale::EnUs);
        let labels = menu.labels();
        assert!(labels.contains(&"Automatic rules: On"));
        assert!(labels.contains(&"No rules match right now"));
    }

    #[test]
    fn a_status_with_no_rules_at_all_states_that_and_offers_no_control_over_nothing() {
        let menu = menu(&inactive(), Locale::EnUs);
        let labels = menu.labels();

        assert!(labels.contains(&"Automatic rules: No rules yet"));
        assert!(!labels.contains(&"No rules match right now"));
        assert!(!labels.contains(&"Pause automatic rules"));
        assert!(!labels.contains(&"Override all rules"));

        let actions = actions(&menu);
        assert!(
            !actions.iter().any(|action| matches!(
                action,
                MenuAction::PauseRules(_)
                    | MenuAction::ResumeRules
                    | MenuAction::ArmOverrideAllRules
                    | MenuAction::ConfirmOverrideAllRules
            )),
            "there is nothing to pause, resume, or override"
        );
    }

    #[test]
    fn rules_that_all_exist_but_are_switched_off_are_reported_as_off_not_as_missing() {
        let menu = menu(&with_rules(3, 0), Locale::EnUs);
        let labels = menu.labels();
        assert!(labels.contains(&"Automatic rules: Off"));
        assert!(!labels.contains(&"Automatic rules: No rules yet"));
        assert!(!labels.contains(&"Pause automatic rules"));
    }

    #[test]
    fn overriding_every_rule_takes_two_activations_and_one_alone_asks_for_nothing() {
        let status = with_rules(3, 3);

        let unarmed = menu(&status, Locale::EnUs);
        let control = unarmed
            .find_by_label("Override all rules")
            .expect("the override control is offered");
        assert_eq!(control.action, Some(MenuAction::ArmOverrideAllRules));
        assert!(
            unarmed
                .find_by_label("Click again to override all")
                .is_none()
        );

        // The whole unarmed menu, not just that one item: nothing anywhere in
        // it turns into the confirmed override.
        for action in actions(&unarmed) {
            assert_ne!(
                menu_request(action).map(|request| request.body),
                Some(RequestBody::OverrideAllRules { confirmed: true }),
                "one activation must never override every rule: {action:?}"
            );
        }

        // The second activation is offered only once the first has armed it.
        let armed = armed_menu(&status, Locale::EnUs);
        let confirm = armed
            .find_by_label("Click again to override all")
            .expect("the armed control shows the confirming wording");
        assert_eq!(confirm.action, Some(MenuAction::ConfirmOverrideAllRules));
        assert!(armed.find_by_label("Override all rules").is_none());
        assert_eq!(
            menu_request(MenuAction::ConfirmOverrideAllRules).map(|request| request.body),
            Some(RequestBody::OverrideAllRules { confirmed: true })
        );
    }

    #[test]
    fn an_arming_expires_so_a_forgotten_confirmation_cannot_be_completed_later() {
        let mut confirmation = OverrideConfirmation::default();
        assert!(!confirmation.is_armed(0), "nothing is armed to begin with");

        confirmation.arm(100);
        assert!(confirmation.is_armed(100));
        assert!(confirmation.is_armed(100 + OVERRIDE_ARM_SECONDS - 1));
        assert!(
            !confirmation.is_armed(100 + OVERRIDE_ARM_SECONDS),
            "an arm nobody confirmed must not stay live"
        );

        confirmation.arm(100);
        confirmation.clear();
        assert!(
            !confirmation.is_armed(100),
            "choosing anything else drops the arming"
        );
    }

    #[test]
    fn ending_a_session_while_a_rule_holds_one_says_the_rule_survives_it() {
        let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        status.rule_summary = WireRuleSummary {
            total: 2,
            enabled: 2,
            refused: 0,
        };
        status.active_rules = vec![active_rule(1, "External display is connected")];

        let menu = menu(&status, Locale::EnUs);
        assert!(menu.labels().contains(&"End session, keep rules"));
        assert_eq!(
            menu.find_by_label("End session, keep rules")
                .and_then(|item| item.action),
            Some(MenuAction::EndSession)
        );
    }

    #[test]
    fn a_session_that_belongs_only_to_a_rule_offers_no_end_session_to_aim_at_it() {
        let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        status.sessions[0].origin = SessionOrigin::Trigger;
        status.indicator = WireIndicator::ActiveTrigger;

        assert!(
            !actions(&menu(&status, Locale::EnUs)).contains(&MenuAction::EndSession),
            "there is no manual session to end"
        );
    }
}
