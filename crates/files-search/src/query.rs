//! What was asked for, and what the answer must satisfy.
//!
//! The filters are separated from the text on purpose. A filter is a yes or no
//! about an entry and costs nothing to evaluate; the text is what produces a
//! score and an ordering. Keeping them apart is what lets a future indexed
//! provider push the filters into the index and still use the same ranker.

use files_core::{Entry, EntryKind, FileTime, Location};

/// Where a search looks. Visible in the UI, and changeable, which Issue #6
/// requires — a search whose scope is implicit is a search whose empty result
/// means nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchScope {
    /// The entries of one location, and no deeper. The only scope this build
    /// implements.
    CurrentLocation(Location),
    /// The location and everything under it. Representable so the UI can show
    /// the choice; no provider claims it yet.
    Recursive(Location),
    /// Whatever an index covers. Representable for the same reason.
    Indexed,
}

impl SearchScope {
    /// A stable key, so the scope label is chosen by the presentation layer
    /// rather than by a string built here.
    pub fn key(&self) -> &'static str {
        match self {
            SearchScope::CurrentLocation(_) => "files.search.scope.current_location",
            SearchScope::Recursive(_) => "files.search.scope.recursive",
            SearchScope::Indexed => "files.search.scope.indexed",
        }
    }

    pub fn location(&self) -> Option<&Location> {
        match self {
            SearchScope::CurrentLocation(location) | SearchScope::Recursive(location) => {
                Some(location)
            }
            SearchScope::Indexed => None,
        }
    }
}

/// The non-text conditions. Every one of these is what Issue #6 lists as
/// "representable"; the current-directory provider evaluates all of them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Filters {
    /// Without the dot, compared case-insensitively.
    pub extension: Option<String>,
    /// Empty means every kind.
    pub kinds: Vec<EntryKind>,
    pub min_bytes: Option<u64>,
    pub max_bytes: Option<u64>,
    pub modified_after: Option<FileTime>,
    pub modified_before: Option<FileTime>,
}

impl Filters {
    pub fn is_empty(&self) -> bool {
        *self == Filters::default()
    }

    /// Whether this entry passes every declared condition.
    ///
    /// An entry whose size or modification time is unknown fails a size or date
    /// filter rather than passing it. "I could not read this" is not evidence
    /// that it is in range.
    pub fn accepts(&self, entry: &Entry) -> bool {
        if let Some(extension) = &self.extension
            && !entry.extension().eq_ignore_ascii_case(extension)
        {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&entry.kind) {
            return false;
        }
        if self.min_bytes.is_some() || self.max_bytes.is_some() {
            let Some(bytes) = entry.size.bytes() else {
                return false;
            };
            if self.min_bytes.is_some_and(|min| bytes < min) {
                return false;
            }
            if self.max_bytes.is_some_and(|max| bytes > max) {
                return false;
            }
        }
        if self.modified_after.is_some() || self.modified_before.is_some() {
            let Some(modified) = entry.modified else {
                return false;
            };
            if self.modified_after.is_some_and(|after| modified < after) {
                return false;
            }
            if self.modified_before.is_some_and(|before| modified > before) {
                return false;
            }
        }
        true
    }
}

/// One search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchQuery {
    /// What the user typed. An empty query is a valid query: it means "show
    /// everything the filters allow", which is how an extension-only search
    /// works.
    pub text: String,
    pub scope: SearchScope,
    /// Issue #6: hidden files follow an explicit search setting rather than
    /// inheriting the view's. A search for a dotfile should find it even when
    /// the view is not showing dotfiles, and only an explicit setting can make
    /// that a choice rather than a surprise.
    pub include_hidden: bool,
    pub filters: Filters,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>, scope: SearchScope) -> Self {
        Self {
            text: text.into(),
            scope,
            include_hidden: false,
            filters: Filters::default(),
        }
    }

    pub fn including_hidden(mut self, include: bool) -> Self {
        self.include_hidden = include;
        self
    }

    pub fn with_filters(mut self, filters: Filters) -> Self {
        self.filters = filters;
        self
    }

    /// Whether this query would narrow anything at all.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.filters.is_empty()
    }

    /// The text, trimmed and lowercased once, so a run does not redo it per
    /// entry.
    pub fn normalized_text(&self) -> String {
        self.text.trim().to_lowercase()
    }
}
