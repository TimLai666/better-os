//! A bounded listing cache with explicit invalidation.
//!
//! Issue #6 asks for caching with bounded memory and clear invalidation, and
//! the bound that matters is the number of *entries* held, not the number of
//! locations. Ten cached directories of ten entries and one cached directory
//! of a hundred thousand are very different amounts of memory, and a
//! capacity counted in locations would treat them the same.
//!
//! Nothing is cached implicitly. A caller stores a completed listing on
//! purpose, and a watcher event or a reload drops it on purpose. There is no
//! time-based expiry, because a stale directory is not made correct by being
//! recent.

use std::collections::HashMap;

use crate::entry::Entry;
use crate::location::Location;

/// A cached listing and the order it was cached in.
#[derive(Clone, Debug)]
struct CachedListing {
    entries: Vec<Entry>,
    /// Bumped on every hit, so eviction drops the least recently used.
    used: u64,
}

/// Completed listings, keyed by location URI.
///
/// Keyed by the URI rather than by `Location` so that a device location and a
/// local path can share one map without `Location` needing to be `Hash`,
/// which it cannot be while it carries a device identity.
#[derive(Debug)]
pub struct ListingCache {
    entries: HashMap<String, CachedListing>,
    capacity: usize,
    held: usize,
    clock: u64,
    hits: u64,
    misses: u64,
}

impl ListingCache {
    /// A cache holding at most `capacity` entries in total.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            held: 0,
            clock: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// How many entries are held right now.
    pub fn held(&self) -> usize {
        self.held
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// Stores a completed listing.
    ///
    /// A listing larger than the whole cache is not stored at all rather than
    /// evicting everything to make room for something that will be evicted
    /// next anyway.
    pub fn store(&mut self, location: &Location, entries: Vec<Entry>) -> bool {
        if entries.len() > self.capacity {
            return false;
        }
        let key = location.to_uri();
        self.remove_key(&key);
        self.clock += 1;
        self.held += entries.len();
        self.entries.insert(
            key,
            CachedListing {
                entries,
                used: self.clock,
            },
        );
        self.evict_until_within_capacity();
        true
    }

    /// Reads a cached listing, counting the access for eviction.
    pub fn get(&mut self, location: &Location) -> Option<&[Entry]> {
        let key = location.to_uri();
        self.clock += 1;
        let clock = self.clock;
        match self.entries.get_mut(&key) {
            Some(cached) => {
                cached.used = clock;
                self.hits += 1;
                Some(&cached.entries)
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Drops one location. This is what a watcher event and a manual reload
    /// call; it is the only way a stale listing leaves the cache.
    pub fn invalidate(&mut self, location: &Location) -> bool {
        let key = location.to_uri();
        self.remove_key(&key)
    }

    /// Drops every location under a path, which is what a device being
    /// unmounted or a directory tree being moved requires.
    pub fn invalidate_prefix(&mut self, prefix: &Location) -> usize {
        let prefix = prefix.to_uri();
        let keys: Vec<String> = self
            .entries
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect();
        let count = keys.len();
        for key in keys {
            self.remove_key(&key);
        }
        count
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.held = 0;
    }

    fn remove_key(&mut self, key: &str) -> bool {
        match self.entries.remove(key) {
            Some(cached) => {
                self.held -= cached.entries.len();
                true
            }
            None => false,
        }
    }

    fn evict_until_within_capacity(&mut self) {
        while self.held > self.capacity {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, cached)| cached.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.remove_key(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::EntryKind;
    use crate::location::LocalPath;

    fn listing(count: usize) -> Vec<Entry> {
        (0..count)
            .map(|index| {
                Entry::file(
                    format!("f{index}"),
                    LocalPath::new(format!("/d/f{index}")).unwrap(),
                    EntryKind::File,
                )
            })
            .collect()
    }

    fn at(path: &str) -> Location {
        Location::local(path).unwrap()
    }

    #[test]
    fn the_bound_is_counted_in_entries_not_locations() {
        let mut cache = ListingCache::new(10);
        cache.store(&at("/a"), listing(6));
        cache.store(&at("/b"), listing(6));
        assert_eq!(cache.len(), 1, "the older location was evicted");
        assert!(cache.held() <= cache.capacity());
    }

    #[test]
    fn eviction_drops_the_least_recently_used() {
        let mut cache = ListingCache::new(10);
        cache.store(&at("/a"), listing(4));
        cache.store(&at("/b"), listing(4));
        assert!(cache.get(&at("/a")).is_some());
        cache.store(&at("/c"), listing(4));
        assert!(cache.get(&at("/a")).is_some());
        assert!(cache.get(&at("/b")).is_none());
    }

    #[test]
    fn a_listing_larger_than_the_cache_is_refused_rather_than_emptying_it() {
        let mut cache = ListingCache::new(10);
        cache.store(&at("/keep"), listing(4));
        assert!(!cache.store(&at("/huge"), listing(11)));
        assert!(cache.get(&at("/keep")).is_some());
    }

    #[test]
    fn invalidation_is_explicit_and_reports_whether_anything_was_dropped() {
        let mut cache = ListingCache::new(100);
        cache.store(&at("/a"), listing(3));
        assert!(cache.invalidate(&at("/a")));
        assert!(!cache.invalidate(&at("/a")));
        assert_eq!(cache.held(), 0);
    }

    #[test]
    fn a_whole_subtree_can_be_invalidated_when_a_device_goes_away() {
        let mut cache = ListingCache::new(100);
        cache.store(&at("/media/stick"), listing(2));
        cache.store(&at("/media/stick/photos"), listing(2));
        cache.store(&at("/home/user"), listing(2));
        assert_eq!(cache.invalidate_prefix(&at("/media/stick")), 2);
        assert!(cache.get(&at("/home/user")).is_some());
        assert!(cache.get(&at("/media/stick/photos")).is_none());
    }

    #[test]
    fn virtual_locations_share_the_cache_without_colliding_with_paths() {
        let mut cache = ListingCache::new(100);
        cache.store(&Location::Applications, listing(2));
        cache.store(&at("/Applications"), listing(3));
        assert_eq!(
            cache.get(&Location::Applications).map(<[Entry]>::len),
            Some(2)
        );
        assert_eq!(cache.get(&at("/Applications")).map(<[Entry]>::len), Some(3));
    }
}
