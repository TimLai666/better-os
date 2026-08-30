//! The selection model.
//!
//! Selection is stored as entry identities, never as indices. Issue #6 asks
//! for "incremental item insertion without selection jumping", and an
//! index-based selection cannot deliver that: an entry inserted above the
//! selected row by an arriving batch would silently move the selection to its
//! neighbour. Identities do not move, so a selection made on the first batch
//! still names the same file after the hundredth.
//!
//! The cursor and the anchor are identities for the same reason. Range
//! extension resolves them against the list's current order at the moment the
//! user extends the range, which is the only point where position matters.

use std::collections::BTreeSet;

use crate::entry::EntryId;

/// What the user has selected, plus where the keyboard is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    selected: BTreeSet<EntryId>,
    /// The entry the keyboard is on. Not necessarily selected: arrowing with
    /// Ctrl held moves the cursor without changing the selection.
    cursor: Option<EntryId>,
    /// Where a shift-extended range started.
    anchor: Option<EntryId>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn contains(&self, id: &EntryId) -> bool {
        self.selected.contains(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &EntryId> {
        self.selected.iter()
    }

    pub fn cursor(&self) -> Option<&EntryId> {
        self.cursor.as_ref()
    }

    pub fn anchor(&self) -> Option<&EntryId> {
        self.anchor.as_ref()
    }

    /// Replaces the selection with one entry, which is a plain click.
    pub fn select_only(&mut self, id: EntryId) {
        self.selected.clear();
        self.selected.insert(id.clone());
        self.cursor = Some(id.clone());
        self.anchor = Some(id);
    }

    /// Adds or removes one entry, which is Ctrl-click.
    pub fn toggle(&mut self, id: EntryId) {
        if self.selected.remove(&id) {
            // The anchor stays where it was: the next shift-click extends from
            // the last deliberate starting point, not from a deselection.
            self.cursor = Some(id);
        } else {
            self.selected.insert(id.clone());
            self.cursor = Some(id.clone());
            self.anchor = Some(id);
        }
    }

    /// Moves the keyboard cursor without touching the selection.
    pub fn move_cursor(&mut self, id: Option<EntryId>) {
        self.cursor = id;
    }

    /// Selects an inclusive run of the currently ordered ids, which is
    /// shift-click and shift-arrow.
    ///
    /// The range is resolved against the order the caller passes in, so a
    /// re-sorted list extends along what the user can actually see.
    pub fn select_range(&mut self, ordered: &[EntryId], to: EntryId) {
        let anchor = self.anchor.clone().unwrap_or_else(|| to.clone());
        let start = ordered.iter().position(|id| *id == anchor);
        let end = ordered.iter().position(|id| *id == to);
        self.selected.clear();
        match (start, end) {
            (Some(start), Some(end)) => {
                let (low, high) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                for id in &ordered[low..=high] {
                    self.selected.insert(id.clone());
                }
            }
            // The anchor is no longer in the list — it was deleted or filtered
            // out. Falling back to the single target is the honest answer;
            // guessing a nearby index would select files the user never
            // pointed at.
            _ => {
                self.selected.insert(to.clone());
                self.anchor = Some(to.clone());
            }
        }
        self.cursor = Some(to);
    }

    pub fn select_all(&mut self, ordered: &[EntryId]) {
        self.selected = ordered.iter().cloned().collect();
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.cursor = None;
        self.anchor = None;
    }

    /// Drops selected ids that no longer exist, after a refresh removed them.
    ///
    /// Returns the ids that were dropped so a caller can report "3 selected
    /// items were deleted elsewhere" instead of quietly shrinking the count.
    pub fn retain_existing(&mut self, exists: impl Fn(&EntryId) -> bool) -> Vec<EntryId> {
        let removed: Vec<EntryId> = self
            .selected
            .iter()
            .filter(|id| !exists(id))
            .cloned()
            .collect();
        for id in &removed {
            self.selected.remove(id);
        }
        if let Some(cursor) = &self.cursor
            && !exists(cursor)
        {
            self.cursor = None;
        }
        if let Some(anchor) = &self.anchor
            && !exists(anchor)
        {
            self.anchor = None;
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> EntryId {
        EntryId::Name(name.to_string())
    }

    #[test]
    fn a_selection_names_entries_so_insertions_cannot_move_it() {
        let mut selection = Selection::new();
        selection.select_only(id("photo.jpg"));
        // A batch arrives and every position shifts.
        let after_insert = [id("aaa.txt"), id("bbb.txt"), id("photo.jpg")];
        assert!(selection.contains(&id("photo.jpg")));
        assert_eq!(selection.len(), 1);
        assert!(after_insert.contains(selection.cursor().unwrap()));
    }

    #[test]
    fn ctrl_click_adds_and_removes_without_losing_the_anchor() {
        let mut selection = Selection::new();
        selection.select_only(id("a"));
        selection.toggle(id("c"));
        assert_eq!(selection.len(), 2);
        selection.toggle(id("c"));
        assert_eq!(selection.len(), 1);
        assert_eq!(selection.anchor(), Some(&id("c")));
    }

    #[test]
    fn a_range_follows_the_order_the_view_is_showing() {
        let ordered = vec![id("a"), id("b"), id("c"), id("d")];
        let mut selection = Selection::new();
        selection.select_only(id("b"));
        selection.select_range(&ordered, id("d"));
        assert_eq!(selection.len(), 3);
        assert!(selection.contains(&id("c")));
        assert!(!selection.contains(&id("a")));

        // Extending backwards from the same anchor.
        selection.select_range(&ordered, id("a"));
        assert_eq!(selection.len(), 2);
        assert!(selection.contains(&id("a")));
        assert!(selection.contains(&id("b")));
    }

    #[test]
    fn a_range_whose_anchor_vanished_selects_only_the_target() {
        let ordered = vec![id("a"), id("b")];
        let mut selection = Selection::new();
        selection.select_only(id("deleted"));
        selection.select_range(&ordered, id("b"));
        assert_eq!(selection.len(), 1);
        assert!(selection.contains(&id("b")));
        assert_eq!(selection.anchor(), Some(&id("b")));
    }

    #[test]
    fn entries_removed_elsewhere_are_reported_rather_than_silently_dropped() {
        let mut selection = Selection::new();
        selection.select_only(id("gone"));
        selection.toggle(id("kept"));
        let removed = selection.retain_existing(|entry| *entry == id("kept"));
        assert_eq!(removed, vec![id("gone")]);
        assert_eq!(selection.len(), 1);
        assert_eq!(selection.cursor(), Some(&id("kept")));
    }
}
