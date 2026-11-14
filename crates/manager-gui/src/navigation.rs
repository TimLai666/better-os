//! Where the control center is, where it has been, and how wide its pane is,
//! with no GPUI in sight.
//!
//! The window has nine destinations in its navigation pane (spec section 1,
//! "Navigation items"); a [`Page`] is a finer location inside one of them, such
//! as a component's details inside Apps. The back button walks a history of
//! pages, as Windows 11 Settings' does, and skips the screens of a running
//! flow (review, installing, results) that make no sense to return to.

use crate::model::Page;

/// Below this logical width the pane is compact, 48px behind ☰, and opens
/// over the content (spec section 1, audit M5).
pub(crate) const COMPACT_BELOW: f32 = 1000.0;
/// The widest the content column grows, as in Windows 11 Settings.
pub(crate) const CONTENT_MAX_WIDTH: f32 = 1000.0;
/// How far back the back button remembers. Older pages are forgotten first.
pub(crate) const HISTORY_LIMIT: usize = 64;

/// One item of the navigation pane, in the spec's order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Route {
    Home,
    System,
    /// Ticket 106's page, its own item until ticket 86's System page can
    /// hold it as a section.
    WindowAttention,
    Touchpad,
    KeepAwake,
    Apps,
    DefaultApps,
    Fixes,
    /// In the pane's footer.
    Settings,
}

impl Route {
    /// The pane's items above the footer, top to bottom.
    pub(crate) const PANE: [Route; 8] = [
        Route::Home,
        Route::System,
        Route::WindowAttention,
        Route::Touchpad,
        Route::KeepAwake,
        Route::Apps,
        Route::DefaultApps,
        Route::Fixes,
    ];
    /// Every route: the pane's items, then the footer.
    pub(crate) const ALL: [Route; 9] = [
        Route::Home,
        Route::System,
        Route::WindowAttention,
        Route::Touchpad,
        Route::KeepAwake,
        Route::Apps,
        Route::DefaultApps,
        Route::Fixes,
        Route::Settings,
    ];

    /// Where the route sits in the pane, counting the items and then the
    /// footer as one list, which is how `NavigationView` indexes them.
    pub(crate) fn pane_index(self) -> usize {
        Route::ALL
            .iter()
            .position(|route| *route == self)
            .expect("every route is in ALL")
    }

    pub(crate) fn from_pane_index(index: usize) -> Option<Route> {
        Route::ALL.get(index).copied()
    }

    /// The pane item a page belongs to.
    pub(crate) fn of(page: &Page) -> Route {
        match page {
            Page::Home => Route::Home,
            Page::System => Route::System,
            Page::WindowAttention => Route::WindowAttention,
            Page::Touchpad => Route::Touchpad,
            Page::KeepAwake => Route::KeepAwake,
            Page::Fixes => Route::Fixes,
            Page::Settings => Route::Settings,
            Page::Defaults
            | Page::DefaultsComponent(_)
            | Page::DefaultsReview
            | Page::DefaultsResults => Route::DefaultApps,
            Page::Components
            | Page::ComponentDetail(_)
            | Page::Updates
            | Page::ReviewChanges
            | Page::Installing
            | Page::Finished
            | Page::Restore
            | Page::Restored
            | Page::Health
            | Page::DoctorResults
            | Page::Activity => Route::Apps,
        }
    }

    /// The page a click on the route's pane item opens.
    pub(crate) fn landing(self) -> Page {
        match self {
            Route::Home => Page::Home,
            Route::System => Page::System,
            Route::WindowAttention => Page::WindowAttention,
            Route::Touchpad => Page::Touchpad,
            Route::KeepAwake => Page::KeepAwake,
            Route::Apps => Page::Components,
            Route::DefaultApps => Page::Defaults,
            Route::Fixes => Page::Fixes,
            Route::Settings => Page::Settings,
        }
    }
}

/// Whether a page is a step of a running flow rather than a place. Leaving
/// one does not put it in the history: going back to a review that was
/// already approved, or to an installation that has finished, would show a
/// screen about something that is over.
pub(crate) fn is_transient(page: &Page) -> bool {
    matches!(
        page,
        Page::ReviewChanges
            | Page::Installing
            | Page::Finished
            | Page::Restore
            | Page::Restored
            | Page::DefaultsReview
            | Page::DefaultsResults
    )
}

/// The pages the back button returns to, most recent last.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct History {
    back: Vec<Page>,
}

impl History {
    /// Records leaving `from` for `to`. Staying put records nothing, and a
    /// flow step is never recorded.
    pub(crate) fn visit(&mut self, from: &Page, to: &Page) {
        if from == to || is_transient(from) {
            return;
        }
        if self.back.last() != Some(from) {
            self.back.push(from.clone());
        }
        if self.back.len() > HISTORY_LIMIT {
            let excess = self.back.len() - HISTORY_LIMIT;
            self.back.drain(..excess);
        }
    }

    /// The page to go back to from `current`, taken off the history.
    pub(crate) fn back(&mut self, current: &Page) -> Option<Page> {
        while let Some(page) = self.back.pop() {
            if &page != current {
                return Some(page);
            }
        }
        None
    }

    pub(crate) fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    /// The pages, oldest first, for a restart to carry (ticket 105).
    pub(crate) fn pages(&self) -> &[Page] {
        &self.back
    }

    /// A history a restart carried, with the transient pages it can no longer
    /// return to left out.
    pub(crate) fn from_pages(pages: Vec<Page>) -> History {
        let mut back: Vec<Page> = pages.into_iter().filter(|p| !is_transient(p)).collect();
        if back.len() > HISTORY_LIMIT {
            let excess = back.len() - HISTORY_LIMIT;
            back.drain(..excess);
        }
        History { back }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.back.len()
    }
}

/// Whether a transaction is running, which changes where Back and Apps go.
///
/// While one runs its progress screen is the one place it can be followed
/// and cancelled, so Back from anywhere else returns to it, Back on it is not
/// offered, and the Apps item lands on it rather than on the component list.
pub(crate) fn back_target(history: &mut History, current: &Page, running: bool) -> Option<Page> {
    if running {
        (current != &Page::Installing).then_some(Page::Installing)
    } else {
        history.back(current)
    }
}

/// Whether the titlebar shows Back, by the same rule as [`back_target`].
pub(crate) fn shows_back(history: &History, current: &Page, running: bool) -> bool {
    if running {
        current != &Page::Installing
    } else {
        history.can_go_back()
    }
}

/// The page a pane item opens, with a transaction running or not.
pub(crate) fn landing(route: Route, running: bool) -> Page {
    if running && route == Route::Apps {
        Page::Installing
    } else {
        route.landing()
    }
}

/// Whether today's Better Manager screens draw their narrow layout: only in
/// a narrow window, never because the new pane is beside them.
pub(crate) fn legacy_compact(width: f32) -> bool {
    is_narrow(width)
}

/// How the pane is drawn at one window width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PaneLayout {
    /// 48px, icons only.
    pub(crate) compact: bool,
    /// When open, the pane floats over the content instead of pushing it.
    pub(crate) overlay: bool,
}

/// The pane at `width` logical pixels. A wide window's pane is open unless
/// the person closed it with ☰; a narrow window's is compact unless the
/// person opened it, and then it floats so the content keeps its width.
pub(crate) fn pane_layout(width: f32, wide_collapsed: bool, narrow_open: bool) -> PaneLayout {
    if width < COMPACT_BELOW {
        PaneLayout {
            compact: !narrow_open,
            overlay: true,
        }
    } else {
        PaneLayout {
            compact: wide_collapsed,
            overlay: false,
        }
    }
}

/// Whether a logical width is a narrow window's.
pub(crate) fn is_narrow(width: f32) -> bool {
    width < COMPACT_BELOW
}

#[cfg(test)]
mod tests {
    use super::*;
    use better_core::ComponentId;

    fn component(id: &str) -> Page {
        Page::ComponentDetail(ComponentId::new(id).expect("a valid id"))
    }

    #[test]
    fn the_pane_lists_the_nine_items_in_the_spec_order() {
        assert_eq!(Route::ALL.len(), 9);
        assert_eq!(Route::ALL[..8], Route::PANE);
        assert_eq!(Route::ALL[8], Route::Settings);
        for (index, route) in Route::ALL.iter().enumerate() {
            assert_eq!(route.pane_index(), index);
            assert_eq!(Route::from_pane_index(index), Some(*route));
            assert_eq!(Route::of(&route.landing()), *route);
        }
        assert_eq!(Route::from_pane_index(9), None);
    }

    #[test]
    fn every_page_belongs_to_one_pane_item() {
        assert_eq!(Route::of(&component("better-files")), Route::Apps);
        assert_eq!(Route::of(&Page::Installing), Route::Apps);
        assert_eq!(Route::of(&Page::Activity), Route::Apps);
        assert_eq!(Route::of(&Page::DefaultsReview), Route::DefaultApps);
        assert_eq!(Route::of(&Page::Settings), Route::Settings);
    }

    #[test]
    fn back_returns_through_the_pages_visited_and_then_stops() {
        let mut history = History::default();
        assert!(!history.can_go_back());
        history.visit(&Page::Home, &Page::Components);
        history.visit(&Page::Components, &component("better-files"));
        assert!(history.can_go_back());
        assert_eq!(
            history.back(&component("better-files")),
            Some(Page::Components)
        );
        assert_eq!(history.back(&Page::Components), Some(Page::Home));
        assert_eq!(history.back(&Page::Home), None);
        assert!(!history.can_go_back());
    }

    #[test]
    fn staying_put_and_repeats_add_nothing() {
        let mut history = History::default();
        history.visit(&Page::Home, &Page::Home);
        assert_eq!(history.len(), 0);
        history.visit(&Page::Home, &Page::Settings);
        history.visit(&Page::Home, &Page::System);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn a_running_flow_is_left_out_of_the_history() {
        let mut history = History::default();
        history.visit(&Page::Components, &Page::ReviewChanges);
        history.visit(&Page::ReviewChanges, &Page::Installing);
        history.visit(&Page::Installing, &Page::Finished);
        history.visit(&Page::Finished, &Page::Home);
        assert_eq!(history.back(&Page::Home), Some(Page::Components));
        assert_eq!(history.back(&Page::Components), None);
    }

    #[test]
    fn back_skips_a_recorded_copy_of_the_current_page() {
        let mut history = History::default();
        history.visit(&Page::Home, &Page::Settings);
        history.visit(&Page::Settings, &Page::Home);
        assert_eq!(history.back(&Page::Home), Some(Page::Settings));
        assert_eq!(history.back(&Page::Settings), Some(Page::Home));
    }

    #[test]
    fn a_running_transaction_is_where_back_and_apps_lead() {
        let mut history = History::default();
        history.visit(&Page::Components, &Page::ReviewChanges);
        history.visit(&Page::ReviewChanges, &Page::Installing);
        history.visit(&Page::Installing, &Page::Home);
        // On the progress screen there is no Back while it runs.
        assert!(!shows_back(&history, &Page::Installing, true));
        assert_eq!(back_target(&mut history, &Page::Installing, true), None);
        // Anywhere else, Back returns to it.
        assert!(shows_back(&history, &Page::Home, true));
        assert_eq!(
            back_target(&mut history, &Page::Home, true),
            Some(Page::Installing)
        );
        assert_eq!(landing(Route::Apps, true), Page::Installing);
        assert_eq!(landing(Route::Home, true), Page::Home);
        // Once it is over, the history is what it was.
        assert_eq!(landing(Route::Apps, false), Page::Components);
        assert_eq!(
            back_target(&mut history, &Page::Home, false),
            Some(Page::Components)
        );
    }

    #[test]
    fn today_s_screens_are_compact_only_in_a_narrow_window() {
        assert!(!legacy_compact(1600.0));
        assert!(!legacy_compact(1000.0));
        assert!(legacy_compact(960.0));
    }

    #[test]
    fn the_history_forgets_its_oldest_pages_first() {
        let mut history = History::default();
        let pages = [Page::Home, Page::Settings];
        for step in 0..(HISTORY_LIMIT * 2) {
            history.visit(&pages[step % 2], &pages[(step + 1) % 2]);
        }
        assert_eq!(history.len(), HISTORY_LIMIT);
    }

    /// Audit M5: at 960 the pane is 48px and still there; opening it floats
    /// it over the content. At 1600 it is open unless closed.
    #[test]
    fn the_pane_is_compact_below_one_thousand_and_never_gone() {
        assert_eq!(
            pane_layout(960.0, false, false),
            PaneLayout {
                compact: true,
                overlay: true
            }
        );
        assert_eq!(
            pane_layout(960.0, false, true),
            PaneLayout {
                compact: false,
                overlay: true
            }
        );
        assert_eq!(
            pane_layout(1600.0, false, true),
            PaneLayout {
                compact: false,
                overlay: false
            }
        );
        assert_eq!(
            pane_layout(1600.0, true, false),
            PaneLayout {
                compact: true,
                overlay: false
            }
        );
        assert!(is_narrow(999.0));
        assert!(!is_narrow(1000.0));
    }
}
