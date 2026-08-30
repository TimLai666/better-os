//! The content area: what a row looks like, what a keystroke does to the
//! selection, and what opening an entry means.
//!
//! **Nothing here holds a copy of the directory.** `files_core::DirectoryModel`
//! already owns the entries, the sort order, the hidden projection, and the
//! selection; a second list here would be a second answer to "what is in this
//! folder" and would have to be kept in step with a stream that is still
//! arriving. A row is formatted from the entry the model hands over, at the
//! index the viewport asked for, and thrown away — which is what makes both
//! view modes virtualized in the only sense that matters: a hundred thousand
//! entries cost a hundred thousand entries' worth of memory in `files-core`
//! and one screenful of formatting per frame here.
//!
//! The cursor is the one piece of state the view keeps, and it is kept as an
//! index *and* re-derived from the selection's own cursor identity. Incremental
//! insertion moves indices around; it must not move the selection. Resolving
//! the index from the identity every time entries arrive is how that is held.

use std::time::{Duration, Instant};

use files_core::{
    DirectoryModel, Entry, EntryId, EntryKind, ListingStatus, Location, OpenIntent, OpenRefusal,
    SortDirection, SortKey, SortOrder,
};

use crate::format;
use crate::i18n::Copy;
use crate::prefs::{ItemScale, ViewMode};

/// How long a type-ahead buffer survives without another keystroke.
pub const TYPE_AHEAD_TIMEOUT: Duration = Duration::from_millis(1_000);

/// One entry, formatted for one frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedRow {
    pub id: EntryId,
    pub name: String,
    pub kind: EntryKind,
    pub size: String,
    pub modified: String,
    pub type_label: String,
    pub extension: String,
    pub hidden: bool,
    pub selected: bool,
    /// True for the entry the keyboard is on, which is drawn differently from
    /// a merely selected one.
    pub focused: bool,
    /// A glyph rather than an icon theme lookup. Ticket 35 owns real icons; a
    /// placeholder that is honest about being one beats an empty square.
    pub glyph: &'static str,
}

/// Formats one entry for display.
pub fn rendered_row(entry: &Entry, selected: bool, focused: bool, c: &'static Copy) -> RenderedRow {
    RenderedRow {
        id: entry.id(),
        name: entry.name.clone(),
        kind: entry.kind,
        size: format::entry_size(entry.size),
        modified: format::file_time(entry.modified),
        type_label: type_label(entry, c),
        extension: entry.extension().to_string(),
        hidden: entry.hidden.is_hidden(),
        selected,
        focused,
        glyph: glyph_for(entry.kind),
    }
}

/// The type cell: the detected MIME type when the platform found one, and the
/// localized kind otherwise.
pub fn type_label(entry: &Entry, c: &'static Copy) -> String {
    if entry.kind == EntryKind::Directory {
        return c.kind_folder.to_string();
    }
    if let Some(mime) = entry.mime.as_ref() {
        return mime.as_str().to_string();
    }
    match entry.kind {
        EntryKind::Directory => c.kind_folder,
        EntryKind::File => c.kind_file,
        EntryKind::Application => c.kind_application,
        EntryKind::Special => c.kind_special,
        EntryKind::Unknown => c.kind_unknown,
    }
    .to_string()
}

fn glyph_for(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Directory => "📁",
        EntryKind::File => "📄",
        EntryKind::Application => "🚀",
        EntryKind::Special => "⚙",
        EntryKind::Unknown => "❓",
    }
}

/// The columns the detailed list draws, in order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ListColumn {
    Name,
    Size,
    Modified,
    Type,
    Extension,
}

impl ListColumn {
    pub const ALL: [ListColumn; 5] = [
        ListColumn::Name,
        ListColumn::Size,
        ListColumn::Modified,
        ListColumn::Type,
        ListColumn::Extension,
    ];

    /// The sort key clicking this header selects. Every column has one, which
    /// is why the sort key set and the column set are the same five things.
    pub fn sort_key(self) -> SortKey {
        match self {
            ListColumn::Name => SortKey::Name,
            ListColumn::Size => SortKey::Size,
            ListColumn::Modified => SortKey::Modified,
            ListColumn::Type => SortKey::Type,
            ListColumn::Extension => SortKey::Extension,
        }
    }

    pub fn header(self, c: &'static Copy) -> &'static str {
        match self {
            ListColumn::Name => c.column_name,
            ListColumn::Size => c.column_size,
            ListColumn::Modified => c.column_modified,
            ListColumn::Type => c.column_type,
            ListColumn::Extension => c.column_extension,
        }
    }

    /// Column width in logical pixels, owned here so the overflow tests and
    /// the renderer cannot disagree about how much room a header has.
    pub fn width(self) -> f32 {
        match self {
            ListColumn::Name => 320.0,
            ListColumn::Size => 110.0,
            ListColumn::Modified => 170.0,
            ListColumn::Type => 190.0,
            ListColumn::Extension => 120.0,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ListColumn::Name => "name",
            ListColumn::Size => "size",
            ListColumn::Modified => "modified",
            ListColumn::Type => "type",
            ListColumn::Extension => "extension",
        }
    }
}

/// The sort keys the toolbar offers, in menu order.
pub const SORT_KEYS: [SortKey; 5] = [
    SortKey::Name,
    SortKey::Modified,
    SortKey::Size,
    SortKey::Type,
    SortKey::Extension,
];

pub fn sort_key_label(key: SortKey, c: &'static Copy) -> &'static str {
    match key {
        SortKey::Name => c.sort_name,
        SortKey::Modified => c.sort_modified,
        SortKey::Size => c.sort_size,
        SortKey::Type => c.sort_type,
        SortKey::Extension => c.sort_extension,
    }
}

/// What clicking a column header does.
///
/// Clicking the sorted column flips the direction; clicking another one sorts
/// by it ascending. Folders-first is untouched either way, because it is a
/// preference rather than part of the click.
pub fn header_click(order: SortOrder, column: ListColumn) -> SortOrder {
    let key = column.sort_key();
    if order.key == key {
        SortOrder::new(key, reversed(order.direction)).with_folders_first(order.folders_first)
    } else {
        SortOrder::new(key, SortDirection::Ascending).with_folders_first(order.folders_first)
    }
}

/// Flips a sort direction. `files_core::SortDirection` keeps its own flip
/// private, and one line here is cheaper than widening that crate's surface
/// for a header click.
pub fn reversed(direction: SortDirection) -> SortDirection {
    match direction {
        SortDirection::Ascending => SortDirection::Descending,
        SortDirection::Descending => SortDirection::Ascending,
    }
}

/// The keyboard and pointer gestures the content area understands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionInput {
    /// A plain click: this entry alone.
    Click(usize),
    /// Control-click: add or remove this entry.
    ToggleClick(usize),
    /// Shift-click: everything between the anchor and here.
    RangeClick(usize),
    Up,
    Down,
    /// One row up in the list, one tile left in the grid.
    Left,
    Right,
    Home,
    End,
    PageUp(usize),
    PageDown(usize),
    SelectAll,
    Clear,
}

impl SelectionInput {
    /// The same gesture, aimed at a different index.
    ///
    /// A running search draws its own rows, so the row number a click carries
    /// has to be translated before the selection sees it. Doing that here
    /// rather than at the three click sites is what keeps the three from
    /// drifting apart.
    pub fn at(self, index: usize) -> Self {
        match self {
            SelectionInput::Click(_) => SelectionInput::Click(index),
            SelectionInput::ToggleClick(_) => SelectionInput::ToggleClick(index),
            SelectionInput::RangeClick(_) => SelectionInput::RangeClick(index),
            other => other,
        }
    }
}

/// Where the content area's keyboard focus is.
#[derive(Clone, Debug, Default)]
pub struct ContentView {
    pub mode: ViewMode,
    pub scale: ItemScale,
    /// The row the keyboard is on, as an index into the visible list.
    cursor: Option<usize>,
    type_ahead: String,
    last_keystroke: Option<Instant>,
}

impl ContentView {
    pub fn new(mode: ViewMode, scale: ItemScale) -> Self {
        Self {
            mode,
            scale,
            cursor: None,
            type_ahead: String::new(),
            last_keystroke: None,
        }
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn type_ahead(&self) -> &str {
        &self.type_ahead
    }

    /// How many tiles fit across the grid at this width. One in the list.
    pub fn columns(&self, viewport_width: f32) -> usize {
        match self.mode {
            ViewMode::List => 1,
            ViewMode::Grid => {
                let tile = self.scale.tile_size() + GRID_GAP;
                ((viewport_width / tile).floor() as usize).max(1)
            }
        }
    }

    /// The height of one row or one tile row, which is what the virtualized
    /// list divides the scroll offset by.
    pub fn row_height(&self) -> f32 {
        match self.mode {
            ViewMode::List => self.scale.row_height(),
            ViewMode::Grid => self.scale.tile_size() + GRID_GAP,
        }
    }

    /// Re-derives the cursor index from the selection's cursor identity.
    ///
    /// This is the whole of "incremental insertion does not move the
    /// selection". Entries arriving above the cursor shift its index; the
    /// identity is unchanged, so the index is recomputed from it and the same
    /// entry stays focused. When the focused entry is gone — deleted, or
    /// filtered out by a hidden toggle — the cursor is dropped rather than
    /// silently landing on whatever took that index.
    pub fn resync(&mut self, model: &DirectoryModel) {
        let Some(cursor) = model.selection().cursor().cloned() else {
            self.cursor = None;
            return;
        };
        // The common case is that nothing moved, so the current index is
        // checked before the list is walked.
        if let Some(index) = self.cursor
            && model.visible(index).map(Entry::id).as_ref() == Some(&cursor)
        {
            return;
        }
        self.cursor = model.iter_visible().position(|entry| entry.id() == cursor);
    }

    /// Applies one gesture. Returns the index the view should scroll to, when
    /// the gesture moved the cursor.
    pub fn apply(
        &mut self,
        model: &mut DirectoryModel,
        input: SelectionInput,
        columns: usize,
    ) -> Option<usize> {
        let length = model.visible_len();
        if length == 0 {
            self.cursor = None;
            model.selection_mut().clear();
            return None;
        }
        let columns = columns.max(1);
        let target = match input {
            SelectionInput::Click(index) | SelectionInput::ToggleClick(index) => {
                index.min(length - 1)
            }
            SelectionInput::RangeClick(index) => index.min(length - 1),
            SelectionInput::Up => step(self.cursor, length, -(columns as isize)),
            SelectionInput::Down => step(self.cursor, length, columns as isize),
            SelectionInput::Left => step(self.cursor, length, -1),
            SelectionInput::Right => step(self.cursor, length, 1),
            SelectionInput::Home => 0,
            SelectionInput::End => length - 1,
            SelectionInput::PageUp(rows) => {
                step(self.cursor, length, -((rows.max(1) * columns) as isize))
            }
            SelectionInput::PageDown(rows) => {
                step(self.cursor, length, (rows.max(1) * columns) as isize)
            }
            SelectionInput::SelectAll => {
                let ordered = model.visible_ids();
                model.selection_mut().select_all(&ordered);
                return self.cursor;
            }
            SelectionInput::Clear => {
                model.selection_mut().clear();
                self.cursor = None;
                return None;
            }
        };

        let id = model.visible(target).map(Entry::id)?;
        match input {
            SelectionInput::ToggleClick(_) => {
                model.selection_mut().toggle(id);
            }
            SelectionInput::RangeClick(_) => {
                let ordered = model.visible_ids();
                model.selection_mut().select_range(&ordered, id);
            }
            _ => {
                model.selection_mut().select_only(id);
            }
        }
        self.cursor = Some(target);
        Some(target)
    }

    /// Handles one type-ahead character. Returns the index it jumped to.
    ///
    /// The buffer accumulates while the user keeps typing and resets after a
    /// pause, which is what makes `re` find `reports` rather than jumping to
    /// the first `e`.
    pub fn type_ahead_key(
        &mut self,
        model: &mut DirectoryModel,
        character: char,
        now: Instant,
    ) -> Option<usize> {
        let expired = self
            .last_keystroke
            .is_none_or(|last| now.duration_since(last) > TYPE_AHEAD_TIMEOUT);
        if expired {
            self.type_ahead.clear();
        }
        self.last_keystroke = Some(now);
        self.type_ahead.extend(character.to_lowercase());

        let needle = self.type_ahead.clone();
        // Searching from the entry after the cursor, wrapping, is what makes
        // pressing the same letter twice step through the matches instead of
        // sticking on the first one.
        let length = model.visible_len();
        if length == 0 {
            return None;
        }
        let start = match (self.cursor, needle.chars().count()) {
            (Some(cursor), 1) => cursor + 1,
            (Some(cursor), _) => cursor,
            (None, _) => 0,
        };
        let mut found = None;
        for offset in 0..length {
            let index = (start + offset) % length;
            let matches = model
                .visible(index)
                .is_some_and(|entry| entry.name.to_lowercase().starts_with(&needle));
            if matches {
                found = Some(index);
                break;
            }
        }
        let index = found?;
        let id = model.visible(index).map(Entry::id)?;
        model.selection_mut().select_only(id);
        self.cursor = Some(index);
        Some(index)
    }

    /// Drops a stale type-ahead buffer without needing a keystroke.
    pub fn clear_type_ahead(&mut self) {
        self.type_ahead.clear();
        self.last_keystroke = None;
    }
}

/// The gap between grid tiles, in logical pixels.
pub const GRID_GAP: f32 = 12.0;

fn step(cursor: Option<usize>, length: usize, delta: isize) -> usize {
    let Some(cursor) = cursor else {
        return if delta < 0 { length - 1 } else { 0 };
    };
    let next = cursor as isize + delta;
    next.clamp(0, length as isize - 1) as usize
}

/// What the window does when an entry is opened.
///
/// Directories navigate. Applications and files are handed back as intents the
/// window reports, because launching an application and resolving a file's
/// handler are ticket 35's work through the shared catalog and Better App
/// Chooser, and inventing a second launcher here is exactly the duplication
/// ENG.md forbids.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenOutcome {
    /// Go here.
    Navigate(Box<Location>),
    /// Nothing is wired up to open this yet, with the message to show.
    NoHandler(NoHandlerReason),
    /// The entry cannot be opened at all, and why.
    Refused(OpenRefusal),
}

/// Why nothing happened, when the entry itself was fine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoHandlerReason {
    /// A file whose association would be resolved by Better App Chooser.
    File { name: String },
    /// An application row, which the shared catalog launches.
    Application { name: String },
}

impl NoHandlerReason {
    pub fn message(&self, c: &'static Copy) -> String {
        match self {
            NoHandlerReason::File { name } => format!("{name} — {}", c.no_handler_wired),
            NoHandlerReason::Application { name } => {
                format!("{name} — {}", c.launching_not_wired)
            }
        }
    }
}

/// Routes one entry's open intent.
pub fn route_open(entry: &Entry) -> OpenOutcome {
    match files_core::open_intent(entry) {
        OpenIntent::Navigate(location) => OpenOutcome::Navigate(location),
        OpenIntent::Launch { .. } => OpenOutcome::NoHandler(NoHandlerReason::Application {
            name: entry.name.clone(),
        }),
        OpenIntent::OpenFile { .. } => OpenOutcome::NoHandler(NoHandlerReason::File {
            name: entry.name.clone(),
        }),
        OpenIntent::Refused(refusal) => OpenOutcome::Refused(refusal),
    }
}

/// The one-line summary under the content area.
pub fn status_line(model: &DirectoryModel, c: &'static Copy) -> String {
    let mut parts = Vec::new();
    match model.status() {
        ListingStatus::Loading => parts.push(c.loading.to_string()),
        ListingStatus::Failed(_) => parts.push(c.listing_failed.to_string()),
        ListingStatus::Cancelled => parts.push(c.listing_cancelled.to_string()),
        ListingStatus::Complete => {}
    }
    parts.push(format!("{} {}", model.visible_len(), c.item_count));
    let hidden = model.total_len().saturating_sub(model.visible_len());
    if hidden > 0 {
        parts.push(format!("{hidden} {}", c.hidden_shown));
    }
    let selected = model.selection().len();
    if selected > 0 {
        parts.push(format!("{selected} {}", c.selected_count));
    }
    if !model.skipped().is_empty() {
        parts.push(format!("{} {}", model.skipped().len(), c.skipped_entries));
    }
    parts.join(" · ")
}

/// The empty-state message for a directory with nothing to draw, or `None`
/// when there is something to draw.
pub fn empty_state(model: &DirectoryModel, c: &'static Copy) -> Option<&'static str> {
    if model.visible_len() > 0 {
        return None;
    }
    Some(match model.status() {
        ListingStatus::Loading => c.loading,
        ListingStatus::Failed(_) => c.listing_failed,
        ListingStatus::Cancelled => c.listing_cancelled,
        ListingStatus::Complete => c.empty_folder,
    })
}

/// Whether this location can be listed at all, so the window shows a reason
/// rather than an empty grid for a network path this build does not implement.
pub fn unlistable_reason(location: &Location, c: &'static Copy) -> Option<&'static str> {
    (!location.is_listable()).then_some(c.not_listable_here)
}
