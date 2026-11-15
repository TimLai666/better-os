//! Keep awake (ticket 87, spec section 6): Better Awake's settings inside the
//! control center. Status, Rules (with the rule editor as a sub-page),
//! History, and Diagnostics, reading and changing everything through Better
//! Awake's service, which owns every session, rule, and quick-session length.
//!
//! What the page decides is in `model` (moved from the old window) and
//! `decide` (the page's own), both GPUI-free; `bus` talks to the service,
//! `state` holds what the page keeps between interactions, and `render`
//! draws it with the Fluent kit.

pub(crate) mod bus;
pub(crate) mod copy;
pub(crate) mod decide;
pub(crate) mod localtime;
pub(crate) mod model;
pub(crate) mod render;
pub(crate) mod state;
#[cfg(test)]
mod tests;

use crate::attention::{AttentionAction, AttentionItem, Severity, Snapshot, row};
use crate::i18n::{Locale, shell};
use crate::model::Page;
use crate::navigation::Route;
use crate::pages::{NavBadge, PageModule, PageStatus};
use crate::search::SearchEntry;

use decide::{Facts, SubPage};

/// The ids of the page's cards, which the search scrolls to.
pub(crate) mod card {
    pub(crate) const STATE: &str = "awake-state";
    pub(crate) const QUICK: &str = "awake-quick-lengths";
    pub(crate) const DEFAULT_SLEEP: &str = "awake-default-sleep";
    pub(crate) const DEFAULT_DISPLAY: &str = "awake-default-display";
    pub(crate) const DEFAULT_LID: &str = "awake-default-lid";
    pub(crate) const BATTERY_STOP: &str = "awake-battery-stop";
    pub(crate) const LID_LIMIT: &str = "awake-lid-limit";
    pub(crate) const RULES: &str = "awake-rules";
    pub(crate) const HISTORY: &str = "awake-history";
    pub(crate) const DIAGNOSTICS: &str = "awake-diagnostics";
}

/// The sub-page a search result's card is on.
pub(crate) fn sub_page_of(card_id: &str) -> SubPage {
    match card_id {
        card::RULES => SubPage::Rules,
        card::HISTORY => SubPage::History,
        card::DIAGNOSTICS => SubPage::Diagnostics,
        _ => SubPage::Status,
    }
}

pub(crate) const PAGE: PageModule = PageModule {
    route: Route::KeepAwake,
    name: "keep-awake",
    status: PageStatus::Built,
    title: |s| s.keep_awake,
    search_entries,
    attention,
    badge,
};

fn search_entries(locale: Locale) -> Vec<SearchEntry> {
    let c = copy::copy(locale);
    let w = copy::page_copy(locale);
    let page = shell(locale).keep_awake;
    let entry = |header, description, group, card| SearchEntry {
        header,
        description,
        page,
        group,
        route: Route::KeepAwake,
        card,
    };
    vec![
        entry(w.keep_awake, w.nothing_holds_description, None, card::STATE),
        entry(w.until_a_time, w.until_title, None, card::STATE),
        entry(
            w.quick_header,
            w.quick_description,
            Some(w.quick_group),
            card::QUICK,
        ),
        entry(
            w.prevent_system_sleep,
            "",
            Some(w.defaults_group),
            card::DEFAULT_SLEEP,
        ),
        entry(
            w.prevent_display_sleep,
            c.reduced_security_warning,
            Some(w.defaults_group),
            card::DEFAULT_DISPLAY,
        ),
        entry(
            c.keep_awake_lid_closed,
            "",
            Some(w.defaults_group),
            card::DEFAULT_LID,
        ),
        entry(
            w.battery_stop_header,
            w.battery_stop_switch,
            Some(w.defaults_group),
            card::BATTERY_STOP,
        ),
        entry(
            c.lid_limit_heading,
            c.lid_limit_description,
            Some(w.battery_group),
            card::LID_LIMIT,
        ),
        entry(w.selector_rules, w.rules_empty, None, card::RULES),
        entry(w.selector_history, "", None, card::HISTORY),
        entry(w.selector_diagnostics, "", None, card::DIAGNOSTICS),
    ]
}

/// Home's Keep awake items (spec section 3): the service not running, a
/// session holding the PC (informational), and a backend that cannot stop
/// sleep.
fn attention(snapshot: &Snapshot) -> Vec<AttentionItem> {
    let w = copy::page_copy(snapshot.locale);
    let item = |severity, row, header: &str, description: String, label, action| AttentionItem {
        source: Route::KeepAwake,
        severity,
        row,
        header: header.to_string(),
        description,
        action_label: label,
        action,
    };
    match &snapshot.keep_awake {
        Facts::Quiet => Vec::new(),
        Facts::Stopped { offers_start } => vec![item(
            Severity::Warning,
            row::KEEP_AWAKE_SERVICE_STOPPED,
            w.home_stopped_header,
            w.home_stopped_description.to_string(),
            if *offers_start {
                w.start_service
            } else {
                w.view
            },
            if *offers_start {
                AttentionAction::StartKeepAwakeService
            } else {
                AttentionAction::Open(Page::KeepAwake)
            },
        )],
        Facts::Running {
            active,
            cannot_stop_sleep,
            ..
        } => {
            let mut items = Vec::new();
            if let Some(reason) = cannot_stop_sleep {
                items.push(item(
                    Severity::Warning,
                    row::KEEP_AWAKE_CANNOT_STOP_SLEEP,
                    w.home_cannot_stop_header,
                    reason.clone(),
                    w.view,
                    AttentionAction::OpenKeepAwakeDiagnostics,
                ));
            }
            if let Some(active) = active {
                items.push(item(
                    Severity::Info,
                    row::KEEP_AWAKE_SESSION_ACTIVE,
                    w.home_active_header,
                    active.clone(),
                    w.view,
                    AttentionAction::Open(Page::KeepAwake),
                ));
            }
            items
        }
    }
}

/// A dot on the pane item while a session holds the PC awake.
fn badge(snapshot: &Snapshot) -> Option<NavBadge> {
    match &snapshot.keep_awake {
        Facts::Running {
            active: Some(_), ..
        } => Some(NavBadge::Dot),
        _ => None,
    }
}
