//! What the overlay is showing, with no GPUI in it.
//!
//! Issue #2's central requirement is that browsing and searching are one
//! screen, not two modes. `launcher-core` already guarantees that at the level
//! of the query: an empty query *is* the application library. This model adds
//! the two things a screen needs on top of it — a selection and a state —
//! without reintroducing a mode.
//!
//! Three properties are worth stating because they are what the tests hold on
//! to.
//!
//! - **Clearing the query costs nothing.** The library rows are borrowed from
//!   the index's browse model, which was built once. Emptying the search row
//!   discards a result vector; it never rebuilds or re-clones the library.
//! - **The selection follows the application, not the position.** A live
//!   catalog update or a keystroke that reorders results keeps the selected
//!   application selected when it is still on screen, and falls back to the
//!   first row when it is not. A selection that quietly jumped to whatever now
//!   occupies index 3 is how someone launches the wrong thing.
//! - **A failed launch is state, not a log line.** [`OverlayModel::activate`]
//!   records the failure and reports that the overlay must stay open.

use app_catalog_core::DesktopId;
use launcher_core::{
    LauncherApplication, LauncherState, LauncherView, NoUsage, RankingOptions, SearchIndex,
};
use launcher_platform::{ApplicationStarter, LauncherSnapshot};

/// Whether the application list has been read yet, and whether it is being
/// re-read.
///
/// `Refreshing` is deliberately not `Loading`: an index rebuild after an
/// application was installed must not blank a screen the user is already using.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LoadState {
    #[default]
    Loading,
    Ready,
    Refreshing,
}

/// Something the overlay has to tell the user about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    /// The launch failed, carrying the platform's stable machine key. The
    /// wording is the locale's job; the key is what makes it diagnosable.
    LaunchFailed(String),
}

/// What a keystroke asks the selection to do.
///
/// Rows and columns are separate because the library is a grid: Down means the
/// next row, not the next application. The column count comes from the
/// rendered width, so this enum stays true whatever the window size is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Move {
    Next,
    Previous,
    NextRow,
    PreviousRow,
    First,
    Last,
}

/// What activating a row did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Activation {
    /// Started. The overlay has done its job and should close.
    Launched(DesktopId),
    /// Nothing was selected, so nothing happened and the overlay stays.
    NothingSelected,
    /// It failed. The overlay stays open, showing why.
    Failed(String),
}

/// Which rows are on screen. Kept as a two-variant enum rather than one vector
/// so the library is borrowed and only search results are owned.
#[derive(Clone, Debug, Default)]
enum Rows {
    #[default]
    Library,
    Results(Vec<LauncherApplication>),
}

/// The overlay's state.
#[derive(Debug, Default)]
pub struct OverlayModel {
    snapshot: Option<LauncherSnapshot>,
    query: LauncherState,
    rows: Rows,
    load: LoadState,
    /// The row the keyboard is on. Meaningless when there are no rows, which
    /// is why it is read through [`OverlayModel::selected`].
    selected: usize,
    columns: usize,
    notice: Option<Notice>,
}

impl OverlayModel {
    pub fn new() -> Self {
        Self {
            columns: 1,
            ..Self::default()
        }
    }

    pub fn load_state(&self) -> LoadState {
        self.load
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn query(&self) -> &str {
        self.query.query()
    }

    /// Whether the application library is what is on screen. This is a
    /// question about the query, not about a stored mode.
    pub fn is_browsing(&self) -> bool {
        self.query.is_browsing()
    }

    /// The applications on screen, in the order they are drawn.
    pub fn rows(&self) -> &[LauncherApplication] {
        match (&self.rows, &self.snapshot) {
            (Rows::Library, Some(snapshot)) => snapshot.index.browse().applications(),
            (Rows::Results(results), _) => results,
            (Rows::Library, None) => &[],
        }
    }

    /// Whether a typed query matched nothing. Distinct from an empty library:
    /// the two states say different things and are worded differently.
    pub fn is_empty_result(&self) -> bool {
        !self.is_browsing() && self.rows().is_empty()
    }

    /// Whether the machine has no applications to show at all.
    pub fn is_empty_library(&self) -> bool {
        self.is_browsing() && self.load != LoadState::Loading && self.rows().is_empty()
    }

    pub fn selected_index(&self) -> Option<usize> {
        if self.rows().is_empty() {
            None
        } else {
            Some(self.selected.min(self.rows().len() - 1))
        }
    }

    pub fn selected(&self) -> Option<&LauncherApplication> {
        self.selected_index().map(|index| &self.rows()[index])
    }

    /// How many tiles fit across. Set from the rendered width; never below one,
    /// because a grid zero tiles wide has no next row.
    pub fn set_columns(&mut self, columns: usize) {
        self.columns = columns.max(1);
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    /// The application list has been read. Keeps the query, keeps the selected
    /// application if it survived, and clears a stale launch failure.
    pub fn apply_snapshot(&mut self, snapshot: LauncherSnapshot) {
        let previously_selected = self
            .selected()
            .map(|application| application.desktop_id.clone());
        self.snapshot = Some(snapshot);
        self.load = LoadState::Ready;
        self.notice = None;
        self.recompute(previously_selected);
    }

    /// The application directories changed and a re-read is under way. The
    /// current rows stay on screen while it runs.
    pub fn begin_refresh(&mut self) {
        if self.load == LoadState::Ready {
            self.load = LoadState::Refreshing;
        }
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        let previously_selected = self
            .selected()
            .map(|application| application.desktop_id.clone());
        self.query.set_query(query);
        self.notice = None;
        self.recompute(previously_selected);
    }

    /// Emptying the search row. Returns to the library without touching the
    /// snapshot, which is what "without closing or reopening the window" means
    /// at this level.
    pub fn clear_query(&mut self) {
        self.set_query("");
    }

    /// Rebuilds the visible rows for the current query, then places the
    /// selection.
    fn recompute(&mut self, keep: Option<DesktopId>) {
        self.rows = match &self.snapshot {
            None => Rows::Library,
            Some(snapshot) => {
                match self
                    .query
                    .view(index_of(snapshot), &RankingOptions::default(), &NoUsage)
                {
                    LauncherView::Browse(_) => Rows::Library,
                    LauncherView::Search(results) => Rows::Results(
                        results
                            .results()
                            .iter()
                            .map(|result| result.application.clone())
                            .collect(),
                    ),
                }
            }
        };
        self.selected = keep
            .and_then(|desktop_id| {
                self.rows()
                    .iter()
                    .position(|application| application.desktop_id == desktop_id)
            })
            .unwrap_or(0);
    }

    /// Moves the keyboard selection. Clamps rather than wraps: wrapping from
    /// the last application back to the first is how a long press ends up
    /// somewhere nobody expected.
    pub fn move_selection(&mut self, movement: Move) {
        let Some(current) = self.selected_index() else {
            return;
        };
        let last = self.rows().len() - 1;
        let columns = self.columns;
        self.selected = match movement {
            Move::Next => (current + 1).min(last),
            Move::Previous => current.saturating_sub(1),
            Move::NextRow => (current + columns).min(last),
            Move::PreviousRow => current.saturating_sub(columns),
            Move::First => 0,
            Move::Last => last,
        };
    }

    /// Selects a row directly, which is what a pointer does.
    pub fn select_at(&mut self, index: usize) {
        if index < self.rows().len() {
            self.selected = index;
            self.notice = None;
        }
    }

    pub fn select_by_id(&mut self, desktop_id: &DesktopId) {
        if let Some(index) = self
            .rows()
            .iter()
            .position(|application| &application.desktop_id == desktop_id)
        {
            self.select_at(index);
        }
    }

    /// Launches the selected application through the shared platform path.
    ///
    /// A failure is recorded and reported rather than returned and forgotten:
    /// the overlay stays open showing what went wrong, because closing on a
    /// failed launch looks exactly like a successful one.
    pub fn activate(&mut self, starter: &dyn ApplicationStarter) -> Activation {
        let Some(desktop_id) = self
            .selected()
            .map(|application| application.desktop_id.clone())
        else {
            return Activation::NothingSelected;
        };
        match starter.start(&desktop_id) {
            Ok(_) => {
                self.notice = None;
                Activation::Launched(desktop_id)
            }
            Err(error) => {
                let key = error.to_string();
                self.notice = Some(Notice::LaunchFailed(key.clone()));
                Activation::Failed(key)
            }
        }
    }
}

fn index_of(snapshot: &LauncherSnapshot) -> &SearchIndex {
    &snapshot.index
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_catalog_core::{CatalogBuilder, DirectoryRank, EntryScope, NoProbe};
    use launcher_core::IndexOptions;
    use launcher_platform::{PlatformError, RecordingStarter};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Builds a snapshot from `(desktop id, name, keywords)` triples, through
    /// the real catalog builder and the real index. Nothing here fabricates an
    /// application record by hand.
    fn snapshot(entries: &[(&str, &str, &str)]) -> LauncherSnapshot {
        let mut builder = CatalogBuilder::new(&NoProbe);
        for (id, name, keywords) in entries {
            let body = format!(
                "[Desktop Entry]\nType=Application\nName={name}\nKeywords={keywords}\nExec={}\n",
                name.to_lowercase().replace(' ', "-")
            );
            builder.add_entry(
                DesktopId::new(*id).unwrap(),
                PathBuf::from(format!("/usr/share/applications/{id}")),
                &DirectoryRank {
                    rank: 0,
                    scope: EntryScope::System,
                },
                body.as_bytes(),
            );
        }
        let catalog = builder.build();
        let index = SearchIndex::from_catalog(&catalog, &IndexOptions::new());
        LauncherSnapshot {
            catalog: Arc::new(catalog),
            index: Arc::new(index),
        }
    }

    fn library() -> LauncherSnapshot {
        snapshot(&[
            ("archive.desktop", "Archive Manager", "zip;"),
            ("browser.desktop", "Browser", "web;"),
            ("calculator.desktop", "Calculator", "maths;"),
            ("editor.desktop", "Text Editor", "notes;"),
        ])
    }

    fn names(model: &OverlayModel) -> Vec<String> {
        model
            .rows()
            .iter()
            .map(|application| application.display_name.clone())
            .collect()
    }

    #[test]
    fn an_overlay_starts_loading_and_shows_nothing_it_has_not_read_yet() {
        let model = OverlayModel::new();
        assert_eq!(model.load_state(), LoadState::Loading);
        assert!(model.rows().is_empty());
        assert!(model.selected().is_none());
        assert!(
            !model.is_empty_library(),
            "a list that has not been read is not an empty list"
        );
    }

    #[test]
    fn typing_filters_in_place_and_clearing_restores_the_whole_library() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        assert_eq!(model.load_state(), LoadState::Ready);
        assert!(model.is_browsing());
        assert_eq!(
            names(&model),
            vec!["Archive Manager", "Browser", "Calculator", "Text Editor"]
        );

        model.set_query("cal");
        assert!(!model.is_browsing());
        assert_eq!(names(&model), vec!["Calculator"]);

        model.set_query("   ");
        assert!(
            model.is_browsing(),
            "a query of nothing but whitespace is what deleting leaves behind"
        );
        assert_eq!(names(&model).len(), 4);

        model.set_query("cal");
        model.clear_query();
        assert!(model.is_browsing());
        assert_eq!(
            names(&model),
            vec!["Archive Manager", "Browser", "Calculator", "Text Editor"],
            "the library comes back in the same order it left in"
        );
    }

    #[test]
    fn a_query_that_matches_nothing_is_an_empty_result_not_an_empty_library() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.set_query("qqqq");

        assert!(model.rows().is_empty());
        assert!(model.is_empty_result());
        assert!(!model.is_empty_library());

        model.clear_query();
        assert!(!model.is_empty_result());
        assert!(!model.is_empty_library());
    }

    #[test]
    fn a_machine_with_no_applications_reports_an_empty_library() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(snapshot(&[]));
        assert!(model.is_empty_library());
        assert!(!model.is_empty_result());
    }

    #[test]
    fn arrow_keys_walk_the_grid_by_row_and_never_leave_it() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.set_columns(2);
        assert_eq!(model.selected_index(), Some(0));

        model.move_selection(Move::Next);
        assert_eq!(model.selected().unwrap().display_name, "Browser");
        model.move_selection(Move::NextRow);
        assert_eq!(model.selected().unwrap().display_name, "Text Editor");
        model.move_selection(Move::PreviousRow);
        assert_eq!(model.selected().unwrap().display_name, "Browser");
        model.move_selection(Move::Previous);
        assert_eq!(model.selected_index(), Some(0));

        // Clamped at both ends rather than wrapping.
        model.move_selection(Move::Previous);
        model.move_selection(Move::PreviousRow);
        assert_eq!(model.selected_index(), Some(0));
        model.move_selection(Move::Last);
        assert_eq!(model.selected_index(), Some(3));
        model.move_selection(Move::Next);
        model.move_selection(Move::NextRow);
        assert_eq!(model.selected_index(), Some(3));
        model.move_selection(Move::First);
        assert_eq!(model.selected_index(), Some(0));
    }

    #[test]
    fn a_one_column_layout_makes_a_row_move_the_same_as_a_single_step() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.set_columns(0);
        assert_eq!(model.columns(), 1, "a grid is never zero tiles wide");
        model.move_selection(Move::NextRow);
        assert_eq!(model.selected_index(), Some(1));
    }

    #[test]
    fn typing_places_the_selection_on_the_first_result() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.move_selection(Move::Last);
        model.set_query("bro");
        assert_eq!(model.selected_index(), Some(0));
        assert_eq!(model.selected().unwrap().display_name, "Browser");
    }

    #[test]
    fn the_selection_follows_the_application_when_the_rows_change_under_it() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.set_query("e");
        let chosen = model
            .rows()
            .iter()
            .find(|application| application.display_name == "Text Editor")
            .unwrap()
            .desktop_id
            .clone();
        model.select_by_id(&chosen);
        assert_eq!(model.selected().unwrap().display_name, "Text Editor");

        // A narrower query keeps the same application, at a different position.
        model.set_query("edi");
        assert_eq!(model.selected().unwrap().display_name, "Text Editor");
    }

    #[test]
    fn an_installed_application_appears_without_disturbing_the_query() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.set_query("ma");
        let before = names(&model);
        assert!(before.iter().any(|name| name == "Archive Manager"));
        assert!(!before.iter().any(|name| name == "Mail"));

        model.begin_refresh();
        assert_eq!(
            model.load_state(),
            LoadState::Refreshing,
            "a re-read must not blank the screen someone is using"
        );
        assert_eq!(
            names(&model),
            before,
            "the rows stay until the new ones arrive"
        );

        let mut grown = vec![
            ("archive.desktop", "Archive Manager", "zip;"),
            ("browser.desktop", "Browser", "web;"),
            ("calculator.desktop", "Calculator", "maths;"),
            ("editor.desktop", "Text Editor", "notes;"),
        ];
        grown.push(("mail.desktop", "Mail", "mail;"));
        model.apply_snapshot(snapshot(&grown));

        assert_eq!(model.load_state(), LoadState::Ready);
        assert_eq!(model.query(), "ma", "the query survives a catalog swap");
        assert!(names(&model).iter().any(|name| name == "Mail"));
    }

    #[test]
    fn a_removed_application_leaves_the_selection_on_something_that_still_exists() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.move_selection(Move::Last);
        assert_eq!(model.selected().unwrap().display_name, "Text Editor");

        model.apply_snapshot(snapshot(&[
            ("archive.desktop", "Archive Manager", "zip;"),
            ("browser.desktop", "Browser", "web;"),
        ]));
        assert_eq!(
            model.selected().unwrap().display_name,
            "Archive Manager",
            "a selection that no longer exists falls back to the first row"
        );
    }

    #[test]
    fn a_successful_launch_reports_what_it_started_and_leaves_no_notice() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.set_query("cal");
        let starter = RecordingStarter::succeeding();

        let activation = model.activate(&starter);
        assert_eq!(
            activation,
            Activation::Launched(DesktopId::new("calculator.desktop").unwrap())
        );
        assert!(model.notice().is_none());
        assert_eq!(starter.started().len(), 1);
    }

    #[test]
    fn a_failed_launch_is_shown_and_the_overlay_stays_open() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        let starter = RecordingStarter::failing(PlatformError::UnknownApplication(
            "archive.desktop".to_string(),
        ));

        let activation = model.activate(&starter);
        let Activation::Failed(key) = activation else {
            panic!("a failed launch must not report a launch");
        };
        assert!(key.starts_with("launcher.platform.error.unknown_application"));
        assert_eq!(
            model.notice(),
            Some(&Notice::LaunchFailed(key)),
            "the failure is state the screen renders, not a log line"
        );
    }

    #[test]
    fn a_failure_is_cleared_by_the_next_thing_the_user_does() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        let starter = RecordingStarter::failing(PlatformError::UnknownApplication(
            "archive.desktop".to_string(),
        ));
        model.activate(&starter);
        assert!(model.notice().is_some());

        model.set_query("br");
        assert!(model.notice().is_none());

        model.activate(&starter);
        assert!(model.notice().is_some());
        model.select_at(0);
        assert!(model.notice().is_none());
    }

    #[test]
    fn activating_with_nothing_on_screen_starts_nothing() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.set_query("qqqq");
        let starter = RecordingStarter::succeeding();

        assert_eq!(model.activate(&starter), Activation::NothingSelected);
        assert!(starter.started().is_empty());
    }

    #[test]
    fn a_pointer_cannot_select_a_row_that_is_not_there() {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.select_at(99);
        assert_eq!(model.selected_index(), Some(0));
        model.select_at(2);
        assert_eq!(model.selected_index(), Some(2));
    }
}
