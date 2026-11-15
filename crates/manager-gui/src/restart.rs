//! What Better Manager carries over when an update restarts it (ticket 105),
//! with no GPUI in it.
//!
//! The view is the page, the way back from it, the component or tab it is
//! about, the search, and the pane's state. The change that just finished
//! travels with it, which is what lets Better Manager update itself: it
//! finishes the transaction, the runner records the result in the state file,
//! the window restarts into the new program, and the new window's rows say
//! "Updated to …" for that change. A page that cannot stand on its own after a
//! restart opens the place it belongs to instead.
//!
//! A restart waits while a transaction, a repair, or the startup pass that
//! starts the record's user units is running.

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
            Page::Health => PageKey::Health,
            Page::Activity => PageKey::Activity,
            Page::Settings => PageKey::Settings,
        }
    }

    /// The page this key names, given what the new window has. A component
    /// page needs a component the catalog knows. The Defaults flow's review
    /// and results are worked out from the Defaults section's live state,
    /// which a restart does not carry, so they open the section itself.
    ///
    /// The review, installing, result, and restore pages are gone (owner,
    /// 2026-11-15); a hand-off from a version that still had them opens
    /// Updates, where the rows say what happened, and the doctor's results
    /// open Health, which now shows them.
    pub(crate) fn page(&self, knows: &dyn Fn(&ComponentId) -> bool) -> Page {
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
            PageKey::Updates
            | PageKey::ReviewChanges
            | PageKey::Installing
            | PageKey::Finished
            | PageKey::Restore
            | PageKey::Restored => Page::Updates,
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
    /// A tab of an older version; its contents are on Overview now.
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
        }
    }

    pub(crate) fn tab(self) -> DetailTab {
        match self {
            TabKey::Versions => DetailTab::Versions,
            TabKey::Overview | TabKey::Permissions => DetailTab::Overview,
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
    /// The plan an older version's review page was showing. Read and
    /// ignored: a change starts at once now, and a running one is in the
    /// state file.
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
/// window's own `transaction_running()`.
pub(crate) fn restart_blocked(
    transaction_running: bool,
    repairing: bool,
    healing: bool,
) -> Option<Blocker> {
    (transaction_running || repairing || healing).then_some(Blocker::Task)
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
            assert_eq!(read.page(&knows), page, "{text}");
        }
    }

    #[test]
    fn an_older_version_s_flow_pages_open_updates_and_health() {
        let knows = |_: &ComponentId| true;
        for name in [
            "review_changes",
            "installing",
            "finished",
            "restore",
            "restored",
        ] {
            let key: PageKey = serde_json::from_str(&format!(r#"{{"page":"{name}"}}"#)).unwrap();
            assert_eq!(key.page(&knows), Page::Updates, "{name}");
        }
        let doctor: PageKey = serde_json::from_str(r#"{"page":"doctor_results"}"#).unwrap();
        assert_eq!(doctor.page(&knows), Page::Health);
    }

    #[test]
    fn a_component_the_catalog_does_not_know_opens_its_section() {
        let knows = |_: &ComponentId| false;
        assert_eq!(
            PageKey::ComponentDetail("better-files".into()).page(&knows),
            Page::Components
        );
        assert_eq!(
            PageKey::DefaultsComponent("better-files".into()).page(&knows),
            Page::Defaults
        );
        // Not a component's shape at all.
        let all = |_: &ComponentId| true;
        assert_eq!(
            PageKey::ComponentDetail("../etc".into()).page(&all),
            Page::Components
        );
    }

    #[test]
    fn pages_worked_out_from_live_state_open_their_section() {
        let knows = |_: &ComponentId| true;
        assert_eq!(
            PageKey::of(&Page::DefaultsReview).page(&knows),
            Page::Defaults
        );
        assert_eq!(
            PageKey::of(&Page::DefaultsResults).page(&knows),
            Page::Defaults
        );
    }

    #[test]
    fn tabs_and_filters_round_trip() {
        for tab in [DetailTab::Overview, DetailTab::Versions] {
            assert_eq!(TabKey::of(tab).tab(), tab);
        }
        // The files tab of an older version opens Overview, which shows them.
        assert_eq!(TabKey::Permissions.tab(), DetailTab::Overview);
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
    fn a_transaction_or_a_repair_holds_the_restart() {
        assert_eq!(restart_blocked(false, false, false), None);
        for flags in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert_eq!(
                restart_blocked(flags.0, flags.1, flags.2),
                Some(Blocker::Task),
                "{flags:?}"
            );
        }
    }
}
