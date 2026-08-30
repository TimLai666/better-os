//! The list a view draws, assembled from streaming batches.
//!
//! Entries are stored once, in arrival order, and ordering is a separate
//! vector of indices into that storage. A batch is sorted and merged into the
//! index vector, which moves four bytes per entry instead of a whole `Entry`,
//! so a hundred thousand entries arriving in four hundred batches costs index
//! merges rather than four hundred re-sorts of the entries themselves.
//!
//! Hidden filtering is a second projection over the same indices. Toggling it
//! rebuilds that projection from data already in memory, which is what makes
//! `Ctrl+H` immediate and non-blocking as Issue #6 requires.

use std::collections::HashMap;

use crate::entry::{Entry, EntryId};
use crate::error::ListingError;
use crate::hidden::HiddenPreference;
use crate::listing::{ListingEvent, ListingId, SkippedEntry};
use crate::location::Location;
use crate::selection::Selection;
use crate::sort::SortOrder;

/// Where a listing stands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListingStatus {
    /// Batches are still arriving.
    Loading,
    /// The whole directory is present.
    Complete,
    /// The location could not be read. The entries delivered before the
    /// failure are still in the model.
    Failed(ListingError),
    /// The listing was abandoned. Distinct from a failure: the model holds a
    /// partial list nobody asked to finish.
    Cancelled,
}

/// A change to one entry, coming from the file watcher rather than the initial
/// listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefreshEvent {
    Added(Box<Entry>),
    /// Replaces the entry with the same identity. Ignored when the entry is
    /// not present, because a modification to something outside this listing
    /// is not this listing's business.
    Modified(Box<Entry>),
    Removed(EntryId),
    /// The watcher lost track and the location has to be listed again. Emitted
    /// rather than guessed at, so a dropped-event overflow is visible instead
    /// of leaving a stale list on screen.
    Resynchronize,
}

/// The directory list one tab is showing.
#[derive(Clone, Debug)]
pub struct DirectoryModel {
    location: Location,
    listing: Option<ListingId>,
    status: ListingStatus,
    /// Entry storage. A removed entry leaves a hole that the next insertion
    /// reuses, so removals do not invalidate the indices of the entries around
    /// them.
    storage: Vec<Option<Entry>>,
    free: Vec<u32>,
    positions: HashMap<EntryId, u32>,
    /// Indices into `storage`, in sort order.
    ordered: Vec<u32>,
    /// Slots staged by [`DirectoryModel::apply`] and not yet merged.
    ///
    /// Merging is what costs: it touches the whole ordered list. Doing it once
    /// per batch makes a hundred thousand entries four hundred merges of an
    /// ever-growing list, which is quadratic. Staging lets a consumer drain
    /// every batch that arrived since the last frame and pay for one merge
    /// instead.
    pending: Vec<u32>,
    /// Reused by the merge so committing does not allocate and free a vector
    /// the size of the whole listing on every frame.
    scratch: Vec<u32>,
    /// The subset of `ordered` the hidden preference admits.
    visible: Vec<u32>,
    order: SortOrder,
    hidden: HiddenPreference,
    selection: Selection,
    skipped: Vec<SkippedEntry>,
}

impl DirectoryModel {
    pub fn new(location: Location, order: SortOrder, hidden: HiddenPreference) -> Self {
        Self {
            location,
            listing: None,
            status: ListingStatus::Loading,
            storage: Vec::new(),
            free: Vec::new(),
            positions: HashMap::new(),
            ordered: Vec::new(),
            pending: Vec::new(),
            scratch: Vec::new(),
            visible: Vec::new(),
            order,
            hidden,
            selection: Selection::new(),
            skipped: Vec::new(),
        }
    }

    pub fn location(&self) -> &Location {
        &self.location
    }

    pub fn status(&self) -> &ListingStatus {
        &self.status
    }

    pub fn order(&self) -> SortOrder {
        self.order
    }

    pub fn hidden_preference(&self) -> HiddenPreference {
        self.hidden
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut Selection {
        &mut self.selection
    }

    /// Entries that could not be read, reported by the listing.
    pub fn skipped(&self) -> &[SkippedEntry] {
        &self.skipped
    }

    /// How many entries exist, hidden ones included.
    pub fn total_len(&self) -> usize {
        self.ordered.len()
    }

    /// How many rows a view draws.
    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    pub fn visible(&self, index: usize) -> Option<&Entry> {
        self.visible
            .get(index)
            .and_then(|slot| self.storage[*slot as usize].as_ref())
    }

    pub fn iter_visible(&self) -> impl Iterator<Item = &Entry> {
        self.visible
            .iter()
            .filter_map(|slot| self.storage[*slot as usize].as_ref())
    }

    /// The visible identities in order, which is what a range selection is
    /// resolved against.
    pub fn visible_ids(&self) -> Vec<EntryId> {
        self.iter_visible().map(Entry::id).collect()
    }

    pub fn get(&self, id: &EntryId) -> Option<&Entry> {
        self.positions
            .get(id)
            .and_then(|slot| self.storage[*slot as usize].as_ref())
    }

    /// Applies one streaming event. Returns whether the model changed, so a
    /// consumer can skip a redraw for a batch it already had.
    ///
    /// A batch is *staged*, not merged. Call [`DirectoryModel::commit`] after
    /// applying every event that is currently available — which is what
    /// [`crate::Pane::pump`] does — before reading the list. A terminal event
    /// commits on its own, so a completed listing is never left staged.
    pub fn apply(&mut self, event: ListingEvent) -> bool {
        // A batch from a listing this model is not showing is dropped. The
        // listing id makes that decidable even when a stale batch arrives
        // after the new listing has already delivered some of its own.
        if let Some(current) = self.listing
            && event.listing() != current
        {
            return false;
        }
        if self.listing.is_none() {
            self.listing = Some(event.listing());
        }
        match event {
            ListingEvent::Batch(batch) => {
                if batch.entries.is_empty() {
                    return false;
                }
                self.stage_batch(batch.entries);
                true
            }
            ListingEvent::Complete(summary) => {
                self.commit();
                self.skipped = summary.skipped;
                self.status = ListingStatus::Complete;
                true
            }
            ListingEvent::Failed { error, .. } => {
                self.commit();
                self.status = ListingStatus::Failed(error);
                true
            }
            ListingEvent::Cancelled { .. } => {
                self.commit();
                self.status = ListingStatus::Cancelled;
                true
            }
        }
    }

    /// Whether entries have been staged but not merged.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Merges everything staged into the sorted list. Cheap and a no-op when
    /// nothing is staged, so a consumer can call it every frame.
    pub fn commit(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let mut incoming = std::mem::take(&mut self.pending);
        self.sort_slots(&mut incoming);
        self.merge(incoming);
        self.rebuild_visible();
        true
    }

    /// Stages and immediately merges a batch.
    ///
    /// An entry whose identity is already present replaces it rather than
    /// appearing twice: a watcher event and a slow listing can both deliver
    /// the same file.
    pub fn insert_batch(&mut self, entries: Vec<Entry>) {
        self.stage_batch(entries);
        self.commit();
    }

    fn stage_batch(&mut self, entries: Vec<Entry>) {
        self.pending.reserve(entries.len());
        for entry in entries {
            let id = entry.id();
            if let Some(existing) = self.positions.get(&id).copied() {
                self.storage[existing as usize] = Some(entry);
                // The entry is already known. Its sort position may have
                // changed, so it leaves the committed order and is re-merged
                // with the rest of the staged entries. A slot already staged
                // is not staged twice.
                if !self.pending.contains(&existing) {
                    self.ordered.retain(|slot| *slot != existing);
                    self.pending.push(existing);
                }
                continue;
            }
            let slot = match self.free.pop() {
                Some(slot) => {
                    self.storage[slot as usize] = Some(entry);
                    slot
                }
                None => {
                    self.storage.push(Some(entry));
                    (self.storage.len() - 1) as u32
                }
            };
            self.positions.insert(id, slot);
            self.pending.push(slot);
        }
    }

    /// Applies a watcher event. Returns whether anything changed.
    pub fn apply_refresh(&mut self, event: RefreshEvent) -> bool {
        match event {
            RefreshEvent::Added(entry) | RefreshEvent::Modified(entry) => {
                let id = entry.id();
                let known = self.positions.contains_key(&id);
                if matches!(&self.status, ListingStatus::Loading) && !known {
                    // While the listing is still streaming, an addition the
                    // reader has not reached yet would arrive twice. Inserting
                    // is still correct because the batch path replaces by
                    // identity rather than appending.
                }
                self.insert_batch(vec![*entry]);
                true
            }
            RefreshEvent::Removed(id) => self.remove(&id),
            RefreshEvent::Resynchronize => false,
        }
    }

    /// Removes one entry. Returns whether it was there.
    pub fn remove(&mut self, id: &EntryId) -> bool {
        // Anything staged has to be placed first, or removing an entry that
        // arrived in the same drain would leave its slot in `pending` and
        // resurrect it on the next commit.
        self.commit();
        let Some(slot) = self.positions.remove(id) else {
            return false;
        };
        self.storage[slot as usize] = None;
        self.free.push(slot);
        self.ordered.retain(|candidate| *candidate != slot);
        self.rebuild_visible();
        let positions = &self.positions;
        self.selection
            .retain_existing(|candidate| positions.contains_key(candidate));
        true
    }

    /// Changes the sort order and re-sorts what is already loaded.
    ///
    /// Selection is untouched: it names entries, and re-sorting does not
    /// change which entries exist.
    pub fn set_order(&mut self, order: SortOrder) {
        if self.order == order {
            return;
        }
        self.commit();
        self.order = order;
        let mut slots = std::mem::take(&mut self.ordered);
        self.sort_slots(&mut slots);
        self.ordered = slots;
        self.rebuild_visible();
    }

    /// Shows or hides hidden entries.
    ///
    /// This never reloads. Every entry is already in the model with its hidden
    /// state attached, so the change is a re-filter of indices.
    pub fn set_hidden_preference(&mut self, hidden: HiddenPreference) {
        if self.hidden == hidden {
            return;
        }
        self.commit();
        self.hidden = hidden;
        self.rebuild_visible();
    }

    /// Points the model at a new listing, keeping the location.
    ///
    /// Everything from the previous run is dropped, including the selection:
    /// a reloaded directory's entries are new objects, and keeping a selection
    /// across a reload of a different location is how a delete acts on the
    /// wrong files.
    pub fn restart(&mut self, listing: ListingId) {
        self.listing = Some(listing);
        self.status = ListingStatus::Loading;
        self.storage.clear();
        self.free.clear();
        self.positions.clear();
        self.ordered.clear();
        self.pending.clear();
        self.visible.clear();
        self.skipped.clear();
        self.selection.clear();
    }

    fn entry_at(&self, slot: u32) -> &Entry {
        self.storage[slot as usize]
            .as_ref()
            .expect("an ordered slot always holds an entry")
    }

    fn sort_slots(&self, slots: &mut [u32]) {
        slots.sort_by(|left, right| {
            self.order
                .compare(self.entry_at(*left), self.entry_at(*right))
        });
    }

    /// Merges a sorted run into the sorted order in one pass.
    fn merge(&mut self, incoming: Vec<u32>) {
        if incoming.is_empty() {
            return;
        }
        if self.ordered.is_empty() {
            self.ordered = incoming;
            return;
        }
        // Both vectors are taken out so the merge can read entries through
        // `&self` while writing into the scratch buffer, and the buffer is
        // swapped back in rather than allocated, so a commit on a hundred
        // thousand entries does not allocate and free four hundred kilobytes
        // every frame.
        let existing = std::mem::take(&mut self.ordered);
        let mut merged = std::mem::take(&mut self.scratch);
        merged.clear();
        merged.reserve(existing.len() + incoming.len());

        let mut left = 0usize;
        let mut right = 0usize;
        while left < existing.len() && right < incoming.len() {
            let a = existing[left];
            let b = incoming[right];
            if self
                .order
                .compare(self.entry_at(a), self.entry_at(b))
                .is_le()
            {
                merged.push(a);
                left += 1;
            } else {
                merged.push(b);
                right += 1;
            }
        }
        merged.extend_from_slice(&existing[left..]);
        merged.extend_from_slice(&incoming[right..]);

        self.ordered = merged;
        // The old order vector becomes next commit's scratch.
        self.scratch = existing;
        self.pending = incoming;
        self.pending.clear();
    }

    fn rebuild_visible(&mut self) {
        let hidden = self.hidden;
        self.visible.clear();
        self.visible.reserve(self.ordered.len());
        for slot in &self.ordered {
            if let Some(entry) = &self.storage[*slot as usize]
                && hidden.accepts(entry.hidden)
            {
                self.visible.push(*slot);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{EntryKind, HiddenReason, HiddenState};
    use crate::listing::{ListingBatch, ListingRequest, ListingSession};
    use crate::location::LocalPath;

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry::file(name, LocalPath::new(format!("/d/{name}")).unwrap(), kind)
    }

    fn hidden_entry(name: &str) -> Entry {
        let mut entry = entry(name, EntryKind::File);
        entry.hidden = HiddenState::Hidden(HiddenReason::Dotfile);
        entry
    }

    fn model() -> DirectoryModel {
        DirectoryModel::new(
            Location::local("/d").unwrap(),
            SortOrder::default(),
            HiddenPreference::default(),
        )
    }

    fn names_of(model: &DirectoryModel) -> Vec<String> {
        model
            .iter_visible()
            .map(|entry| entry.name.clone())
            .collect()
    }

    #[test]
    fn batches_arriving_out_of_order_produce_one_sorted_list() {
        let mut model = model();
        model.insert_batch(vec![entry("delta", EntryKind::File)]);
        model.insert_batch(vec![
            entry("alpha", EntryKind::File),
            entry("zulu", EntryKind::File),
        ]);
        model.insert_batch(vec![entry("bravo", EntryKind::Directory)]);
        assert_eq!(names_of(&model), ["bravo", "alpha", "delta", "zulu"]);
    }

    #[test]
    fn the_order_does_not_depend_on_how_the_batches_were_split() {
        let all: Vec<Entry> = ["m", "b", "z", "a", "q", "c", "n"]
            .iter()
            .map(|name| entry(name, EntryKind::File))
            .collect();
        let mut single = model();
        single.insert_batch(all.clone());
        for split in [1usize, 2, 3, 5] {
            let mut chunked = model();
            for chunk in all.chunks(split) {
                chunked.insert_batch(chunk.to_vec());
            }
            assert_eq!(names_of(&chunked), names_of(&single), "split of {split}");
        }
    }

    #[test]
    fn showing_hidden_entries_does_not_touch_the_stored_listing() {
        let mut model = model();
        model.insert_batch(vec![
            entry("visible.txt", EntryKind::File),
            hidden_entry(".x"),
        ]);
        assert_eq!(model.total_len(), 2);
        assert_eq!(model.visible_len(), 1);
        model.set_hidden_preference(HiddenPreference::showing_hidden());
        assert_eq!(model.visible_len(), 2);
        assert_eq!(model.total_len(), 2);
        assert_eq!(names_of(&model), [".x", "visible.txt"]);
    }

    #[test]
    fn a_selection_survives_later_batches_inserting_above_it() {
        let mut model = model();
        model.insert_batch(vec![entry("photo.jpg", EntryKind::File)]);
        let id = model.visible(0).unwrap().id();
        model.selection_mut().select_only(id.clone());
        for round in 0..10 {
            model.insert_batch(vec![entry(&format!("aaa{round}.txt"), EntryKind::File)]);
        }
        assert_eq!(model.selection().len(), 1);
        assert!(model.selection().contains(&id));
        // The selected entry moved down the list, and the selection followed
        // the file rather than the row.
        let index = model.visible_ids().iter().position(|other| *other == id);
        assert_eq!(index, Some(10));
    }

    #[test]
    fn re_sorting_keeps_the_selection() {
        let mut model = model();
        model.insert_batch(vec![
            entry("a", EntryKind::File),
            entry("b", EntryKind::File),
        ]);
        let id = model.visible(0).unwrap().id();
        model.selection_mut().select_only(id.clone());
        model.set_order(SortOrder::new(
            crate::sort::SortKey::Name,
            crate::sort::SortDirection::Descending,
        ));
        assert_eq!(names_of(&model), ["b", "a"]);
        assert!(model.selection().contains(&id));
    }

    #[test]
    fn a_watcher_removal_drops_the_entry_and_deselects_it() {
        let mut model = model();
        model.insert_batch(vec![
            entry("keep", EntryKind::File),
            entry("gone", EntryKind::File),
        ]);
        let gone = EntryId::Name("gone".to_string());
        model.selection_mut().select_only(gone.clone());
        assert!(model.apply_refresh(RefreshEvent::Removed(gone.clone())));
        assert_eq!(names_of(&model), ["keep"]);
        assert!(!model.selection().contains(&gone));
        assert!(!model.apply_refresh(RefreshEvent::Removed(gone)));
    }

    #[test]
    fn a_modification_replaces_in_place_and_re_sorts() {
        let mut model = model();
        model.insert_batch(vec![
            entry("a", EntryKind::File),
            entry("b", EntryKind::File),
        ]);
        model.set_order(SortOrder::new(
            crate::sort::SortKey::Size,
            crate::sort::SortDirection::Ascending,
        ));
        let mut updated = entry("b", EntryKind::File);
        updated.size = crate::entry::EntrySize::Bytes(1);
        model.apply_refresh(RefreshEvent::Modified(Box::new(updated)));
        assert_eq!(model.total_len(), 2);
        assert_eq!(names_of(&model), ["b", "a"]);
    }

    #[test]
    fn a_removed_slot_is_reused_rather_than_growing_storage() {
        let mut model = model();
        model.insert_batch(vec![entry("one", EntryKind::File)]);
        model.remove(&EntryId::Name("one".to_string()));
        model.insert_batch(vec![entry("two", EntryKind::File)]);
        assert_eq!(model.storage.len(), 1);
        assert_eq!(names_of(&model), ["two"]);
    }

    #[test]
    fn staged_batches_merge_once_and_end_in_the_same_order_as_merging_each() {
        let names = ["m", "b", "z", "a", "q", "c", "n", "d"];
        let batches: Vec<Vec<Entry>> = names
            .chunks(2)
            .map(|chunk| {
                chunk
                    .iter()
                    .map(|name| entry(name, EntryKind::File))
                    .collect()
            })
            .collect();

        let listing = crate::listing::ListingId::next();
        let mut staged = model();
        staged.restart(listing);
        for (sequence, batch) in batches.iter().enumerate() {
            staged.apply(ListingEvent::Batch(ListingBatch {
                listing,
                sequence: sequence as u64,
                entries: batch.clone(),
            }));
        }
        assert!(staged.has_pending());
        assert_eq!(staged.visible_len(), 0, "nothing is placed until commit");
        assert!(staged.commit());
        assert!(!staged.has_pending());
        assert!(!staged.commit(), "committing twice is a no-op");

        let mut eager = model();
        for batch in &batches {
            eager.insert_batch(batch.clone());
        }
        assert_eq!(names_of(&staged), names_of(&eager));
        assert_eq!(names_of(&staged), ["a", "b", "c", "d", "m", "n", "q", "z"]);
    }

    #[test]
    fn a_terminal_event_commits_what_was_staged() {
        let listing = crate::listing::ListingId::next();
        let mut model = model();
        model.restart(listing);
        model.apply(ListingEvent::Batch(ListingBatch {
            listing,
            sequence: 0,
            entries: vec![entry("only", EntryKind::File)],
        }));
        assert!(model.has_pending());
        model.apply(ListingEvent::Complete(crate::listing::ListingSummary {
            listing,
            total: 1,
            skipped: Vec::new(),
        }));
        assert!(!model.has_pending());
        assert_eq!(names_of(&model), ["only"]);
    }

    #[test]
    fn removing_an_entry_that_is_still_staged_does_not_resurrect_it() {
        let listing = crate::listing::ListingId::next();
        let mut model = model();
        model.restart(listing);
        model.apply(ListingEvent::Batch(ListingBatch {
            listing,
            sequence: 0,
            entries: vec![
                entry("keep", EntryKind::File),
                entry("gone", EntryKind::File),
            ],
        }));
        assert!(model.has_pending());
        assert!(model.remove(&EntryId::Name("gone".to_string())));
        model.commit();
        assert_eq!(names_of(&model), ["keep"]);
    }

    #[test]
    fn a_batch_from_an_abandoned_listing_is_dropped() {
        let first = ListingRequest::new(Location::local("/d").unwrap());
        let second = ListingRequest::new(Location::local("/d").unwrap());
        let (_session, _sink) = ListingSession::start(&first);
        let mut model = model();
        model.restart(second.listing);
        let stale = ListingEvent::Batch(ListingBatch {
            listing: first.listing,
            sequence: 0,
            entries: vec![entry("stale", EntryKind::File)],
        });
        assert!(!model.apply(stale));
        assert_eq!(model.total_len(), 0);
    }
}
