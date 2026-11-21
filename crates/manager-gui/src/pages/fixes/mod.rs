//! Fixes (ticket 90, spec section 9): Better Patch as a page. It lists the
//! known faults this PC has, says what is wrong in plain words, fixes it,
//! checks the fix, and undoes it.
//!
//! `patch-core` decides everything and `patch-platform` reads and changes
//! the machine; `model` turns their reports into the page's groups and
//! words without GPUI, `state` runs every check, fix, and undo off the UI
//! thread, `render` draws it with the Fluent kit, and `copy` holds the
//! words.

pub(crate) mod copy;
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

pub(crate) use model::FixesFacts;

/// The ids of the page's cards, which the search scrolls to.
pub(crate) mod card {
    /// The description and Check again.
    pub(crate) const CHECK: &str = "fixes-check";
    pub(crate) const FOUND: &str = "fixes-found";
    pub(crate) const APPLIED: &str = "fixes-applied";
    pub(crate) const FONT_CACHE: &str = "fixes-known-fontconfig-cache";
    pub(crate) const ELECTRON_CURSOR: &str = "fixes-known-electron-cursor";
    pub(crate) const USB_C_INTERFACE: &str = "fixes-known-usb-c-interface";
    pub(crate) const USB_CONTROLLER: &str = "fixes-known-usb-controller";
}

pub(crate) const PAGE: PageModule = PageModule {
    route: Route::Fixes,
    name: "fixes",
    status: PageStatus::Built,
    title: |s| s.fixes,
    search_entries,
    attention,
    badge,
};

fn search_entries(locale: Locale) -> Vec<SearchEntry> {
    let c = copy::copy(locale);
    let page = shell(locale).fixes;
    let entry = |header, description, group, card| SearchEntry {
        header,
        description,
        page,
        group,
        route: Route::Fixes,
        card,
    };
    vec![
        entry(page, c.description, None, card::CHECK),
        entry(c.check_again, c.description, None, card::CHECK),
        entry(c.group_found, "", None, card::FOUND),
        entry(c.group_applied, "", None, card::APPLIED),
        entry(
            c.font_header,
            c.font_cause,
            Some(c.group_all),
            card::FONT_CACHE,
        ),
        entry(
            c.electron_header_any,
            c.electron_cause,
            Some(c.group_all),
            card::ELECTRON_CURSOR,
        ),
        entry(
            c.usb_header,
            c.usb_cause,
            Some(c.group_all),
            card::USB_C_INTERFACE,
        ),
        entry(
            c.controller_header,
            c.controller_cause,
            Some(c.group_all),
            card::USB_CONTROLLER,
        ),
        entry(
            c.reset_usb_c,
            c.usb_cause,
            Some(c.group_all),
            card::USB_C_INTERFACE,
        ),
    ]
}

/// Home's Fixes item (spec section 3): a known fault was found.
fn attention(snapshot: &Snapshot) -> Vec<AttentionItem> {
    let n = snapshot.fixes.found.len();
    let Some(first) = snapshot.fixes.first_title(snapshot.locale) else {
        return Vec::new();
    };
    let c = copy::copy(snapshot.locale);
    vec![AttentionItem {
        source: Route::Fixes,
        severity: Severity::Warning,
        row: row::FIXES_FOUND,
        header: if n == 1 {
            c.home_found_one.to_string()
        } else {
            c.home_found.replace("{n}", &n.to_string())
        },
        description: first,
        action_label: c.view,
        action: AttentionAction::Open(Page::Fixes),
    }]
}

/// The pane's count: the faults found, when above zero.
fn badge(snapshot: &Snapshot) -> Option<NavBadge> {
    let n = snapshot.fixes.found.len();
    (n > 0).then_some(NavBadge::Count(n))
}
