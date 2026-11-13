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
    DirectoryModel, Entry, EntryId, EntryKind, ListingStatus, Location, SortDirection, SortKey,
    SortOrder,
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
    /// What stands in for the entry when no themed icon is found: a folder
    /// or a page from the window's own icon set, never an emoji.
    pub fallback: FallbackIcon,
}

/// The second line under a grid tile, as folio's tiles have: the size and
/// the day a file was changed, or just the day for a folder. Empty when
/// neither is known.
pub fn tile_caption(row: &RenderedRow) -> String {
    let day = row.modified.split(' ').next().unwrap_or("").trim();
    let day = if day == "—" { "" } else { day };
    let size = if row.kind == EntryKind::File && row.size != "—" {
        row.size.as_str()
    } else {
        ""
    };
    match (size.is_empty(), day.is_empty()) {
        (false, false) => format!("{size} · {day}"),
        (false, true) => size.to_string(),
        (true, false) => day.to_string(),
        (true, true) => String::new(),
    }
}

/// The window's own icon for an entry, used when the icon theme has none.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FallbackIcon {
    Folder,
    File,
    Application,
}

/// Formats one entry for display.
pub fn rendered_row(entry: &Entry, selected: bool, focused: bool, c: &'static Copy) -> RenderedRow {
    rendered_row_named(entry, selected, focused, c, &|_| None)
}

/// [`rendered_row`], with the readable name of a file type looked up by
/// `describe` (the shared MIME database's comment, in the person's
/// language).
pub fn rendered_row_named(
    entry: &Entry,
    selected: bool,
    focused: bool,
    c: &'static Copy,
    describe: &dyn Fn(&str) -> Option<String>,
) -> RenderedRow {
    RenderedRow {
        id: entry.id(),
        name: entry.name.clone(),
        kind: entry.kind,
        size: format::entry_size(entry.size),
        modified: format::file_time(entry.modified),
        type_label: type_label_named(entry, c, describe),
        extension: entry.extension().to_string(),
        hidden: entry.hidden.is_hidden(),
        selected,
        focused,
        fallback: fallback_icon(entry.kind),
    }
}

/// The type cell, in words: "Folder", "Plain text document", "PNG image".
/// A type the database does not name reads as its extension in capitals
/// and the word for a file ("RS file"), never as `text/rust`.
pub fn type_label(entry: &Entry, c: &'static Copy) -> String {
    type_label_named(entry, c, &|_| None)
}

pub fn type_label_named(
    entry: &Entry,
    c: &'static Copy,
    describe: &dyn Fn(&str) -> Option<String>,
) -> String {
    match entry.kind {
        EntryKind::Directory => return c.kind_folder.to_string(),
        EntryKind::Application => return c.kind_application.to_string(),
        EntryKind::Special => return c.kind_special.to_string(),
        EntryKind::File | EntryKind::Unknown => {}
    }
    if let Some(name) = entry.mime.as_ref().and_then(|mime| describe(mime.as_str())) {
        return name;
    }
    let extension = entry.extension();
    if extension.is_empty() {
        c.kind_file.to_string()
    } else {
        format!("{} {}", extension.to_uppercase(), c.type_unknown_file)
    }
}

/// The themed icons to try for an entry, most specific first, as
/// `(icon theme context, name)`.
pub fn icon_candidates(
    entry: &Entry,
    icon_names: &dyn Fn(&str) -> Vec<String>,
) -> Vec<(&'static str, String)> {
    match entry.kind {
        EntryKind::Directory => vec![("places", "folder".to_string())],
        EntryKind::Application => vec![("mimetypes", "application-x-executable".to_string())],
        EntryKind::Special => vec![("mimetypes", "inode-symlink".to_string())],
        EntryKind::File | EntryKind::Unknown => {
            let mut names: Vec<(&'static str, String)> = entry
                .mime
                .as_ref()
                .map(|mime| icon_names(mime.as_str()))
                .unwrap_or_default()
                .into_iter()
                .map(|name| ("mimetypes", name))
                .collect();
            names.push(("mimetypes", "text-x-generic".to_string()));
            names
        }
    }
}

fn fallback_icon(kind: EntryKind) -> FallbackIcon {
    match kind {
        EntryKind::Directory => FallbackIcon::Folder,
        EntryKind::Application => FallbackIcon::Application,
        _ => FallbackIcon::File,
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

/// How the content area lays its items out, for rubber-band selection: the
/// grid as the virtual body draws it (rows of `columns` tiles of `tile`
/// pixels with `gap` between them and around the top, `pad` from the left),
/// or the list's rows of `row` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BandLayout {
    Grid {
        columns: usize,
        tile: f32,
        gap: f32,
        pad: f32,
    },
    List {
        row: f32,
    },
}

/// The items a rubber band from `start` to `end` touches, in the content's
/// own coordinates (scrolling already added), out of `count` items. A band
/// in the gaps between tiles touches nothing; in the list, any row the band
/// crosses is touched.
pub fn band_hits(
    start: (f32, f32),
    end: (f32, f32),
    layout: BandLayout,
    count: usize,
) -> Vec<usize> {
    let (left, right) = (start.0.min(end.0), start.0.max(end.0));
    let (top, bottom) = (start.1.min(end.1), start.1.max(end.1));
    let mut hits = Vec::new();
    match layout {
        BandLayout::List { row } if row > 0.0 => {
            let first = (top.max(0.0) / row).floor() as usize;
            let last = (bottom.max(0.0) / row).floor() as usize;
            hits.extend((first..=last).filter(|index| *index < count));
        }
        BandLayout::Grid {
            columns,
            tile,
            gap,
            pad,
        } if columns > 0 && tile > 0.0 => {
            let rows = count.div_ceil(columns);
            for row in 0..rows {
                let y = gap + row as f32 * (tile + gap);
                if y > bottom || y + tile < top {
                    continue;
                }
                for column in 0..columns {
                    let index = row * columns + column;
                    if index >= count {
                        break;
                    }
                    let x = pad + column as f32 * (tile + gap);
                    if x <= right && x + tile >= left {
                        hits.push(index);
                    }
                }
            }
        }
        _ => {}
    }
    hits
}

/// The narrowest the name column is drawn before other columns step aside.
pub const MIN_NAME_WIDTH: f32 = 220.0;

/// The columns the list draws in `width` pixels: the name always, then the
/// others while the name keeps [`MIN_NAME_WIDTH`]; the extension steps aside
/// first, then the type, the size, and the date, as folio's list drops the
/// date then the size when it is narrow.
pub fn visible_columns(width: f32) -> Vec<ListColumn> {
    let mut shown = vec![
        ListColumn::Name,
        ListColumn::Size,
        ListColumn::Modified,
        ListColumn::Type,
        ListColumn::Extension,
    ];
    let others = |shown: &[ListColumn]| -> f32 {
        shown
            .iter()
            .filter(|column| **column != ListColumn::Name)
            .map(|column| column.width() + 8.0)
            .sum()
    };
    for drop in [
        ListColumn::Extension,
        ListColumn::Type,
        ListColumn::Size,
        ListColumn::Modified,
    ] {
        if width - others(&shown) >= MIN_NAME_WIDTH {
            break;
        }
        shown.retain(|column| *column != drop);
    }
    shown
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
            ViewMode::List | ViewMode::Columns => 1,
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
            ViewMode::List | ViewMode::Columns => self.scale.row_height(),
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

/// The one-line summary under the content area.
pub fn status_line(model: &DirectoryModel, c: &'static Copy) -> String {
    let mut parts = Vec::new();
    match model.status() {
        ListingStatus::Loading => parts.push(c.loading.to_string()),
        ListingStatus::Failed(_) => parts.push(c.listing_failed.to_string()),
        ListingStatus::Cancelled => parts.push(c.listing_cancelled.to_string()),
        ListingStatus::Complete => {}
    }
    parts.push(crate::i18n::counted(
        model.visible_len(),
        c.item_one,
        c.item_count,
    ));
    let hidden = model.total_len().saturating_sub(model.visible_len());
    if hidden > 0 {
        parts.push(crate::i18n::counted(hidden, c.hidden_one, c.hidden_shown));
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
