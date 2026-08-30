//! What the launcher shows, as plain data with no GPUI in sight.
//!
//! Issue #2 describes one overlay with two states driven by the current query,
//! not two products. That is why there is a single [`LauncherState`] holding a
//! query and a single [`LauncherState::view`] returning either the browse
//! model or the search model. There is no mode to set, so there is no mode to
//! get out of sync: emptying the query returns the application library because
//! that is what an empty query means, not because something switched modes.
//!
//! Search results borrow from the index rather than cloning it. A launcher
//! rebuilds this on every keystroke over thousands of records, and copying a
//! name, an icon reference, and a category list per result is the one cost
//! that would grow with the catalog for no reason.

use app_catalog_core::{DesktopId, IconReference};

use crate::index::SearchIndex;
use crate::matcher::{FieldKind, MatchKind};
use crate::usage::UsageStore;

/// The freedesktop registered main categories, in the order sections are
/// presented. The grouping and presentation of the application library is an
/// open decision in Issue #2; this is the neutral default, and it is a list
/// rather than a policy so replacing it does not touch matching or ranking.
pub const MAIN_CATEGORIES: [&str; 11] = [
    "AudioVideo",
    "Development",
    "Education",
    "Game",
    "Graphics",
    "Network",
    "Office",
    "Science",
    "Settings",
    "System",
    "Utility",
];

/// The section applications with no registered main category fall into.
pub const OTHER_CATEGORY: &str = "Other";

/// One application as the launcher presents it. Everything needed to draw a
/// tile; nothing needed to launch one, which stays with the catalog record and
/// the shared launch path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LauncherApplication {
    pub desktop_id: DesktopId,
    /// The name in the locale the index was built for.
    pub display_name: String,
    pub generic_name: Option<String>,
    pub icon: Option<IconReference>,
    pub categories: Vec<String>,
}

/// One category section of the application library.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowseSection {
    pub category: &'static str,
    /// Positions in [`BrowseModel::applications`], in the same order.
    pub members: Vec<usize>,
}

/// The application library: every visible application in one deterministic
/// order, plus an optional grouping over that same order.
///
/// Sections index into the flat list instead of repeating it, so a surface
/// that ignores grouping pays nothing for it and a surface that uses it never
/// shows an application in an order the flat list disagrees with.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BrowseModel {
    applications: Vec<LauncherApplication>,
    sections: Vec<BrowseSection>,
}

impl BrowseModel {
    pub(crate) fn new(applications: Vec<LauncherApplication>) -> Self {
        let mut sections = Vec::new();
        for category in MAIN_CATEGORIES {
            let members: Vec<usize> = applications
                .iter()
                .enumerate()
                .filter(|(_, application)| {
                    application
                        .categories
                        .iter()
                        .any(|declared| declared == category)
                })
                .map(|(position, _)| position)
                .collect();
            if !members.is_empty() {
                sections.push(BrowseSection { category, members });
            }
        }
        let uncategorized: Vec<usize> = applications
            .iter()
            .enumerate()
            .filter(|(_, application)| {
                !application
                    .categories
                    .iter()
                    .any(|declared| MAIN_CATEGORIES.contains(&declared.as_str()))
            })
            .map(|(position, _)| position)
            .collect();
        if !uncategorized.is_empty() {
            sections.push(BrowseSection {
                category: OTHER_CATEGORY,
                members: uncategorized,
            });
        }
        Self {
            applications,
            sections,
        }
    }

    /// Every visible application, ordered by folded display name and then by
    /// desktop ID.
    pub fn applications(&self) -> &[LauncherApplication] {
        &self.applications
    }

    /// The category sections over [`BrowseModel::applications`]. An
    /// application declaring several main categories appears in each of them.
    pub fn sections(&self) -> &[BrowseSection] {
        &self.sections
    }

    pub fn len(&self) -> usize {
        self.applications.len()
    }

    pub fn is_empty(&self) -> bool {
        self.applications.is_empty()
    }
}

/// One search hit, with the reason it ranked where it did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchResult<'index> {
    pub application: &'index LauncherApplication,
    /// How the query met the field.
    pub match_kind: MatchKind,
    /// Which field it met.
    pub matched_field: FieldKind,
    /// The score before any usage adjustment. Comparable only within one
    /// query.
    pub base_score: u32,
    /// The score results were sorted by. Equal to `base_score` unless usage
    /// weighting was switched on.
    pub score: u32,
    /// How many launches the usage store reported, whether or not weighting
    /// was on.
    pub launch_count: u32,
}

/// The ranked answer to one query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchModel<'index> {
    query: String,
    results: Vec<SearchResult<'index>>,
}

impl<'index> SearchModel<'index> {
    pub(crate) fn new(query: String, results: Vec<SearchResult<'index>>) -> Self {
        Self { query, results }
    }

    /// The query these results answer, as it was typed.
    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &[SearchResult<'index>] {
        &self.results
    }

    /// Whether the query matched nothing, which the surface renders as its
    /// empty-result state rather than as an empty library.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// The desktop IDs in ranked order, which is what a determinism check
    /// compares.
    pub fn desktop_ids(&self) -> Vec<&'index DesktopId> {
        self.results
            .iter()
            .map(|result| &result.application.desktop_id)
            .collect()
    }
}

/// What the launcher should be showing right now. There is no third variant,
/// and no variant means "switching".
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LauncherView<'index> {
    /// The application library, shown whenever the query is blank.
    Browse(&'index BrowseModel),
    /// Ranked results for a non-blank query.
    Search(SearchModel<'index>),
}

impl<'index> LauncherView<'index> {
    pub fn is_browse(&self) -> bool {
        matches!(self, Self::Browse(_))
    }

    /// The applications to draw, in order, whichever state this is.
    pub fn applications(&self) -> Vec<&'index LauncherApplication> {
        match self {
            Self::Browse(model) => model.applications().iter().collect(),
            Self::Search(model) => model
                .results()
                .iter()
                .map(|result| result.application)
                .collect(),
        }
    }
}

/// Everything about ranking that is a choice rather than a rule.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RankingOptions {
    /// Whether launch history adjusts the order. Off by default: Issue #2
    /// defers that decision to an ADR, and the base matcher is required to
    /// stand on its own.
    ///
    /// When on, the adjustment is bounded so it can only reorder results that
    /// matched the same field in the same way. A frequently launched
    /// application never overtakes one whose name the query actually matched.
    pub usage_weighting: bool,
    /// How many results to keep. `None` keeps every match.
    pub limit: Option<usize>,
}

/// The launcher's whole state: one query.
///
/// Keeping this a type rather than a loose `String` in the GUI is what makes
/// "clearing the query returns to the library" a property of the model instead
/// of a behavior the overlay has to remember to implement.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LauncherState {
    query: String,
}

impl LauncherState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
    }

    pub fn clear(&mut self) {
        self.query.clear();
    }

    /// Whether the query would produce the application library. A query of
    /// nothing but whitespace counts as blank: it is what is left while
    /// someone is deleting what they typed.
    pub fn is_browsing(&self) -> bool {
        self.query.trim().is_empty()
    }

    /// The current view. This is the only function the overlay calls to redraw
    /// after a keystroke.
    pub fn view<'index>(
        &self,
        index: &'index SearchIndex,
        options: &RankingOptions,
        usage: &dyn UsageStore,
    ) -> LauncherView<'index> {
        if self.is_browsing() {
            LauncherView::Browse(index.browse())
        } else {
            LauncherView::Search(index.search(&self.query, options, usage))
        }
    }
}
