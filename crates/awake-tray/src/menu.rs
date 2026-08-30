//! The compact popup menu, as a value.
//!
//! The menu is built from a status document and nothing else, so both layouts
//! Issue #13 draws can be asserted without a panel, a bus, or a running
//! service. `dbusmenu.rs` turns this tree into the wire format; nothing in this
//! file knows that D-Bus exists.

use awake_core::PolicyGap;
use awake_ipc::{StatusDocument, WireEnd, WireIndicator, WireRemaining, WireSession};

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
    EndSession,
    OpenApplication,
    Quit,
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
pub fn build(
    status: &StatusDocument,
    options: QuickOptions,
    locale: Locale,
    offset: UtcOffset,
) -> Menu {
    let labels = locale.labels();
    let mut items = if status.is_active() {
        active_items(status, labels, offset)
    } else {
        inactive_items(status, options, labels)
    };

    items.push(MenuItem::separator());
    items.push(automatic_rules(labels));
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
    items.push(MenuItem::standard(
        labels.end_session,
        MenuAction::EndSession,
    ));
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

/// Automatic rules exist from ticket 26 onward. Until then the row states that
/// plainly instead of showing a switch that does nothing.
fn automatic_rules(labels: &Labels) -> MenuItem {
    MenuItem::submenu(
        format!("{}: {}", labels.automatic_rules, labels.not_available_yet),
        vec![MenuItem::info(labels.pause_automatic_rules)],
    )
    .disabled()
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
    use awake_core::{BackendCapabilities, SessionOrigin, SessionPolicy};
    use awake_ipc::{WireBackend, WireInterrupted, WireReason};

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
            now_unix_seconds: NOW,
        }
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
        build(status, QuickOptions::default(), locale, UtcOffset::UTC)
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
                "Automatic rules: Not available yet",
                "Pause automatic rules",
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
        let menu = menu(&inactive(), Locale::ZhTw);
        let labels = menu.labels();
        for expected in [
            "保持清醒",
            "目前未保持清醒",
            "開始一段工作階段",
            "持續保持清醒",
            "直到指定時間",
            "允許螢幕關閉",
            "低於 20% 電量時停止",
            "暫停自動規則",
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
}
