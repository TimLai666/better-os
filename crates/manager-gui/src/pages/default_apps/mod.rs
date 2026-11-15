//! Default apps (ticket 89, spec section 8): Better Defaults inside the
//! control center. Which Better OS components the system opens files, links,
//! and features with, and changing that only through a review, with the
//! previous value captured before the change, read back after it, and put
//! back from what was captured.
//!
//! `model` decides every state, preselection, and outcome without GPUI, and
//! is where [`model::ApprovedPlan`] can only be built from a review; `jobs`
//! holds the page's state and runs `defaults-core` off the UI thread, and is
//! the one place an approved plan is executed; `render` draws it with the
//! Fluent kit; `copy` holds the words the page adds to today's Defaults copy.

pub(crate) mod copy;
pub(crate) mod jobs;
pub(crate) mod model;
pub(crate) mod render;
#[cfg(test)]
mod tests;

use crate::attention::{AttentionAction, AttentionItem, Severity, Snapshot, row};
use crate::i18n::{Locale, copy as legacy, shell};
use crate::model::Page;
use crate::navigation::Route;
use crate::pages::{PageModule, PageStatus};
use crate::search::SearchEntry;

/// The ids of the page's cards, which the search scrolls to.
pub(crate) mod card {
    /// The summary line under the title.
    pub(crate) const SUMMARY: &str = "default-apps-summary";
    /// The command bar.
    pub(crate) const COMMANDS: &str = "default-apps-commands";
    /// The first component's expander.
    pub(crate) const COMPONENTS: &str = "default-apps-components";
}

pub(crate) const PAGE: PageModule = PageModule {
    route: Route::DefaultApps,
    name: "default-apps",
    status: PageStatus::Built,
    title: |s| s.default_apps,
    search_entries,
    attention,
    badge: |_| None,
};

fn search_entries(locale: Locale) -> Vec<SearchEntry> {
    let c = legacy(locale);
    let w = copy::page_copy(locale);
    let page = shell(locale).default_apps;
    let entry = |header, description, card| SearchEntry {
        header,
        description,
        page,
        group: None,
        route: Route::DefaultApps,
        card,
    };
    vec![
        entry(page, w.description, card::SUMMARY),
        entry(c.defaults_changed_externally, "", card::SUMMARY),
        entry(c.use_better_defaults, "", card::COMMANDS),
        entry(c.restore_previous_defaults, "", card::COMMANDS),
        entry(c.verify_again, "", card::COMMANDS),
        entry(c.integrations_label, c.defaults_subtitle, card::COMPONENTS),
    ]
}

/// Home's Default apps item (spec section 3): settings that another program
/// changed after Better OS set them.
fn attention(snapshot: &Snapshot) -> Vec<AttentionItem> {
    let n = snapshot.default_apps_changed;
    if n == 0 {
        return Vec::new();
    }
    let w = copy::page_copy(snapshot.locale);
    vec![AttentionItem {
        source: Route::DefaultApps,
        severity: Severity::Warning,
        row: row::DEFAULT_APPS_CHANGED,
        header: w.home_changed_header.to_string(),
        description: if n == 1 {
            w.home_changed_description_one.to_string()
        } else {
            w.home_changed_description.replace("{n}", &n.to_string())
        },
        action_label: w.view,
        action: AttentionAction::Open(Page::Defaults),
    }]
}
