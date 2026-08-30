//! Where launch history comes from, kept abstract on purpose.
//!
//! Issue #2 requires that usage-frequency storage be documented and removable,
//! and it deliberately does not decide whether usage frequency should adjust
//! ranking by default. Both of those are structural requirements, so this
//! module is a trait and two implementations and nothing else:
//!
//! - [`NoUsage`] remembers nothing. It is the default, and it is what the
//!   launcher runs on while any real store is still loading, so the list is
//!   usable before history is.
//! - [`InMemoryUsage`] remembers within one process. It is what tests and
//!   benchmarks use, and it is the reference for what a persistent
//!   implementation has to provide.
//!
//! A persistent store belongs in `launcher-platform`, not here: this crate
//! performs no I/O. Removing usage history entirely means dropping the
//! implementation and passing [`NoUsage`], with no change to matching or
//! ranking, because ranking only ever reads [`UsageStore::launch_count`] and
//! only when it has been explicitly switched on.
//!
//! Nothing here stores a query. Issue #2 forbids persisting typed queries by
//! default, and the way to keep that true is to give this interface no place
//! to put one.

use std::collections::BTreeMap;

use app_catalog_core::DesktopId;

/// A launch, as the launcher observed it. The timestamp is supplied by the
/// caller rather than read from the clock, so a store stays free of I/O and a
/// test can describe any history it likes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchEvent {
    /// Seconds since the Unix epoch.
    pub at: u64,
}

/// Local launch history for ranking. Read-only for ranking, written only by
/// the surface that actually launched something.
pub trait UsageStore {
    /// How many times this application was launched. Zero for an application
    /// the store has never seen.
    fn launch_count(&self, desktop_id: &DesktopId) -> u32;

    /// When it was last launched, if ever.
    fn last_launch(&self, desktop_id: &DesktopId) -> Option<u64>;

    /// Records one launch.
    fn record_launch(&mut self, desktop_id: &DesktopId, event: LaunchEvent);
}

/// A store that records nothing and reports nothing. The default, and the
/// proof that ranking works with no history at all.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoUsage;

impl UsageStore for NoUsage {
    fn launch_count(&self, _desktop_id: &DesktopId) -> u32 {
        0
    }

    fn last_launch(&self, _desktop_id: &DesktopId) -> Option<u64> {
        None
    }

    fn record_launch(&mut self, _desktop_id: &DesktopId, _event: LaunchEvent) {}
}

/// Launch history held for the lifetime of one process and never written
/// anywhere.
#[derive(Clone, Debug, Default)]
pub struct InMemoryUsage {
    entries: BTreeMap<DesktopId, Entry>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    count: u32,
    last: u64,
}

impl InMemoryUsage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every application the store has seen, in desktop ID order.
    pub fn tracked(&self) -> impl Iterator<Item = &DesktopId> {
        self.entries.keys()
    }

    /// Forgets one application, which is what "remove this from my history"
    /// has to be able to do.
    pub fn forget(&mut self, desktop_id: &DesktopId) {
        self.entries.remove(desktop_id);
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl UsageStore for InMemoryUsage {
    fn launch_count(&self, desktop_id: &DesktopId) -> u32 {
        self.entries
            .get(desktop_id)
            .map(|entry| entry.count)
            .unwrap_or(0)
    }

    fn last_launch(&self, desktop_id: &DesktopId) -> Option<u64> {
        self.entries.get(desktop_id).map(|entry| entry.last)
    }

    fn record_launch(&mut self, desktop_id: &DesktopId, event: LaunchEvent) {
        let entry = self.entries.entry(desktop_id.clone()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.last = entry.last.max(event.at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> DesktopId {
        DesktopId::new(value).expect("valid desktop id")
    }

    #[test]
    fn the_no_op_store_reports_nothing_and_keeps_nothing() {
        let mut store = NoUsage;
        store.record_launch(&id("gimp.desktop"), LaunchEvent { at: 100 });
        assert_eq!(store.launch_count(&id("gimp.desktop")), 0);
        assert_eq!(store.last_launch(&id("gimp.desktop")), None);
    }

    #[test]
    fn the_in_memory_store_counts_launches_and_keeps_the_latest_timestamp() {
        let mut store = InMemoryUsage::new();
        store.record_launch(&id("gimp.desktop"), LaunchEvent { at: 100 });
        store.record_launch(&id("gimp.desktop"), LaunchEvent { at: 250 });
        assert_eq!(store.launch_count(&id("gimp.desktop")), 2);
        assert_eq!(store.last_launch(&id("gimp.desktop")), Some(250));
        assert_eq!(store.launch_count(&id("files.desktop")), 0);
    }

    #[test]
    fn an_out_of_order_timestamp_never_moves_the_last_launch_backwards() {
        let mut store = InMemoryUsage::new();
        store.record_launch(&id("gimp.desktop"), LaunchEvent { at: 250 });
        store.record_launch(&id("gimp.desktop"), LaunchEvent { at: 100 });
        assert_eq!(store.last_launch(&id("gimp.desktop")), Some(250));
    }

    #[test]
    fn history_can_be_removed_one_application_at_a_time_or_entirely() {
        let mut store = InMemoryUsage::new();
        store.record_launch(&id("gimp.desktop"), LaunchEvent { at: 1 });
        store.record_launch(&id("files.desktop"), LaunchEvent { at: 2 });
        store.forget(&id("gimp.desktop"));
        assert_eq!(
            store.tracked().map(DesktopId::as_str).collect::<Vec<_>>(),
            vec!["files.desktop"]
        );
        store.clear();
        assert_eq!(store.tracked().count(), 0);
    }
}
