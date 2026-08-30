//! Tabs, and the recently-closed list that makes closing one recoverable.
//!
//! A tab is its own history and its own view preferences, so switching tabs
//! restores where that tab was rather than where the window was. Closing one
//! keeps its whole history in a bounded recently-closed stack, which is what
//! lets reopening a closed tab put Back and Forward back the way they were
//! instead of dropping the user at the folder with no history.
//!
//! The window always has at least one tab. Closing the last one is refused
//! rather than silently leaving a window with nothing in it.

use crate::error::NavigationError;
use crate::hidden::HiddenPreference;
use crate::history::History;
use crate::location::Location;
use crate::sort::SortOrder;

/// How many closed tabs are recoverable.
pub const DEFAULT_CLOSED_TAB_LIMIT: usize = 16;

/// Identifies a tab for the lifetime of a window.
///
/// Ids are never reused, so a stale reference to a closed tab is a
/// [`NavigationError::NoSuchTab`] rather than a silent hit on a different tab
/// that happens to have taken the same slot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TabId(u64);

impl TabId {
    pub fn value(self) -> u64 {
        self.0
    }
}

/// The view settings a tab carries.
///
/// Issue #6 defers the per-folder versus global preference policy to a
/// decision, so these live on the tab: it is the choice that neither
/// pre-empts the other, because a global default can be pushed into new tabs
/// and a per-folder rule can override one tab without changing the model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ViewPreferences {
    pub order: SortOrder,
    pub hidden: HiddenPreference,
}

/// One tab.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tab {
    id: TabId,
    history: History,
    preferences: ViewPreferences,
}

impl Tab {
    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn location(&self) -> &Location {
        self.history.current()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    pub fn preferences(&self) -> ViewPreferences {
        self.preferences
    }

    pub fn preferences_mut(&mut self) -> &mut ViewPreferences {
        &mut self.preferences
    }

    /// The label the tab strip shows.
    pub fn title(&self) -> String {
        self.history.current().display_name()
    }
}

/// A tab that was closed, kept whole so reopening restores its history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosedTab {
    history: History,
    preferences: ViewPreferences,
    /// Where it sat in the strip, so it reopens where it was rather than at
    /// the end.
    index: usize,
}

impl ClosedTab {
    pub fn location(&self) -> &Location {
        self.history.current()
    }
}

/// Every tab in one window.
#[derive(Clone, Debug)]
pub struct TabSet {
    tabs: Vec<Tab>,
    active: usize,
    closed: Vec<ClosedTab>,
    closed_limit: usize,
    next_id: u64,
}

impl TabSet {
    /// Opens a window with one tab.
    pub fn new(location: Location) -> Self {
        Self::with_preferences(location, ViewPreferences::default())
    }

    pub fn with_preferences(location: Location, preferences: ViewPreferences) -> Self {
        let first = Tab {
            id: TabId(1),
            history: History::new(location),
            preferences,
        };
        Self {
            tabs: vec![first],
            active: 0,
            closed: Vec::new(),
            closed_limit: DEFAULT_CLOSED_TAB_LIMIT,
            next_id: 2,
        }
    }

    pub fn with_closed_limit(mut self, limit: usize) -> Self {
        self.closed_limit = limit;
        self.trim_closed();
        self
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Always false. Present because clippy asks for it next to `len`, and
    /// because it documents the invariant: a tab set is never empty.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    /// Opens a tab after the active one, which is where a "open in new tab"
    /// action puts it. The new tab inherits the active tab's view
    /// preferences, so a user who set a sort order does not get the default
    /// back on every new tab.
    pub fn open(&mut self, location: Location, activate: bool) -> TabId {
        let preferences = self.active().preferences;
        let id = TabId(self.next_id);
        self.next_id += 1;
        let index = self.active + 1;
        self.tabs.insert(
            index,
            Tab {
                id,
                history: History::new(location),
                preferences,
            },
        );
        if activate {
            self.active = index;
        } else if index <= self.active {
            self.active += 1;
        }
        id
    }

    /// Closes a tab, keeping it recoverable.
    pub fn close(&mut self, id: TabId) -> Result<(), NavigationError> {
        if self.tabs.len() == 1 {
            return Err(NavigationError::LastTab);
        }
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .ok_or(NavigationError::NoSuchTab(id.0))?;
        let tab = self.tabs.remove(index);
        self.closed.push(ClosedTab {
            history: tab.history,
            preferences: tab.preferences,
            index,
        });
        self.trim_closed();
        if self.active > index || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        Ok(())
    }

    pub fn activate(&mut self, id: TabId) -> Result<(), NavigationError> {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .ok_or(NavigationError::NoSuchTab(id.0))?;
        self.active = index;
        Ok(())
    }

    pub fn can_restore(&self) -> bool {
        !self.closed.is_empty()
    }

    /// What reopening would restore, without restoring it. A menu shows this.
    pub fn peek_closed(&self) -> Option<&ClosedTab> {
        self.closed.last()
    }

    /// Reopens the most recently closed tab with its history intact, at the
    /// position it was closed from.
    pub fn restore_closed(&mut self) -> Result<TabId, NavigationError> {
        let closed = self.closed.pop().ok_or(NavigationError::NothingToRestore)?;
        let id = TabId(self.next_id);
        self.next_id += 1;
        let index = closed.index.min(self.tabs.len());
        self.tabs.insert(
            index,
            Tab {
                id,
                history: closed.history,
                preferences: closed.preferences,
            },
        );
        self.active = index;
        Ok(id)
    }

    fn trim_closed(&mut self) {
        if self.closed.len() > self.closed_limit {
            let excess = self.closed.len() - self.closed_limit;
            self.closed.drain(0..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(path: &str) -> Location {
        Location::local(path).unwrap()
    }

    #[test]
    fn a_new_tab_opens_beside_the_active_one() {
        let mut tabs = TabSet::new(at("/home"));
        let second = tabs.open(at("/etc"), true);
        tabs.open(at("/var"), false);
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs.active().id(), second);
        assert_eq!(
            tabs.tabs()
                .iter()
                .map(|tab| tab.location().to_uri())
                .collect::<Vec<_>>(),
            ["file:///home", "file:///etc", "file:///var"]
        );
    }

    #[test]
    fn the_last_tab_cannot_be_closed() {
        let mut tabs = TabSet::new(at("/home"));
        let id = tabs.active().id();
        assert_eq!(tabs.close(id), Err(NavigationError::LastTab));
        assert_eq!(tabs.len(), 1);
    }

    #[test]
    fn a_reopened_tab_keeps_its_history_and_its_position() {
        let mut tabs = TabSet::new(at("/home"));
        let second = tabs.open(at("/etc"), true);
        tabs.open(at("/var"), false);
        tabs.active_mut().history_mut().visit(at("/etc/apt"));
        tabs.active_mut()
            .history_mut()
            .visit(at("/etc/apt/sources.list.d"));

        tabs.close(second).unwrap();
        assert_eq!(tabs.len(), 2);
        assert!(tabs.can_restore());
        assert_eq!(
            tabs.peek_closed().map(ClosedTab::location),
            Some(&at("/etc/apt/sources.list.d"))
        );

        let restored = tabs.restore_closed().unwrap();
        assert_ne!(restored, second, "a reopened tab gets a fresh id");
        assert_eq!(tabs.active_index(), 1);
        let history = tabs.active().history().clone();
        assert!(history.can_go_back());
        let mut history = history;
        assert_eq!(history.back(), Some(&at("/etc/apt")));
        assert_eq!(history.back(), Some(&at("/etc")));
    }

    #[test]
    fn restoring_with_nothing_closed_is_refused() {
        let mut tabs = TabSet::new(at("/home"));
        assert_eq!(
            tabs.restore_closed(),
            Err(NavigationError::NothingToRestore)
        );
    }

    #[test]
    fn the_recently_closed_list_is_bounded() {
        let mut tabs = TabSet::new(at("/keep")).with_closed_limit(2);
        let mut ids = Vec::new();
        for index in 0..4 {
            ids.push(tabs.open(at(&format!("/tab{index}")), false));
        }
        for id in ids {
            tabs.close(id).unwrap();
        }
        assert_eq!(tabs.closed.len(), 2);
        assert!(tabs.restore_closed().is_ok());
        assert_eq!(tabs.active().location(), &at("/tab3"));
    }

    #[test]
    fn closing_the_active_tab_activates_a_neighbour_rather_than_nothing() {
        let mut tabs = TabSet::new(at("/a"));
        tabs.open(at("/b"), true);
        let active = tabs.active().id();
        tabs.close(active).unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.active().location(), &at("/a"));
    }

    #[test]
    fn a_stale_tab_id_is_reported_rather_than_hitting_another_tab() {
        let mut tabs = TabSet::new(at("/a"));
        let second = tabs.open(at("/b"), true);
        tabs.close(second).unwrap();
        tabs.open(at("/c"), true);
        assert_eq!(
            tabs.activate(second),
            Err(NavigationError::NoSuchTab(second.value()))
        );
    }

    #[test]
    fn a_new_tab_inherits_the_active_tabs_view_preferences() {
        let mut tabs = TabSet::new(at("/a"));
        tabs.active_mut().preferences_mut().hidden = HiddenPreference::showing_hidden();
        let second = tabs.open(at("/b"), true);
        assert!(tabs.get(second).unwrap().preferences().hidden.show_hidden);
    }
}
