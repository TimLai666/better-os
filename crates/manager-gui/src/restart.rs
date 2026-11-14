//! What Better Manager carries over when an update restarts it (ticket 105),
//! with no GPUI in it.
//!
//! The view is the page, the way back from it, the component or tab it is
//! about, the search, and the pane's state. A page that is about a plan — the
//! review before a change, the result after one — carries the plan with it,
//! which is what lets Better Manager update itself: it finishes the
//! transaction, the runner records the result in the state file, the window
//! restarts into the new program, and the new window opens on the result page
//! for that plan. A page that cannot stand on its own after a restart opens
//! the place it belongs to instead.
//!
//! A restart waits while a transaction, a repair, or the startup pass that
//! starts the record's user units is running, and while the window is on the
//! installation screen, which is the moment between the transaction ending and
//! the result page appearing.

use better_core::ComponentId;
use better_ui::restart::Blocker;
use manager_core::TransactionPlan;
use serde::{Deserialize, Serialize};

use crate::model::{ActivityFilter, DetailTab, Page};

/// A page, as the hand-off spells it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "page", content = "component", rename_all = "snake_case")]
pub(crate) enum PageKey {
    Home,
    System,
    WindowAttention,
    Touchpad,
    KeepAwake,
    Fixes,
    Components,
    ComponentDetail(String),
    Defaults,
    DefaultsComponent(String),
    DefaultsReview,
    DefaultsResults,
    Updates,
    ReviewChanges,
    Installing,
    Finished,
    Restore,
    Restored,
    Health,
    DoctorResults,
    Activity,
    Settings,
}

impl PageKey {
    pub(crate) fn of(page: &Page) -> PageKey {
        match page {
            Page::Home => PageKey::Home,
            Page::System => PageKey::System,
            Page::WindowAttention => PageKey::WindowAttention,
            Page::Touchpad => PageKey::Touchpad,
            Page::KeepAwake => PageKey::KeepAwake,
            Page::Fixes => PageKey::Fixes,
            Page::Components => PageKey::Components,
            Page::ComponentDetail(id) => PageKey::ComponentDetail(id.to_string()),
            Page::Defaults => PageKey::Defaults,
            Page::DefaultsComponent(id) => PageKey::DefaultsComponent(id.to_string()),
            Page::DefaultsReview => PageKey::DefaultsReview,
            Page::DefaultsResults => PageKey::DefaultsResults,
            Page::Updates => PageKey::Updates,
            Page::ReviewChanges => PageKey::ReviewChanges,
            Page::Installing => PageKey::Installing,
            Page::Finished => PageKey::Finished,
            Page::Restore => PageKey::Restore,
            Page::Restored => PageKey::Restored,
            Page::Health => PageKey::Health,
            Page::DoctorResults => PageKey::DoctorResults,
            Page::Activity => PageKey::Activity,
            Page::Settings => PageKey::Settings,
        }
    }

    /// The page this key names, given what the new window has. A component
    /// page needs a component the catalog knows; a plan page needs its plan.
    /// The Defaults flow's review and results are worked out from the
    /// Defaults section's live state, which a restart does not carry, so they
    /// open the section itself; so does the doctor's results page.
    pub(crate) fn page(
        &self,
        knows: &dyn Fn(&ComponentId) -> bool,
        pending_plan: bool,
        finished_plan: bool,
    ) -> Page {
        let component = |id: &str| ComponentId::new(id).ok().filter(|id| knows(id));
        match self {
            PageKey::Home => Page::Home,
            PageKey::System => Page::System,
            PageKey::WindowAttention => Page::WindowAttention,
            PageKey::Touchpad => Page::Touchpad,
            PageKey::KeepAwake => Page::KeepAwake,
            PageKey::Fixes => Page::Fixes,
            PageKey::Components => Page::Components,
            PageKey::ComponentDetail(id) => match component(id) {
                Some(id) => Page::ComponentDetail(id),
                None => Page::Components,
            },
            PageKey::Defaults | PageKey::DefaultsReview | PageKey::DefaultsResults => {
                Page::Defaults
            }
            PageKey::DefaultsComponent(id) => match component(id) {
                Some(id) => Page::DefaultsComponent(id),
                None => Page::Defaults,
            },
            PageKey::Updates => Page::Updates,
            PageKey::ReviewChanges if pending_plan => Page::ReviewChanges,
            PageKey::Restore if pending_plan => Page::Restore,
            PageKey::ReviewChanges | PageKey::Restore => Page::Components,
            PageKey::Finished if finished_plan => Page::Finished,
            PageKey::Restored if finished_plan => Page::Restored,
            PageKey::Finished | PageKey::Restored => Page::Components,
            // The installation screen is never left for a restart; a window
            // that finds itself on it is the one the state file put there.
            PageKey::Installing => Page::Components,
            PageKey::Health | PageKey::DoctorResults => Page::Health,
            PageKey::Activity => Page::Activity,
            PageKey::Settings => Page::Settings,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TabKey {
    #[default]
    Overview,
    Versions,
    Permissions,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FilterKey {
    #[default]
    All,
    Success,
    Warning,
    Failure,
}

impl TabKey {
    pub(crate) fn of(tab: DetailTab) -> TabKey {
        match tab {
            DetailTab::Overview => TabKey::Overview,
            DetailTab::Versions => TabKey::Versions,
            DetailTab::Permissions => TabKey::Permissions,
        }
    }

    pub(crate) fn tab(self) -> DetailTab {
        match self {
            TabKey::Overview => DetailTab::Overview,
            TabKey::Versions => DetailTab::Versions,
            TabKey::Permissions => DetailTab::Permissions,
        }
    }
}

impl FilterKey {
    pub(crate) fn of(filter: ActivityFilter) -> FilterKey {
        match filter {
            ActivityFilter::All => FilterKey::All,
            ActivityFilter::Success => FilterKey::Success,
            ActivityFilter::Warning => FilterKey::Warning,
            ActivityFilter::Failure => FilterKey::Failure,
        }
    }

    pub(crate) fn filter(self) -> ActivityFilter {
        match self {
            FilterKey::All => ActivityFilter::All,
            FilterKey::Success => ActivityFilter::Success,
            FilterKey::Warning => ActivityFilter::Warning,
            FilterKey::Failure => ActivityFilter::Failure,
        }
    }
}

/// Everything the window puts back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ManagerView {
    pub(crate) page: PageKey,
    /// The back button's pages, oldest first.
    pub(crate) back: Vec<PageKey>,
    pub(crate) detail_tab: TabKey,
    pub(crate) activity_filter: FilterKey,
    pub(crate) search: String,
    pub(crate) nav_search: String,
    pub(crate) wide_collapsed: bool,
    pub(crate) narrow_open: bool,
    pub(crate) about_expanded: bool,
    pub(crate) update_check_requested: bool,
    pub(crate) scroll: f32,
    pub(crate) pending_plan: Option<TransactionPlan>,
    pub(crate) finished_plan: Option<TransactionPlan>,
}

impl Default for ManagerView {
    fn default() -> Self {
        ManagerView {
            page: PageKey::Home,
            back: Vec::new(),
            detail_tab: TabKey::Overview,
            activity_filter: FilterKey::All,
            search: String::new(),
            nav_search: String::new(),
            wide_collapsed: false,
            narrow_open: false,
            about_expanded: true,
            update_check_requested: false,
            scroll: 0.0,
            pending_plan: None,
            finished_plan: None,
        }
    }
}

/// What a restart would lose, if anything. `transaction_running` is the
/// window's own `transaction_running()`; `on_installing_page` covers the
/// moment after the transaction's last state was saved and before the result
/// page opened.
pub(crate) fn restart_blocked(
    transaction_running: bool,
    repairing: bool,
    healing: bool,
    on_installing_page: bool,
) -> Option<Blocker> {
    (transaction_running || repairing || healing || on_installing_page).then_some(Blocker::Task)
}

#[cfg(test)]
mod tests {
    use super::{Blocker, FilterKey, ManagerView, PageKey, TabKey, restart_blocked};
    use crate::model::{ActivityFilter, DetailTab, Page};
    use better_core::ComponentId;

    fn id(value: &str) -> ComponentId {
        ComponentId::new(value).unwrap()
    }

    fn every_page() -> Vec<Page> {
        vec![
            Page::Home,
            Page::System,
            Page::WindowAttention,
            Page::Touchpad,
            Page::KeepAwake,
            Page::Fixes,
            Page::Components,
            Page::ComponentDetail(id("better-files")),
            Page::Defaults,
            Page::DefaultsComponent(id("better-files")),
            Page::Updates,
            Page::ReviewChanges,
            Page::Finished,
            Page::Restore,
            Page::Restored,
            Page::Health,
            Page::Activity,
            Page::Settings,
        ]
    }

    #[test]
    fn every_page_that_stands_on_its_own_comes_back_as_itself() {
        let knows = |_: &ComponentId| true;
        for page in every_page() {
            let key = PageKey::of(&page);
            let text = serde_json::to_string(&key).unwrap();
            let read: PageKey = serde_json::from_str(&text).unwrap();
            assert_eq!(read.page(&knows, true, true), page, "{text}");
        }
    }

    #[test]
    fn a_plan_page_without_its_plan_opens_apps() {
        let knows = |_: &ComponentId| true;
        for key in [
            PageKey::ReviewChanges,
            PageKey::Restore,
            PageKey::Finished,
            PageKey::Restored,
            PageKey::Installing,
        ] {
            assert_eq!(key.page(&knows, false, false), Page::Components, "{key:?}");
        }
    }

    #[test]
    fn a_component_the_catalog_does_not_know_opens_its_section() {
        let knows = |_: &ComponentId| false;
        assert_eq!(
            PageKey::ComponentDetail("better-files".into()).page(&knows, false, false),
            Page::Components
        );
        assert_eq!(
            PageKey::DefaultsComponent("better-files".into()).page(&knows, false, false),
            Page::Defaults
        );
        // Not a component's shape at all.
        let all = |_: &ComponentId| true;
        assert_eq!(
            PageKey::ComponentDetail("../etc".into()).page(&all, false, false),
            Page::Components
        );
    }

    #[test]
    fn pages_worked_out_from_live_state_open_their_section() {
        let knows = |_: &ComponentId| true;
        assert_eq!(
            PageKey::of(&Page::DefaultsReview).page(&knows, true, true),
            Page::Defaults
        );
        assert_eq!(
            PageKey::of(&Page::DefaultsResults).page(&knows, true, true),
            Page::Defaults
        );
        assert_eq!(
            PageKey::of(&Page::DoctorResults).page(&knows, true, true),
            Page::Health
        );
    }

    #[test]
    fn tabs_and_filters_round_trip() {
        for tab in [
            DetailTab::Overview,
            DetailTab::Versions,
            DetailTab::Permissions,
        ] {
            assert_eq!(TabKey::of(tab).tab(), tab);
        }
        for filter in [
            ActivityFilter::All,
            ActivityFilter::Success,
            ActivityFilter::Warning,
            ActivityFilter::Failure,
        ] {
            assert_eq!(FilterKey::of(filter).filter(), filter);
        }
    }

    #[test]
    fn a_view_from_another_version_still_reads() {
        let view: ManagerView =
            serde_json::from_str(r#"{"page":{"page":"settings"},"later_field":true}"#).unwrap();
        assert_eq!(view.page, PageKey::Settings);
        assert!(view.about_expanded, "a missing field takes its default");
    }

    #[test]
    fn a_transaction_a_repair_or_the_installation_screen_holds_the_restart() {
        assert_eq!(restart_blocked(false, false, false, false), None);
        for flags in [
            (true, false, false, false),
            (false, true, false, false),
            (false, false, true, false),
            (false, false, false, true),
        ] {
            assert_eq!(
                restart_blocked(flags.0, flags.1, flags.2, flags.3),
                Some(Blocker::Task),
                "{flags:?}"
            );
        }
    }
}
