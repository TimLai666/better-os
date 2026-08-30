//! Sort order, and the total order that makes incremental insertion possible.
//!
//! A streaming listing inserts into a list the user is already looking at, so
//! the comparison has to be a *total* order: if two entries could compare
//! equal, their relative position would depend on which batch they arrived in,
//! and the same directory would come out in a different order on a slower
//! disk. Every key therefore falls through to the name and finally to the
//! entry identity, both of which are unique within one listing.

use std::cmp::Ordering;

use crate::entry::{Entry, EntrySize};

/// What the list is sorted by. These are Issue #6's five view-model keys.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Size,
    /// The entry kind, then the detected type. Folders, then applications,
    /// then files grouped by what they are.
    Type,
    Extension,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

impl SortDirection {
    fn apply(self, ordering: Ordering) -> Ordering {
        match self {
            SortDirection::Ascending => ordering,
            SortDirection::Descending => ordering.reverse(),
        }
    }
}

/// The complete sort configuration for one view.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SortOrder {
    pub key: SortKey,
    pub direction: SortDirection,
    /// Keep directories above files regardless of the key. Configurable, as
    /// Issue #6 requires, and on by default because that is what the platform
    /// file managers do.
    pub folders_first: bool,
}

impl Default for SortOrder {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            direction: SortDirection::Ascending,
            folders_first: true,
        }
    }
}

impl SortOrder {
    pub fn new(key: SortKey, direction: SortDirection) -> Self {
        Self {
            key,
            direction,
            folders_first: true,
        }
    }

    pub fn with_folders_first(mut self, folders_first: bool) -> Self {
        self.folders_first = folders_first;
        self
    }

    /// The total order two entries are placed in.
    pub fn compare(&self, left: &Entry, right: &Entry) -> Ordering {
        if self.folders_first {
            // Folders-first is not reversed by the direction. A user asking
            // for descending names is asking about the names, not for the
            // folders to move to the bottom.
            let grouping = right
                .kind
                .is_directory()
                .cmp(&left.kind.is_directory())
                .then_with(|| rank_kind(right).cmp(&rank_kind(left)));
            if grouping != Ordering::Equal {
                return grouping;
            }
        }
        let primary = match self.key {
            SortKey::Name => natural_compare(&left.name, &right.name),
            SortKey::Modified => compare_option(left.modified, right.modified),
            SortKey::Size => compare_size(left.size, right.size),
            SortKey::Type => left
                .kind
                .cmp(&right.kind)
                .then_with(|| compare_mime(left, right)),
            SortKey::Extension => natural_compare(left.extension(), right.extension()),
        };
        let primary = self.direction.apply(primary);
        if primary != Ordering::Equal {
            return primary;
        }
        // The tie-break is always ascending by name, then by identity. It does
        // not follow the direction, so flipping the direction of a size sort
        // does not also scramble the names inside each size group. A name sort
        // has already used the name as its primary key, and only reaches here
        // for two entries whose names compare equal, where the identity is
        // what makes the order total.
        if self.key == SortKey::Name {
            return compare_identity(left, right);
        }
        natural_compare(&left.name, &right.name).then_with(|| compare_identity(left, right))
    }
}

/// Compares two entries by identity without building one.
///
/// [`Entry::id`] clones a `String`, and the tie-break runs on every comparison
/// of a merge — a hundred thousand entries arriving in batches is tens of
/// millions of comparisons, and two allocations each is the difference between
/// a listing that assembles in milliseconds and one that takes seconds.
fn compare_identity(left: &Entry, right: &Entry) -> Ordering {
    use crate::entry::EntryBody;
    fn rank(entry: &Entry) -> u8 {
        match &entry.body {
            EntryBody::File(_) => 0,
            EntryBody::Application(_) => 1,
            EntryBody::Trashed(_) => 2,
        }
    }
    fn key(entry: &Entry) -> &str {
        match &entry.body {
            EntryBody::File(_) => &entry.name,
            EntryBody::Application(facts) => facts.desktop_id.as_str(),
            EntryBody::Trashed(facts) => &facts.item,
        }
    }
    rank(left)
        .cmp(&rank(right))
        .then_with(|| key(left).cmp(key(right)))
}

/// Ranks the non-directory kinds so folders-first has a defined second tier
/// rather than leaving applications and files interleaved by accident.
fn rank_kind(entry: &Entry) -> u8 {
    match entry.kind {
        crate::entry::EntryKind::Directory => 0,
        crate::entry::EntryKind::Application => 1,
        crate::entry::EntryKind::File => 2,
        crate::entry::EntryKind::Special => 3,
        crate::entry::EntryKind::Unknown => 4,
    }
}

/// Unknown values sort last in an ascending order rather than as zero. A
/// directory whose size has not been measured belongs at the end of a size
/// sort, not at the top pretending to be empty.
fn compare_size(left: EntrySize, right: EntrySize) -> Ordering {
    match (left, right) {
        (EntrySize::Bytes(a), EntrySize::Bytes(b)) => a.cmp(&b),
        (EntrySize::Bytes(_), _) => Ordering::Less,
        (_, EntrySize::Bytes(_)) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn compare_option<T: Ord>(left: Option<T>, right: Option<T>) -> Ordering {
    match (left, right) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_mime(left: &Entry, right: &Entry) -> Ordering {
    compare_option(
        left.mime.as_ref().map(|mime| mime.as_str()),
        right.mime.as_ref().map(|mime| mime.as_str()),
    )
}

/// Case-insensitive comparison with embedded numbers compared by value.
///
/// `file2` before `file10` is what a user expects, and comparing byte by byte
/// gives the opposite. Case is folded first so `Photos` and `photos` sit
/// together, and an exact byte comparison still breaks the tie so the order is
/// total.
/// This runs tens of millions of times while a large directory streams in, so
/// it allocates nothing: digit runs are compared as byte slices in place
/// rather than copied into `String`s.
pub fn natural_compare(left: &str, right: &str) -> Ordering {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let mut i = 0usize;
    let mut j = 0usize;

    while i < left_bytes.len() && j < right_bytes.len() {
        if left_bytes[i].is_ascii_digit() && right_bytes[j].is_ascii_digit() {
            let left_end = digit_run_end(left_bytes, i);
            let right_end = digit_run_end(right_bytes, j);
            // Leading zeros are skipped for the value comparison but still
            // consumed, so `file010` and `file10` compare equal by value and
            // are separated afterwards by the exact byte comparison.
            let left_start = significant_start(left_bytes, i, left_end);
            let right_start = significant_start(right_bytes, j, right_end);
            // Compared as digit slices rather than parsed, so a forty-digit
            // run in a filename cannot overflow an integer.
            let ordering = (left_end - left_start)
                .cmp(&(right_end - right_start))
                .then_with(|| {
                    left_bytes[left_start..left_end].cmp(&right_bytes[right_start..right_end])
                });
            if ordering != Ordering::Equal {
                return ordering;
            }
            i = left_end;
            j = right_end;
            continue;
        }

        // ASCII is the overwhelmingly common case and folds without the
        // multi-character mapping `char::to_lowercase` has to allow for.
        let a = left_bytes[i];
        let b = right_bytes[j];
        if a.is_ascii() && b.is_ascii() {
            let ordering = a
                .to_ascii_lowercase()
                .cmp(&b.to_ascii_lowercase())
                .then_with(|| a.cmp(&b));
            if ordering != Ordering::Equal {
                return ordering;
            }
            i += 1;
            j += 1;
            continue;
        }

        let left_char = left[i..].chars().next().expect("a char boundary");
        let right_char = right[j..].chars().next().expect("a char boundary");
        let ordering = left_char
            .to_lowercase()
            .cmp(right_char.to_lowercase())
            .then_with(|| left_char.cmp(&right_char));
        if ordering != Ordering::Equal {
            return ordering;
        }
        i += left_char.len_utf8();
        j += right_char.len_utf8();
    }

    match (i < left_bytes.len(), j < right_bytes.len()) {
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        // Every compared position matched. The exact bytes break the tie, so
        // names differing only in case or in leading zeros still have a
        // defined order.
        _ => left.cmp(right),
    }
}

fn digit_run_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    index
}

/// The first digit that is not a leading zero, or the last digit of an
/// all-zero run so that `0` and `000` both compare as one zero digit.
fn significant_start(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut index = start;
    while index + 1 < end && bytes[index] == b'0' {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{EntryKind, FileTime};
    use crate::location::LocalPath;

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry::file(name, LocalPath::new(format!("/data/{name}")).unwrap(), kind)
    }

    #[test]
    fn numbers_inside_names_compare_by_value() {
        assert_eq!(natural_compare("file2", "file10"), Ordering::Less);
        assert_eq!(natural_compare("file010", "file10"), Ordering::Less);
        assert_eq!(natural_compare("Photos", "photos"), Ordering::Less);
    }

    #[test]
    fn a_huge_digit_run_does_not_overflow() {
        let long = format!("f{}", "9".repeat(40));
        let longer = format!("f{}", "9".repeat(41));
        assert_eq!(natural_compare(&long, &longer), Ordering::Less);
    }

    #[test]
    fn folders_stay_first_even_when_the_direction_is_reversed() {
        let order = SortOrder::new(SortKey::Name, SortDirection::Descending);
        let folder = entry("aaa", EntryKind::Directory);
        let file = entry("zzz", EntryKind::File);
        assert_eq!(order.compare(&folder, &file), Ordering::Less);
    }

    #[test]
    fn folders_first_can_be_turned_off() {
        let order =
            SortOrder::new(SortKey::Name, SortDirection::Ascending).with_folders_first(false);
        let folder = entry("zzz", EntryKind::Directory);
        let file = entry("aaa", EntryKind::File);
        assert_eq!(order.compare(&folder, &file), Ordering::Greater);
    }

    #[test]
    fn a_descending_name_sort_actually_reverses_the_names() {
        let ascending = SortOrder::new(SortKey::Name, SortDirection::Ascending);
        let descending = SortOrder::new(SortKey::Name, SortDirection::Descending);
        let first = entry("alpha", EntryKind::File);
        let second = entry("beta", EntryKind::File);
        assert_eq!(ascending.compare(&first, &second), Ordering::Less);
        assert_eq!(descending.compare(&first, &second), Ordering::Greater);
    }

    #[test]
    fn an_unmeasured_size_sorts_after_every_measured_one() {
        let order = SortOrder::new(SortKey::Size, SortDirection::Ascending);
        let mut known = entry("known", EntryKind::File);
        known.size = EntrySize::Bytes(0);
        let unknown = entry("unknown", EntryKind::File);
        assert_eq!(order.compare(&known, &unknown), Ordering::Less);
    }

    #[test]
    fn equal_primary_keys_break_the_tie_by_name_in_both_directions() {
        let mut first = entry("alpha", EntryKind::File);
        let mut second = entry("beta", EntryKind::File);
        first.modified = Some(FileTime::new(100, 0));
        second.modified = Some(FileTime::new(100, 0));
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            let order = SortOrder::new(SortKey::Modified, direction);
            assert_eq!(order.compare(&first, &second), Ordering::Less);
        }
    }
}
