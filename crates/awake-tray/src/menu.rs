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

use crate::labels::{Labels, Locale, LocaleLabels};
use crate::localtime::{UtcOffset, clock_time};

/// The lengths offered by Extend session.
pub const EXTEND_MINUTES: [u64; 3] = [15, 30, 60];

/// The battery threshold the `低於 20% 電量時停止` wording promises.
pub const QUICK_BATTERY_PERCENT: u8 = 20;

/// The toggles in Quick options. They shape the next session the tray
/// starts; they never change anything on their own, which is why toggling one
/// is not a request to the service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuickOptions {
    /// Checked means the display may still turn off, which is the default.
    pub allow_display_off: bool,
    pub stop_below_battery: bool,
    /// "Also when the lid is closed". On by default, because a session started
    /// by hand is one the person means to survive closing the lid (ticket 109).
    pub keep_awake_lid_closed: bool,
}

impl Default for QuickOptions {
    fn default() -> Self {
        Self {
            allow_display_off: true,
            stop_below_battery: true,
            keep_awake_lid_closed: awake_core::SessionPolicy::manual_default().prevent_lid_suspend,
        }
    }
}

impl QuickOptions {
    pub fn policy(&self) -> awake_core::SessionPolicy {
        awake_core::SessionPolicy {
            prevent_display_sleep: !self.allow_display_off,
            prevent_lid_suspend: self.keep_awake_lid_closed,
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
    ToggleKeepAwakeLidClosed,
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

    /// How many ids the menu uses.
    pub fn len(&self) -> usize {
        let mut count = 0;
        for item in &self.items {
            item.walk(&mut |_| count += 1);
        }
        count
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The same menu with its ids numbered depth-first from `first`.
    ///
    /// A panel caches every item by id and, when the layout changes, re-reads
    /// only `type` and `children-display` for an id it already knows (the
    /// AppIndicator extension's `_doLayoutUpdate`). So an id must never be
    /// reused for a different item: the controller hands every changed layout
    /// a fresh range.
    pub fn renumbered(mut self, first: i32) -> Menu {
        let mut next = first;
        for item in &mut self.items {
            number(item, &mut next);
        }
        self
    }

    /// This menu's items in this menu's places, showing what can be carried
    /// over from `newer`, a menu of another shape: what a host with a
    /// submenu open is shown while `newer`'s layout waits.
    ///
    /// Awake's items have no identity across layouts, so a row takes
    /// `newer`'s wording only where the two menus line up one to one: at the
    /// same level, from the first item on, for as long as each pair is the
    /// same kind of item with the same action and, for a row with no action,
    /// the same field name before its colon ("Remaining: 3 min"). Past the
    /// first pair that does not line up, nothing on that level is carried
    /// over. Any row whose action `newer` no longer offers anywhere is
    /// greyed out, so it cannot be chosen; a submenu's own row keeps its
    /// state, because greying the row of an open submenu is not something to
    /// do under the pointer.
    pub fn showing(&self, newer: &Menu) -> Menu {
        let mut shown = self.clone();
        line_up(&mut shown.items, &newer.items);
        let mut offered = Vec::new();
        for item in &newer.items {
            item.walk(&mut |item| {
                if item.enabled
                    && let Some(action) = item.action
                {
                    offered.push(action);
                }
            });
        }
        for item in &mut shown.items {
            grey_out(item, &offered);
        }
        shown
    }

    /// The structure without the wording: what kind each item is, whether it
    /// is drawn, and what hangs under it. Two menus with the same shape map
    /// id for id, so a change between them is a change of properties only.
    pub fn shape(&self) -> Vec<Shape> {
        self.items.iter().map(MenuItem::shape).collect()
    }
}

/// One item's place in a [`Menu::shape`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shape {
    kind: std::mem::Discriminant<ItemKind>,
    enabled_submenu: bool,
    children: Vec<Shape>,
}

impl MenuItem {
    fn shape(&self) -> Shape {
        Shape {
            kind: std::mem::discriminant(&self.kind),
            // A disabled submenu is not drawn at all, so enabling one changes
            // the layout and not only a property.
            enabled_submenu: self.kind == ItemKind::Submenu && self.enabled,
            children: self.children.iter().map(MenuItem::shape).collect(),
        }
    }
}

/// The part of a label before its colon, which names the fact the row
/// states; the whole label when it has none.
fn field_name(label: &str) -> &str {
    label
        .split_once([':', '：'])
        .map_or(label, |(field, _)| field)
}

fn corresponds(shown: &MenuItem, newer: &MenuItem) -> bool {
    std::mem::discriminant(&shown.kind) == std::mem::discriminant(&newer.kind)
        && shown.action == newer.action
        && (shown.action.is_some() || field_name(&shown.label) == field_name(&newer.label))
}

/// See [`Menu::showing`].
fn line_up(shown: &mut [MenuItem], newer: &[MenuItem]) {
    for (shown, newer) in shown.iter_mut().zip(newer) {
        if !corresponds(shown, newer) {
            return;
        }
        shown.label.clone_from(&newer.label);
        shown.kind = newer.kind.clone();
        if shown.kind == ItemKind::Submenu {
            line_up(&mut shown.children, &newer.children);
        } else {
            shown.enabled = newer.enabled;
        }
    }
}

fn grey_out(item: &mut MenuItem, offered: &[MenuAction]) {
    if let Some(action) = item.action
        && !offered.contains(&action)
    {
        item.enabled = false;
    }
    for child in &mut item.children {
        grey_out(child, offered);
    }
}

fn number(item: &mut MenuItem, next: &mut i32) {
    item.id = *next;
    *next += 1;
    for child in &mut item.children {
        number(child, next);
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
        active_items(status, labels, locale, offset)
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
            lid_option(status, options, labels),
        ],
    ));
    items
}

/// The lid quick option. A service from before protocol version 3, which an
/// update has not restarted yet, has no lid lock to give, so the option is
/// shown as unavailable rather than ticked and quietly ignored.
fn lid_option(status: &StatusDocument, options: QuickOptions, labels: &Labels) -> MenuItem {
    if status.backend.capabilities.lid_switch {
        MenuItem::checkmark(
            labels.keep_awake_lid_closed,
            options.keep_awake_lid_closed,
            MenuAction::ToggleKeepAwakeLidClosed,
        )
    } else {
        MenuItem::info(labels.keep_awake_lid_closed_unavailable)
    }
}

fn active_items(
    status: &StatusDocument,
    labels: &Labels,
    locale: Locale,
    offset: UtcOffset,
) -> Vec<MenuItem> {
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

    // What the session holds back, in a submenu of its own: four rows of
    // facts in the top level made the awake menu tall enough that GNOME
    // scrolled every submenu inside it on an 800-pixel screen.
    items.push(MenuItem::submenu(
        labels.current_settings,
        vec![
            policy_row(
                labels.system_sleep,
                status.effective_policy.prevent_system_suspend,
                status.unmet_policy.contains(&PolicyGap::SystemSuspend),
                labels,
            ),
            policy_row(
                labels.display_sleep,
                status.effective_policy.prevent_display_sleep,
                status.unmet_policy.contains(&PolicyGap::DisplaySleep),
                labels,
            ),
            policy_row(
                labels.automatic_lock,
                status.effective_policy.prevent_automatic_lock,
                status.unmet_policy.contains(&PolicyGap::AutomaticLock),
                labels,
            ),
            if status.backend.capabilities.lid_switch {
                policy_row(
                    labels.lid_sleep,
                    status.effective_policy.prevent_lid_suspend,
                    status.unmet_policy.contains(&PolicyGap::LidSwitch),
                    labels,
                )
            } else {
                MenuItem::info(format!("{}: {}", labels.lid_sleep, labels.lid_unavailable))
            },
            // As the window's Status page reads it: from the battery
            // protection the service reports, and "not applicable" on a
            // machine with no battery rather than a threshold that never fires.
            MenuItem::info(format!(
                "{}: {}",
                labels.battery_protection,
                match (
                    status.battery_protection.has_battery,
                    status.battery_protection.stop_below_percent,
                ) {
                    (false, _) => labels.battery_not_applicable.to_string(),
                    (true, Some(percent)) => labels
                        .battery_stops_at
                        .replace("{percent}", &percent.to_string()),
                    (true, None) => labels.battery_off.to_string(),
                }
            )),
        ]
        .into_iter()
        .chain(lid_limit_row(status, labels, locale, offset))
        .collect(),
    ));

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
    if status.manual_session().is_some() && status.backend.available {
        // Choosing a length while a session runs replaces the running one's
        // end, measured from now. The controller turns these into a
        // ChangeSession, so a second "start" is never refused as a duplicate.
        // The length the session already has is not offered as a change.
        // A session changed or extended before carries an end time rather
        // than its length, so every length is offered for it.
        let current = session.end;
        let lengths = [
            (
                labels.indefinitely,
                MenuAction::StartIndefinite,
                WireEnd::Indefinite,
            ),
            (
                labels.minutes_15,
                MenuAction::StartMinutes(15),
                WireEnd::Duration { seconds: 15 * 60 },
            ),
            (
                labels.minutes_30,
                MenuAction::StartMinutes(30),
                WireEnd::Duration { seconds: 30 * 60 },
            ),
            (
                labels.hour_1,
                MenuAction::StartMinutes(60),
                WireEnd::Duration { seconds: 60 * 60 },
            ),
            (
                labels.hours_2,
                MenuAction::StartMinutes(120),
                WireEnd::Duration { seconds: 120 * 60 },
            ),
        ];
        items.push(MenuItem::submenu(
            labels.change_length,
            lengths
                .into_iter()
                .filter(|(_, _, end)| *end != current)
                .map(|(label, action, _)| MenuItem::standard(label, action))
                .collect(),
        ));
    }
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

/// The safety that goes with the lid choice, stated beside it: how long the lid
/// may stay closed on battery. Only while the lid is held, because otherwise
/// closing the lid suspends the machine as it always did.
fn lid_limit_row(
    status: &StatusDocument,
    labels: &Labels,
    locale: Locale,
    offset: UtcOffset,
) -> Option<MenuItem> {
    if !status.effective_policy.prevent_lid_suspend
        || status.unmet_policy.contains(&PolicyGap::LidSwitch)
    {
        return None;
    }
    // The clock is running (lid closed, on battery): when it stops, as a
    // countdown. Otherwise the limit as a rule.
    if let Some(stops_at) = status.lid.stops_at_unix_seconds {
        let minutes = stops_at
            .saturating_sub(status.now_unix_seconds)
            .div_ceil(60)
            .max(1);
        let value = labels
            .lid_stops_at
            .replace("{time}", &clock_time(stops_at, offset))
            .replace(
                "{remaining}",
                &awake_ipc::notification::limit_words(locale, minutes as u32),
            );
        return Some(MenuItem::info(format!(
            "{}: {value}",
            labels.lid_battery_limit
        )));
    }
    let value = match status.lid.battery_limit.minutes() {
        Some(minutes) => labels.lid_stops_after.replace(
            "{limit}",
            &awake_ipc::notification::limit_words(locale, minutes),
        ),
        None => labels.lid_never_stops.to_string(),
    };
    Some(MenuItem::info(format!(
        "{}: {value}",
        labels.lid_battery_limit
    )))
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
                lid_switch: true,
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
            lid: awake_ipc::WireLidProtection::default(),
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
            battery_protection: WireBatteryProtection {
                has_battery: true,
                percent: Some(80),
                on_ac_power: Some(false),
                stop_below_percent: Some(20),
            },
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
                "Also when the lid is closed",
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

        assert!(labels.contains(&"System sleep: Held off"));
        assert!(labels.contains(&"Display sleep: Allowed"));
        assert!(labels.contains(&"Automatic lock: Allowed"));
        assert!(labels.contains(&"Battery stop threshold: Stops below 20%"));
    }

    #[test]
    fn a_policy_the_backend_cannot_hold_is_shown_as_unsupported_not_as_in_force() {
        let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        status.effective_policy.prevent_display_sleep = true;
        status.unmet_policy = vec![PolicyGap::DisplaySleep];

        let menu = menu(&status, Locale::EnUs);
        let labels = menu.labels();
        assert!(labels.contains(&"Display sleep: Asked for, not delivered by the backend"));
        assert!(!labels.contains(&"Display sleep: Held off"));
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

        assert!(
            allowed.policy().prevent_lid_suspend,
            "a session started by hand keeps awake with the lid closed by default"
        );

        let held_on = QuickOptions {
            allow_display_off: false,
            stop_below_battery: false,
            keep_awake_lid_closed: false,
        };
        assert!(held_on.policy().prevent_display_sleep);
        assert!(!held_on.policy().prevent_lid_suspend);
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

    // ---- Ticket 102 ---------------------------------------------------------

    #[test]
    fn the_awake_menu_offers_the_lengths_to_change_to_and_end() {
        let menu = menu(
            &active(WireEnd::Indefinite, WireRemaining::UntilEnded),
            Locale::EnUs,
        );
        let change = menu.find_by_label("Change length").unwrap();
        assert_eq!(change.kind, ItemKind::Submenu);
        assert!(change.enabled);
        // Indefinite is what it already is, so it is not offered.
        assert_eq!(
            change
                .children
                .iter()
                .map(|child| child.action.unwrap())
                .collect::<Vec<_>>(),
            vec![
                MenuAction::StartMinutes(15),
                MenuAction::StartMinutes(30),
                MenuAction::StartMinutes(60),
                MenuAction::StartMinutes(120),
            ]
        );
        assert!(actions(&menu).contains(&MenuAction::EndSession));
        assert!(menu.find_by_label("Start a session").is_none());
    }

    #[test]
    fn the_not_awake_menu_offers_each_start_length_once() {
        let menu = menu(&inactive(), Locale::ZhTw);
        let starts: Vec<MenuAction> = actions(&menu)
            .into_iter()
            .filter(|action| {
                matches!(
                    action,
                    MenuAction::StartIndefinite | MenuAction::StartMinutes(_)
                )
            })
            .collect();
        let mut unique = starts.clone();
        unique.dedup();
        assert_eq!(starts, unique);
        assert_eq!(starts.len(), 5);
        assert!(menu.find_by_label("改成其他長度").is_none());
    }

    #[test]
    fn a_session_held_only_by_a_rule_offers_no_length_to_change_to() {
        let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        status.sessions[0].origin = SessionOrigin::Trigger;
        status.indicator = WireIndicator::ActiveTrigger;
        assert!(
            menu(&status, Locale::EnUs)
                .find_by_label("Change length")
                .is_none(),
            "a rule's session is not the person's to change from the tray"
        );
    }

    #[test]
    fn renumbering_keeps_the_items_and_moves_every_id() {
        let menu = menu(&inactive(), Locale::EnUs);
        let moved = menu.clone().renumbered(500);
        assert_eq!(menu.labels(), moved.labels());
        assert_eq!(menu.shape(), moved.shape());
        let mut ids = Vec::new();
        for item in &moved.items {
            item.walk(&mut |candidate| ids.push(candidate.id));
        }
        assert_eq!(ids, (500..500 + menu.len() as i32).collect::<Vec<_>>());
    }

    #[test]
    fn a_new_countdown_is_the_same_shape_and_a_new_state_is_not() {
        let timed = |seconds| {
            menu(
                &active(
                    WireEnd::Duration { seconds: 7_200 },
                    WireRemaining::Seconds { seconds },
                ),
                Locale::EnUs,
            )
        };
        assert_ne!(timed(5_400), timed(5_340));
        assert_eq!(timed(5_400).shape(), timed(5_340).shape());
        assert_ne!(
            timed(5_400).shape(),
            menu(&inactive(), Locale::EnUs).shape()
        );
        // Ticking an option is only a property change.
        let toggled = build(
            &inactive(),
            QuickOptions {
                allow_display_off: false,
                ..QuickOptions::default()
            },
            Locale::EnUs,
            UtcOffset::UTC,
            false,
        );
        assert_eq!(toggled.shape(), menu(&inactive(), Locale::EnUs).shape());
    }

    #[test]
    fn the_awake_menu_keeps_the_policy_facts_one_level_down() {
        let menu = menu(
            &active(WireEnd::Indefinite, WireRemaining::UntilEnded),
            Locale::EnUs,
        );
        let settings = menu.find_by_label("Current settings").unwrap();
        assert_eq!(settings.kind, ItemKind::Submenu);
        assert_eq!(settings.children.len(), 5);
        assert!(
            menu.items
                .iter()
                .all(|item| !item.label.starts_with("System sleep")),
            "the facts are not in the top level"
        );
        // Top level: four facts, settings, a separator, extend (not drawn for
        // an indefinite session), change length, change session, end, a
        // separator, rules, open, quit.
        assert_eq!(menu.items.len(), 14);
    }

    #[test]
    fn the_current_settings_say_whether_the_lid_is_held_and_the_limit_that_goes_with_it() {
        let held = |locale, limit| {
            let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
            status.effective_policy = SessionPolicy::manual_default();
            status.lid.battery_limit = limit;
            menu(&status, locale)
                .find_by_label(locale.labels().current_settings)
                .unwrap()
                .children
                .iter()
                .map(|item| item.label.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            held(Locale::EnUs, awake_core::LidBatteryLimit::default()),
            vec![
                "System sleep: Held off",
                "Display sleep: Allowed",
                "Automatic lock: Allowed",
                "Sleep when the lid is closed: Held off",
                "Battery stop threshold: Stops below 20%",
                "Lid closed on battery: Stops after 2 hours",
            ]
        );
        assert_eq!(
            held(Locale::ZhTw, awake_core::LidBatteryLimit::Never)[3..],
            [
                "闔上螢幕時睡眠: 已擋下",
                "電量停止門檻: 低於 20% 時停止",
                "闔上螢幕並使用電池時: 不會因此停止",
            ]
        );

        // Not held: closing the lid suspends as it always did, and no limit is
        // stated for a choice that is off.
        let off = menu(
            &active(WireEnd::Indefinite, WireRemaining::UntilEnded),
            Locale::EnUs,
        );
        let rows: Vec<String> = off
            .find_by_label("Current settings")
            .unwrap()
            .children
            .iter()
            .map(|item| item.label.clone())
            .collect();
        assert!(rows.contains(&"Sleep when the lid is closed: Allowed".to_string()));
        assert!(
            !rows
                .iter()
                .any(|row| row.starts_with("Lid closed on battery"))
        );
    }

    #[test]
    fn a_running_closed_lid_clock_is_shown_as_a_countdown() {
        let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        status.effective_policy = SessionPolicy::manual_default();
        status.lid.stops_at_unix_seconds = Some(status.now_unix_seconds + 30 * 60);
        let at = clock_time(status.now_unix_seconds + 30 * 60, UtcOffset::UTC);
        let rows: Vec<String> = menu(&status, Locale::ZhTw)
            .find_by_label("目前的設定")
            .unwrap()
            .children
            .iter()
            .map(|item| item.label.clone())
            .collect();
        assert_eq!(
            rows.last().unwrap(),
            &format!("闔上螢幕並使用電池時: {at} 停止（還有 30 分鐘）")
        );
    }

    #[test]
    fn the_lid_quick_option_is_on_by_default_and_toggles_locally() {
        let menu = menu(&inactive(), Locale::ZhTw);
        let item = menu.find_by_label("闔上螢幕時也保持清醒").unwrap();
        assert_eq!(item.action, Some(MenuAction::ToggleKeepAwakeLidClosed));
        assert_eq!(item.kind, ItemKind::Checkmark { checked: true });
    }

    #[test]
    fn a_timed_session_is_not_offered_its_own_length_and_a_changed_one_is_offered_all() {
        let thirty = menu(
            &active(
                WireEnd::Duration { seconds: 1_800 },
                WireRemaining::Seconds { seconds: 1_700 },
            ),
            Locale::EnUs,
        );
        let offered: Vec<MenuAction> = thirty
            .find_by_label("Change length")
            .unwrap()
            .children
            .iter()
            .map(|child| child.action.unwrap())
            .collect();
        assert_eq!(
            offered,
            vec![
                MenuAction::StartIndefinite,
                MenuAction::StartMinutes(15),
                MenuAction::StartMinutes(60),
                MenuAction::StartMinutes(120),
            ]
        );

        let changed = menu(
            &active(
                WireEnd::UntilUnixSeconds {
                    unix_seconds: NOW + 600,
                },
                WireRemaining::Seconds { seconds: 600 },
            ),
            Locale::EnUs,
        );
        assert_eq!(
            changed
                .find_by_label("Change length")
                .unwrap()
                .children
                .len(),
            5
        );
    }

    #[test]
    fn a_machine_with_no_battery_says_the_threshold_does_not_apply() {
        let mut status = active(WireEnd::Indefinite, WireRemaining::UntilEnded);
        status.battery_protection = WireBatteryProtection::default();
        let labels = menu(&status, Locale::ZhTw);
        assert!(labels.labels().contains(&"電量停止門檻: 這台機器不適用"));

        status.battery_protection = WireBatteryProtection {
            has_battery: true,
            percent: Some(90),
            on_ac_power: Some(true),
            stop_below_percent: None,
        };
        assert!(
            menu(&status, Locale::ZhTw)
                .labels()
                .contains(&"電量停止門檻: 不因電量停止")
        );
    }
}
