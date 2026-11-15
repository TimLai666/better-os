//! The Keep awake page's own decisions (spec section 6 and the Keep awake
//! rows of sections 2, 3, and 12), without GPUI: which state the page is in,
//! what its top card says and offers, what each reason's menu holds, when an
//! "Until a time" session ends, the lengths a person can add, and what Home
//! is told. `model.rs` holds what moved from the old window.

use awake_core::{SessionOrigin, SessionPolicy};
use awake_ipc::{
    QuickLength, QuickSessions, QuickSessionsDocument, StatusDocument, WireEnd, WireIndicator,
    WireRemaining, WireSuppression,
};
use better_ui::fluent::time_picker::Time;

use super::copy::{Copy, PageCopy, fill};
use super::localtime::{UtcOffset, clock_time};
use super::model::{ReasonRow, StatusView, remaining_label};

/// The newest protocol the page needs: the quick-session offer is version 4.
pub(crate) const REQUIRED_PROTOCOL: u32 = awake_ipc::PROTOCOL_VERSION;

/// The four sub-pages, in the selector's order. `Ctrl+1` … `Ctrl+4` reach
/// them (audit A8).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub(crate) enum SubPage {
    #[default]
    Status,
    Rules,
    History,
    Diagnostics,
}

impl SubPage {
    pub(crate) const ALL: [SubPage; 4] = [
        SubPage::Status,
        SubPage::Rules,
        SubPage::History,
        SubPage::Diagnostics,
    ];

    pub(crate) fn index(self) -> usize {
        SubPage::ALL
            .iter()
            .position(|page| *page == self)
            .unwrap_or(0)
    }

    pub(crate) fn title(self, w: &'static PageCopy) -> &'static str {
        match self {
            SubPage::Status => w.selector_status,
            SubPage::Rules => w.selector_rules,
            SubPage::History => w.selector_history,
            SubPage::Diagnostics => w.selector_diagnostics,
        }
    }

    /// The shortcut, written the way the audit asks: `Ctrl+1`.
    pub(crate) fn shortcut(self) -> String {
        format!("Ctrl+{}", self.index() + 1)
    }

    /// The sub-page a `Ctrl+digit` reaches.
    pub(crate) fn for_key(key: &str, control: bool, alt: bool, shift: bool) -> Option<SubPage> {
        if !control || alt || shift {
            return None;
        }
        match key {
            "1" => Some(SubPage::Status),
            "2" => Some(SubPage::Rules),
            "3" => Some(SubPage::History),
            "4" => Some(SubPage::Diagnostics),
            _ => None,
        }
    }
}

/// Why reading the service failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ReadError {
    /// Nobody owns the name and the bus could not start it.
    NotOnBus(String),
    /// It answered, but not with something this page can read.
    Failed(String),
}

impl ReadError {
    pub(crate) fn details(&self) -> &str {
        match self {
            ReadError::NotOnBus(details) | ReadError::Failed(details) => details,
        }
    }
}

/// Whether Better Manager's record has Better Awake, and on.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Presence {
    pub(crate) installed: bool,
    pub(crate) enabled: bool,
    pub(crate) version: Option<String>,
}

/// The state the whole page is in (spec section 2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Service {
    /// The first read has not come back.
    Reading,
    Ready,
    /// The service answers an older protocol than the page needs.
    TooOld {
        installed: String,
    },
    /// Installed, but nothing answers.
    NotRunning {
        offers_start: bool,
        details: String,
    },
    NotInstalled,
}

impl Service {
    /// What the page decides from one read and Better Manager's record.
    ///
    /// The bus is asked first: a service that answers is there however it
    /// was installed. A service that answers an older protocol is too old
    /// even when the record says otherwise.
    pub(crate) fn decide(
        read: Option<(&Result<StatusDocument, ReadError>, Option<u32>)>,
        presence: &Presence,
    ) -> Service {
        let Some((status, version)) = read else {
            return Service::Reading;
        };
        match (status, version) {
            (Ok(_), Some(version)) if version < REQUIRED_PROTOCOL => Service::TooOld {
                installed: presence
                    .version
                    .clone()
                    .unwrap_or_else(|| format!("protocol {version}")),
            },
            (Ok(_), _) => Service::Ready,
            (Err(error), _) if presence.installed => Service::NotRunning {
                offers_start: crate::pages::states::offers_start_service(
                    presence.installed,
                    presence.enabled,
                ),
                details: error.details().to_string(),
            },
            (Err(error), _) => match error {
                // Something answered but could not be read: it is there,
                // whatever the record says.
                ReadError::Failed(details) => Service::NotRunning {
                    offers_start: false,
                    details: details.clone(),
                },
                ReadError::NotOnBus(_) => Service::NotInstalled,
            },
        }
    }

    pub(crate) fn is_ready(&self) -> bool {
        matches!(self, Service::Ready)
    }
}

/// What the top card of Status says, and which control it carries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Headline {
    /// Nothing holds the PC: `SplitButton` Keep awake with the lengths and
    /// Until a time.
    Nothing,
    /// A session the person started: Extend ▾ and End.
    Manual {
        session_id: u64,
        remaining: WireRemaining,
        until: Option<u64>,
    },
    /// Only rules hold it: Pause rules ▾.
    RulesOnly { rule: String },
    /// Rules are paused and nothing holds it: Resume rules.
    Paused { until: Option<u64> },
    /// Every rule is overridden and nothing holds it: Resume rules.
    Overridden,
    /// A session needs attention: See diagnostics.
    Attention { reason: String },
}

impl Headline {
    pub(crate) fn of(status: &StatusDocument, view: &StatusView) -> Headline {
        if status.indicator == WireIndicator::AttentionRequired {
            return Headline::Attention {
                reason: status.attention.clone().unwrap_or_default(),
            };
        }
        if let Some(manual) = status.manual_session() {
            return Headline::Manual {
                session_id: manual.session_id,
                remaining: manual.remaining,
                until: match manual.end {
                    WireEnd::UntilUnixSeconds { unix_seconds } => Some(unix_seconds),
                    _ => None,
                },
            };
        }
        if let Some(reason) = view.reasons.first() {
            return Headline::RulesOnly {
                rule: reason.display_name().to_string(),
            };
        }
        match status.rules_suppression {
            Some(WireSuppression::PausedUntil { unix_seconds }) => Headline::Paused {
                until: Some(unix_seconds),
            },
            Some(WireSuppression::PausedUntilResumed) => Headline::Paused { until: None },
            Some(WireSuppression::Overridden) => Headline::Overridden,
            None => Headline::Nothing,
        }
    }

    /// The header and the description the card shows.
    pub(crate) fn words(
        &self,
        w: &'static PageCopy,
        c: &'static Copy,
        offset: UtcOffset,
    ) -> (String, String) {
        match self {
            Headline::Nothing => (
                w.nothing_holds_header.to_string(),
                w.nothing_holds_description.to_string(),
            ),
            Headline::Manual {
                remaining, until, ..
            } => (
                w.kept_awake_header.to_string(),
                match (remaining, until) {
                    (WireRemaining::UntilEnded, _) => w.started_by_you_until_end.to_string(),
                    (_, Some(until)) => fill(
                        w.started_by_you_until_time,
                        "time",
                        &clock_time(*until, offset),
                    ),
                    (remaining, None) => fill(
                        w.started_by_you_left,
                        "time",
                        &remaining_label(*remaining, c),
                    ),
                },
            ),
            Headline::RulesOnly { rule } => (
                w.kept_awake_header.to_string(),
                fill(w.started_by_rule, "rule", rule),
            ),
            Headline::Paused { until: Some(until) } => (
                fill(w.rules_paused_header, "time", &clock_time(*until, offset)),
                String::new(),
            ),
            Headline::Paused { until: None } => (
                w.rules_paused_until_resume_header.to_string(),
                w.rules_paused_description.to_string(),
            ),
            Headline::Overridden => (w.rules_overridden_header.to_string(), String::new()),
            Headline::Attention { reason } => (w.attention_header.to_string(), reason.clone()),
        }
    }
}

/// One entry of a reason's `…` menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReasonAction {
    Extend(u64),
    Modify,
    End,
}

/// The menu a reason's card carries (audit A4: one menu, not five buttons).
/// Extending means something only for a timed session of the person's own;
/// a rule's session ends when its condition does, so only End is offered.
pub(crate) fn reason_actions(reason: &ReasonRow) -> Vec<ReasonAction> {
    let manual = reason.origin == SessionOrigin::Manual;
    let timed = matches!(reason.remaining, Some(WireRemaining::Seconds { .. }));
    let mut actions = Vec::new();
    if manual && timed {
        actions.extend([15, 30, 60].map(ReasonAction::Extend));
    }
    if manual {
        actions.push(ReasonAction::Modify);
    }
    actions.push(ReasonAction::End);
    actions
}

pub(crate) fn reason_action_label(action: ReasonAction, w: &'static PageCopy) -> &'static str {
    match action {
        ReasonAction::Extend(15) => w.extend_15,
        ReasonAction::Extend(30) => w.extend_30,
        ReasonAction::Extend(_) => w.extend_60,
        ReasonAction::Modify => w.modify,
        ReasonAction::End => w.end_this_reason,
    }
}

/// When a session started "until" a time of day ends: that time today, or
/// tomorrow when it has already passed (or is now).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Until {
    pub(crate) unix_seconds: u64,
    pub(crate) tomorrow: bool,
}

pub(crate) fn until(now_unix_seconds: u64, offset: UtcOffset, time: Time) -> Until {
    let now = i64::try_from(now_unix_seconds).unwrap_or(i64::MAX / 2);
    let local = now + offset.0;
    let midnight = local - local.rem_euclid(86_400);
    let mut at = midnight + i64::from(time.minutes()) * 60 - offset.0;
    let tomorrow = at <= now;
    if tomorrow {
        at += 86_400;
    }
    Until {
        unix_seconds: u64::try_from(at).unwrap_or(0),
        tomorrow,
    }
}

/// The Caption under the time picker: `Tomorrow at 07:30`.
pub(crate) fn until_caption(until: Until, time: Time, w: &'static PageCopy) -> String {
    fill(
        if until.tomorrow {
            w.tomorrow_at
        } else {
            w.today_at
        },
        "time",
        &time.format(),
    )
}

/// The time the dialog opens on: the next whole hour after an hour from now.
pub(crate) fn suggested_until(now_unix_seconds: u64, offset: UtcOffset) -> Time {
    let local = i64::try_from(now_unix_seconds).unwrap_or(0) + offset.0 + 3_600;
    let minutes = (local.rem_euclid(86_400) / 60) as u32;
    Time::from_minutes(minutes.div_ceil(60) * 60)
}

/// A length in the page's words, the same words the tray and the
/// notifications use for a length ("1 hour", "90 minutes").
pub(crate) fn length_label(length: QuickLength, locale: crate::i18n::Locale) -> String {
    match length {
        QuickLength::Indefinite => super::copy::copy(locale).preset_indefinite.to_string(),
        QuickLength::Minutes { minutes } => {
            super::copy::limit_words(locale, u32::try_from(minutes).unwrap_or(u32::MAX))
        }
    }
}

/// The lengths "Add a length" offers: common ones the menu does not have,
/// none once it holds eight.
pub(crate) fn addable_lengths(offer: &QuickSessions) -> Vec<QuickLength> {
    if offer.lengths.len() >= awake_ipc::MAX_QUICK_LENGTHS {
        return Vec::new();
    }
    [5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 360, 480, 720]
        .into_iter()
        .map(|minutes| QuickLength::Minutes { minutes })
        .chain(std::iter::once(QuickLength::Indefinite))
        .filter(|length| !offer.lengths.contains(length))
        .collect()
}

/// What a quick-session row offers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LengthRow {
    pub(crate) length: QuickLength,
    pub(crate) is_default: bool,
    pub(crate) can_move_up: bool,
    pub(crate) can_move_down: bool,
    pub(crate) can_remove: bool,
}

pub(crate) fn length_rows(offer: &QuickSessions) -> Vec<LengthRow> {
    let count = offer.lengths.len();
    offer
        .lengths
        .iter()
        .enumerate()
        .map(|(index, length)| LengthRow {
            length: *length,
            is_default: index == offer.default_index as usize,
            can_move_up: index > 0,
            can_move_down: index + 1 < count,
            can_remove: count > 1,
        })
        .collect()
}

/// The notice the offer carries about the one-time import, in words.
pub(crate) fn import_notice(
    document: &QuickSessionsDocument,
    w: &'static PageCopy,
) -> Option<String> {
    let notice = document.import_notice.as_deref()?;
    if let Some(schema) = notice.strip_prefix("awake.quick_sessions.import_newer_schema:") {
        return Some(fill(w.import_newer_schema, "n", schema));
    }
    Some(w.import_unreadable.to_string())
}

/// One of the four "Defaults for new sessions" switches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefaultField {
    SystemSleep,
    Idle,
    DisplaySleep,
    AutomaticLock,
    Lid,
}

impl DefaultField {
    pub(crate) const SWITCHES: [DefaultField; 4] = [
        DefaultField::SystemSleep,
        DefaultField::Idle,
        DefaultField::DisplaySleep,
        DefaultField::AutomaticLock,
    ];

    pub(crate) fn label(self, w: &'static PageCopy, c: &'static Copy) -> &'static str {
        match self {
            DefaultField::SystemSleep => w.prevent_system_sleep,
            DefaultField::Idle => w.prevent_idle,
            DefaultField::DisplaySleep => w.prevent_display_sleep,
            DefaultField::AutomaticLock => w.prevent_automatic_lock,
            DefaultField::Lid => c.keep_awake_lid_closed,
        }
    }

    pub(crate) fn get(self, policy: &SessionPolicy) -> bool {
        match self {
            DefaultField::SystemSleep => policy.prevent_system_suspend,
            DefaultField::Idle => policy.prevent_idle,
            DefaultField::DisplaySleep => policy.prevent_display_sleep,
            DefaultField::AutomaticLock => policy.prevent_automatic_lock,
            DefaultField::Lid => policy.prevent_lid_suspend,
        }
    }

    pub(crate) fn set(self, policy: &mut SessionPolicy, on: bool) {
        match self {
            DefaultField::SystemSleep => policy.prevent_system_suspend = on,
            DefaultField::Idle => policy.prevent_idle = on,
            DefaultField::DisplaySleep => policy.prevent_display_sleep = on,
            DefaultField::AutomaticLock => policy.prevent_automatic_lock = on,
            DefaultField::Lid => policy.prevent_lid_suspend = on,
        }
    }

    /// The display and the lock show the reduced-security caution when on.
    pub(crate) fn warns(self, on: bool) -> bool {
        on && matches!(
            self,
            DefaultField::DisplaySleep | DefaultField::AutomaticLock
        )
    }
}

/// The closed-lid limits Battery and safety offers (ticket 109), in minutes;
/// `None` is never.
pub(crate) const LID_LIMITS: [Option<u32>; 6] =
    [Some(30), Some(60), Some(120), Some(240), Some(480), None];

pub(crate) fn lid_limit(minutes: Option<u32>) -> awake_core::LidBatteryLimit {
    match minutes {
        Some(minutes) => awake_core::LidBatteryLimit::Minutes { minutes },
        None => awake_core::LidBatteryLimit::Never,
    }
}

pub(crate) fn lid_limit_index(limit: awake_core::LidBatteryLimit) -> Option<usize> {
    LID_LIMITS
        .iter()
        .position(|minutes| *minutes == limit.minutes())
}

/// What Home and the pane know about Keep awake (spec section 3).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum Facts {
    /// Nothing read yet, or the component is not there: Home says nothing.
    #[default]
    Quiet,
    /// Installed and not answering.
    Stopped { offers_start: bool },
    Running {
        /// `{first reason}, {time} left`, when a session holds the PC.
        active: Option<String>,
        /// Whether a session of the person's own runs, which the Home toggle
        /// shows as on.
        manual: bool,
        /// The service's reason, when the backend cannot stop sleep.
        cannot_stop_sleep: Option<String>,
    },
}

pub(crate) fn facts(
    service: &Service,
    status: Option<&StatusDocument>,
    w: &'static PageCopy,
    c: &'static Copy,
) -> Facts {
    match (service, status) {
        (Service::NotRunning { offers_start, .. }, _) => Facts::Stopped {
            offers_start: *offers_start,
        },
        (Service::Ready, Some(status)) => {
            let view = StatusView::from_status(status);
            let active = view.reasons.first().map(|reason| {
                let name = reason.display_name();
                match reason.remaining {
                    Some(WireRemaining::Seconds { seconds }) => fill(
                        &fill(w.home_active_description, "reason", name),
                        "time",
                        &super::model::duration_label(seconds, c),
                    ),
                    _ => fill(w.home_active_until_end, "reason", name),
                }
            });
            let cannot_stop_sleep = (!status.backend.available
                || status.indicator == WireIndicator::AttentionRequired)
                // The service's reasons are keys; Home says them in words.
                .then(|| w.home_cannot_stop_description.to_string());
            Facts::Running {
                active,
                manual: status.manual_session().is_some(),
                cannot_stop_sleep,
            }
        }
        _ => Facts::Quiet,
    }
}

/// The request the Home toggle and the split button's main part send: the
/// default length with the defaults for new sessions.
pub(crate) fn start_request(
    offer: &QuickSessions,
    length: QuickLength,
    reason: &str,
) -> awake_ipc::RequestBody {
    start_with_end(offer, length.end(), reason)
}

pub(crate) fn start_with_end(
    offer: &QuickSessions,
    end: WireEnd,
    reason: &str,
) -> awake_ipc::RequestBody {
    let defaults = offer.defaults;
    awake_ipc::RequestBody::StartSession {
        reason: reason.to_string(),
        policy: defaults.policy,
        battery_stop_percent: defaults.battery_stop_percent,
        end,
        // The consequence is stated beside the two switches that ask for it,
        // in Defaults for new sessions, which is where it was chosen.
        security_confirmed: defaults.policy.needs_security_confirmation(),
    }
}

/// What a person changes in the Modify dialog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ModifyDraft {
    pub(crate) session_id: u64,
    /// An index into the offer's lengths, or `None` for "Until a time".
    pub(crate) length: Option<usize>,
    pub(crate) until: Time,
    pub(crate) policy: SessionPolicy,
    pub(crate) battery_stop_percent: Option<u8>,
}

/// The change a Modify dialog asks for, measured from now.
pub(crate) fn modify_request(
    draft: &ModifyDraft,
    offer: &QuickSessions,
    reason: &str,
    now_unix_seconds: u64,
    offset: UtcOffset,
) -> awake_ipc::RequestBody {
    let end = match draft.length.and_then(|index| offer.lengths.get(index)) {
        Some(length) => length.end(),
        None => WireEnd::UntilUnixSeconds {
            unix_seconds: until(now_unix_seconds, offset, draft.until).unix_seconds,
        },
    };
    awake_ipc::RequestBody::ChangeSession {
        session_id: draft.session_id,
        reason: reason.to_string(),
        policy: draft.policy,
        battery_stop_percent: draft.battery_stop_percent,
        end,
        security_confirmed: draft.policy.needs_security_confirmation(),
    }
}

/// A rule's one-line description: its conditions, then whether it matches.
pub(crate) fn rule_description(
    rule: &super::model::RuleView,
    c: &'static Copy,
    w: &'static PageCopy,
) -> String {
    let groups: Vec<String> = rule
        .groups
        .iter()
        .map(|group| {
            let joiner = match group.combine {
                awake_core::Combine::All => " + ",
                awake_core::Combine::Any => " / ",
            };
            group
                .conditions
                .iter()
                .map(|condition| condition.summary.clone())
                .collect::<Vec<_>>()
                .join(joiner)
        })
        .collect();
    let joiner = match rule.combine {
        awake_core::Combine::All => " + ",
        awake_core::Combine::Any => " / ",
    };
    let _ = c;
    format!(
        "{} · {}",
        groups.join(joiner),
        if rule.matching_now {
            w.matching_now
        } else {
            w.not_matching_now
        }
    )
}

/// The History sub-page's rows as the table's text, newest first, and the
/// columns a person can sort by (start and end).
pub(crate) const HISTORY_SORTABLE: [usize; 2] = [0, 1];

pub(crate) fn history_cells(
    rows: &[super::model::HistoryRow],
    c: &'static Copy,
    offset: UtcOffset,
) -> Vec<[String; 5]> {
    rows.iter()
        .map(|row| {
            [
                row.started_label(offset),
                row.ended_label(offset, c),
                row.origin_label(c).to_string(),
                row.reasons.join("、"),
                if row.ended_at_unix_seconds.is_some() {
                    row.cause_label(c).to_string()
                } else {
                    String::new()
                },
            ]
        })
        .collect()
}
