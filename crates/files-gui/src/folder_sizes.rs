//! Folder sizes: which folders of the open view are measured, the measuring
//! thread, the cache, and what a folder's Size cell says (ticket 126).
//!
//! The listing is never slowed by this. Nothing is asked for until the pane's
//! listing has finished, and the walks run on one thread of their own, one
//! folder after another, in the order the view lists them. A size that
//! arrives is written into the directory model as the entry's
//! [`EntrySize::Bytes`], so sorting by size and the selection's total use the
//! same number the cell shows, and the model's identity-keyed selection stays
//! on the same entries while they move.
//!
//! Leaving the folder cancels its walks (the token is checked between two
//! entries), and every visit shares one [`SizeBudget`]: a folder that hits a
//! bound says "More than N items", a folder never reached because the visit's
//! budget ran out says why, and neither shows a number it does not have.
//!
//! A result is kept for [`FRESH_FOR`] and forgotten earlier when the folder
//! watcher reports a change to the folder, inside the folder being viewed, or
//! to any folder beneath a cached one that the watcher can see. The watcher is
//! one level deep, so a change several levels down in a folder that is not
//! open is noticed only when the result goes stale; that limit is recorded in
//! `engineering/specs/files-gui-policy.md`.
//!
//! Network shares and FUSE mounts are not walked unless the person turns that
//! on, and the Trash never is; their cells say "—" with the reason.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use files_core::{CancellationToken, DirectoryModel, Entry, EntryKind, EntrySize, Location};
use files_platform::{FolderSize, FolderSizeLimits, MountTable, Reach, SizeBudget};

use crate::i18n::Copy;

/// How long a measured size is trusted without the watcher saying otherwise.
pub const FRESH_FOR: Duration = Duration::from_secs(300);

/// What every walk of one folder visit may look at together.
pub const VISIT_ITEMS: u64 = 5_000_000;

/// At most this many measured folders of one visit are watched for changes
/// inside them. Every watch is an inotify watch of the process's one
/// instance, which the kernel counts against the user's limit.
pub const CHILD_WATCHES: usize = 256;

/// How long one folder visit's walks may run together.
pub const VISIT_TIME: Duration = Duration::from_secs(120);

/// Why a folder is not measured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotMeasured {
    /// On a network share, and network sizes are off.
    Network,
    /// On a filesystem a program serves, and network sizes are off.
    Fuse,
    /// In the Trash.
    Trash,
    /// The visit's budget ran out before this folder was reached.
    Budget,
}

/// What a folder's Size cell says.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderSizeState {
    Calculating,
    Known(FolderSize),
    NotMeasured(NotMeasured),
}

/// A folder's cell, formatted: the text and, when there is one, the reason
/// behind it for a tooltip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SizeLabel {
    pub text: String,
    pub tooltip: Option<String>,
    /// The text is a byte count, as a file's is.
    pub is_value: bool,
}

/// Where the sizes of one visit come from.
struct Visit {
    folder: Location,
    token: CancellationToken,
    requests: Option<Sender<(PathBuf, u64)>>,
    results: Receiver<(PathBuf, u64, FolderSize)>,
    /// Asked for and not answered yet, with the number of the request. An
    /// answer to an older request (one asked before the watcher saw the
    /// folder change) is dropped.
    pending: HashMap<PathBuf, u64>,
    serial: u64,
    mounts: MountTable,
    /// A watch on each measured folder of this one, so a change directly
    /// inside one is seen while the person is looking at its total.
    watches: Vec<files_platform::Subscription>,
}

impl Drop for Visit {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

type MountSource = Arc<dyn Fn() -> MountTable + Send + Sync>;

/// The folder sizes of one window.
pub struct FolderSizes {
    cache: HashMap<PathBuf, (FolderSize, Instant)>,
    /// Folders a visit decided not to walk, for the cell's reason.
    skipped: HashMap<PathBuf, NotMeasured>,
    visit: Option<Visit>,
    limits: FolderSizeLimits,
    visit_items: u64,
    visit_time: Duration,
    mounts: MountSource,
    hub: Option<Arc<files_platform::WatchHub>>,
}

impl Default for FolderSizes {
    fn default() -> Self {
        Self::new(FolderSizeLimits::default(), Arc::new(MountTable::from_env))
    }
}

impl FolderSizes {
    pub fn new(limits: FolderSizeLimits, mounts: MountSource) -> Self {
        Self {
            cache: HashMap::new(),
            skipped: HashMap::new(),
            visit: None,
            limits,
            visit_items: VISIT_ITEMS,
            visit_time: VISIT_TIME,
            mounts,
            hub: None,
        }
    }

    /// Watches measured folders through `hub` (the window's), so a change
    /// directly inside one forgets its size.
    pub fn watching_with(mut self, hub: Arc<files_platform::WatchHub>) -> Self {
        self.hub = Some(hub);
        self
    }

    /// The same, with a smaller visit budget (tests and the benchmark).
    pub fn with_visit_budget(mut self, items: u64, time: Duration) -> Self {
        self.visit_items = items;
        self.visit_time = time;
        self
    }

    /// Stops measuring: the person turned folder sizes off. What was
    /// measured is kept for when they turn it back on.
    pub fn stop(&mut self) {
        self.visit = None;
        self.skipped.clear();
    }

    /// Takes every folder size back out of the model, so a size sort with
    /// folder sizes off sorts folders as it did before them.
    pub fn clear_model(model: &mut DirectoryModel) -> bool {
        let cleared: Vec<_> = model
            .iter_all()
            .filter(|entry| entry.kind == EntryKind::Directory && entry.size != EntrySize::Unknown)
            .map(|entry| (entry.id(), EntrySize::Unknown))
            .collect();
        model.set_sizes(cleared) > 0
    }

    /// Whether walks are still running or queued.
    pub fn is_busy(&self) -> bool {
        self.visit
            .as_ref()
            .is_some_and(|visit| !visit.pending.is_empty())
    }

    /// The visit's folder, or `None`.
    pub fn visiting(&self) -> Option<&Location> {
        self.visit.as_ref().map(|visit| &visit.folder)
    }

    /// Brings the model's folder sizes up to date: starts a visit when the
    /// folder changed (cancelling the last one), takes the sizes that
    /// arrived, asks for the folders still unanswered, and writes what is
    /// known into the model. `listed` is false while the listing is still
    /// arriving, and then nothing is asked for. Returns whether the model
    /// changed.
    pub fn pump(
        &mut self,
        model: &mut DirectoryModel,
        listed: bool,
        network: bool,
        scan: bool,
    ) -> bool {
        let folder = model.location().clone();
        let mut scan = scan;
        if self.visit.as_ref().map(|visit| &visit.folder) != Some(&folder) {
            self.visit = None;
            self.skipped.clear();
            if !listed || folder.as_local_path().is_none() && !matches!(folder, Location::Trash(_))
            {
                return false;
            }
            self.visit = Some(self.start_visit(folder.clone()));
            scan = true;
        }
        let Some(visit) = self.visit.as_mut() else {
            return false;
        };
        // A change directly inside a measured folder forgets it, and every
        // folder above it, and the scan asks for it again.
        let touched: Vec<PathBuf> = visit
            .watches
            .iter()
            .filter(|watch| {
                watch.poll().into_iter().any(|update| {
                    !matches!(
                        update,
                        files_platform::HubUpdate::Ready { .. }
                            | files_platform::HubUpdate::Suspended
                    )
                })
            })
            .map(|watch| watch.directory().to_path_buf())
            .collect();
        for folder in touched {
            self.invalidate(&folder, &[]);
            scan = true;
        }
        let Some(visit) = self.visit.as_mut() else {
            return false;
        };
        let mut sizes = Vec::new();
        while let Ok((path, serial, size)) = visit.results.try_recv() {
            if visit.pending.get(&path) != Some(&serial) {
                continue;
            }
            visit.pending.remove(&path);
            if size == FolderSize::Cancelled {
                // The visit's budget ran out under this walk; the token is
                // the visit's own, so a cancelled visit never reaches here.
                self.skipped.insert(path, NotMeasured::Budget);
                continue;
            }
            if let Some(hub) = &self.hub
                && matches!(size, FolderSize::Total { .. })
                && visit.watches.len() < CHILD_WATCHES
                && !visit.watches.iter().any(|watch| watch.directory() == path)
            {
                visit
                    .watches
                    .push(hub.subscribe(&path, files_platform::ReaderConfig::new()));
            }
            self.cache.insert(path.clone(), (size, Instant::now()));
            sizes.push((path, size));
        }
        let mut changed = false;
        if !sizes.is_empty() {
            let parent = folder
                .as_local_path()
                .map(|path| path.as_path().to_path_buf());
            let by_name: Vec<_> = sizes
                .into_iter()
                .filter(|(path, _)| path.parent() == parent.as_deref())
                .filter_map(|(path, size)| {
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    Some((files_core::EntryId::Name(name), model_size(size)))
                })
                .collect();
            changed |= model.set_sizes(by_name) > 0;
        }
        if scan && listed {
            changed |= self.scan(model, network);
        }
        changed
    }

    fn start_visit(&self, folder: Location) -> Visit {
        let token = CancellationToken::new();
        let (requests, queue) = channel::<(PathBuf, u64)>();
        let (answers, results) = channel();
        let budget = SizeBudget::new(self.visit_items, self.visit_time, token.clone());
        let limits = self.limits;
        let spawned = std::thread::Builder::new()
            .name("files-folder-sizes".into())
            .spawn(move || {
                for (path, serial) in queue {
                    if budget.token().is_cancelled() {
                        return;
                    }
                    let size = if budget.is_spent() {
                        FolderSize::Cancelled
                    } else {
                        files_platform::measure_folder(&path, limits, &budget)
                    };
                    if budget.token().is_cancelled() || answers.send((path, serial, size)).is_err()
                    {
                        return;
                    }
                }
            });
        Visit {
            folder,
            token,
            // A thread that could not be started takes no requests; the
            // folders stay "Calculating" would be a lie, so the cells fall
            // back to the budget reason below.
            requests: spawned.ok().map(|_| requests),
            results,
            pending: HashMap::new(),
            serial: 0,
            watches: Vec::new(),
            mounts: (self.mounts)(),
        }
    }

    /// Writes cached sizes into the model and asks for the folders that have
    /// none. Folders are asked for in the order the view shows them.
    fn scan(&mut self, model: &mut DirectoryModel, network: bool) -> bool {
        let Some(visit) = self.visit.as_mut() else {
            return false;
        };
        let in_trash = matches!(visit.folder, Location::Trash(_));
        let mut known = Vec::new();
        let now = Instant::now();
        for entry in model.iter_visible() {
            if entry.kind != EntryKind::Directory {
                continue;
            }
            let Some(path) = entry
                .as_local_path()
                .map(|path| path.as_path().to_path_buf())
            else {
                continue;
            };
            if in_trash || matches!(entry.body, files_core::EntryBody::Trashed(_)) {
                self.skipped.insert(path, NotMeasured::Trash);
                continue;
            }
            match visit.mounts.reach(&path) {
                Reach::Network if !network => {
                    self.skipped.insert(path, NotMeasured::Network);
                    continue;
                }
                Reach::Fuse if !network => {
                    self.skipped.insert(path, NotMeasured::Fuse);
                    continue;
                }
                _ => {
                    self.skipped.remove(&path);
                }
            }
            if let Some((size, at)) = self.cache.get(&path)
                && now.duration_since(*at) < FRESH_FOR
            {
                let wanted = model_size(*size);
                if entry.size != wanted {
                    known.push((entry.id(), wanted));
                }
                continue;
            }
            if visit.pending.contains_key(&path) || self.skipped.contains_key(&path) {
                continue;
            }
            visit.serial += 1;
            match &visit.requests {
                Some(requests) if requests.send((path.clone(), visit.serial)).is_ok() => {
                    visit.pending.insert(path, visit.serial);
                }
                _ => {
                    self.skipped.insert(path, NotMeasured::Budget);
                }
            }
        }
        model.set_sizes(known) > 0
    }

    /// The watcher saw `changed` (entries of `folder`) change, or the whole of
    /// `folder` when `changed` is empty: their sizes, `folder`'s own, and
    /// every cached folder above it are forgotten, and the ones in view are
    /// measured again.
    pub fn invalidate(&mut self, folder: &Path, changed: &[PathBuf]) {
        let whole = changed.is_empty();
        self.cache.retain(|path, _| {
            let above = folder.starts_with(path);
            let inside = if whole {
                path.starts_with(folder)
            } else {
                changed.iter().any(|changed| path.starts_with(changed))
            };
            !(above || inside)
        });
        // A folder that a walk is measuring right now is asked for again
        // when its answer is in: the answer may predate the change.
        if let Some(visit) = self.visit.as_mut() {
            let stale: Vec<PathBuf> = visit
                .pending
                .keys()
                .filter(|path| whole && path.starts_with(folder) || changed.contains(path))
                .cloned()
                .collect();
            for path in stale {
                visit.pending.remove(&path);
            }
        }
    }

    /// The state of one folder entry, or `None` for an entry that is not a
    /// folder.
    pub fn state(&self, entry: &Entry) -> Option<FolderSizeState> {
        if entry.kind != EntryKind::Directory {
            return None;
        }
        let path = entry.as_local_path()?.as_path();
        if let Some(reason) = self.skipped.get(path) {
            return Some(FolderSizeState::NotMeasured(*reason));
        }
        if let Some(visit) = &self.visit
            && visit.pending.contains_key(path)
        {
            return Some(FolderSizeState::Calculating);
        }
        if let Some((size, _)) = self.cache.get(path) {
            return Some(FolderSizeState::Known(*size));
        }
        // A folder of the visit's own folder that the scan has not reached
        // yet (it arrived this frame) is about to be asked for. Any other
        // folder — one shown by another column — has no answer.
        self.visit
            .as_ref()
            .filter(|visit| {
                visit.folder.as_local_path().map(|folder| folder.as_path()) == path.parent()
            })
            .map(|_| FolderSizeState::Calculating)
    }

    /// What an entry's Size cell says. A file's is its size; a folder's is
    /// its state; with sizes off, a folder says "—".
    pub fn label(&self, entry: &Entry, enabled: bool, c: &'static Copy) -> SizeLabel {
        let plain = |size: EntrySize| SizeLabel {
            text: crate::format::entry_size(size),
            tooltip: None,
            is_value: size.bytes().is_some(),
        };
        if !enabled {
            return plain(if entry.kind == EntryKind::Directory {
                EntrySize::Unknown
            } else {
                entry.size
            });
        }
        match self.state(entry) {
            None => plain(entry.size),
            Some(state) => state_label(state, c),
        }
    }
}

/// The size a model entry holds for a measured folder. Only a total has a
/// number; a partial one is the readable part's, which is a lower bound and
/// sorts as one.
fn model_size(size: FolderSize) -> EntrySize {
    match size {
        FolderSize::Total { bytes, .. } => EntrySize::Bytes(bytes),
        _ => EntrySize::Unknown,
    }
}

/// A folder's state in words.
pub fn state_label(state: FolderSizeState, c: &'static Copy) -> SizeLabel {
    let label = |text: String, tooltip: Option<&str>| SizeLabel {
        text,
        tooltip: tooltip.map(str::to_string),
        is_value: false,
    };
    match state {
        FolderSizeState::Calculating => label(c.size_calculating.to_string(), None),
        FolderSizeState::Known(FolderSize::Total {
            bytes, unreadable, ..
        }) if unreadable > 0 => SizeLabel {
            text: format!("≥ {}", crate::format::bytes(bytes)),
            tooltip: Some(if unreadable == 1 {
                c.size_partial_one.to_string()
            } else {
                c.size_partial.replace("{count}", &group_digits(unreadable))
            }),
            is_value: true,
        },
        FolderSizeState::Known(FolderSize::Total { bytes, .. }) => SizeLabel {
            text: crate::format::bytes(bytes),
            tooltip: None,
            is_value: true,
        },
        FolderSizeState::Known(FolderSize::TooMany { items }) => label(
            c.size_more_than.replace("{count}", &group_digits(items)),
            Some(c.size_more_than_reason),
        ),
        FolderSizeState::Known(FolderSize::Unreadable) => label(
            c.size_unreadable.to_string(),
            Some(c.size_unreadable_reason),
        ),
        FolderSizeState::Known(FolderSize::Cancelled)
        | FolderSizeState::NotMeasured(NotMeasured::Budget) => {
            label("—".to_string(), Some(c.size_skipped_budget))
        }
        FolderSizeState::NotMeasured(NotMeasured::Network) => {
            label("—".to_string(), Some(c.size_skipped_network))
        }
        FolderSizeState::NotMeasured(NotMeasured::Fuse) => {
            label("—".to_string(), Some(c.size_skipped_fuse))
        }
        FolderSizeState::NotMeasured(NotMeasured::Trash) => {
            label("—".to_string(), Some(c.size_skipped_trash))
        }
    }
}

/// `1234567` as `1,234,567`.
pub fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The status bar's total for a selection: the sum of what is known, "≥"
/// when part of it has no number, "Calculating…" while a selected folder is
/// still being measured. `None` when nothing selected has a size at all.
pub fn selection_total(
    entries: &[&Entry],
    sizes: &FolderSizes,
    enabled: bool,
    c: &'static Copy,
) -> Option<String> {
    let mut bytes = 0u64;
    let mut any = false;
    let mut partial = false;
    for entry in entries {
        if entry.kind == EntryKind::Directory {
            if !enabled {
                partial = true;
                continue;
            }
            match sizes.state(entry) {
                Some(FolderSizeState::Calculating) => return Some(c.size_calculating.to_string()),
                Some(FolderSizeState::Known(FolderSize::Total {
                    bytes: folder,
                    unreadable,
                    ..
                })) => {
                    bytes = bytes.saturating_add(folder);
                    any = true;
                    partial |= unreadable > 0;
                }
                _ => partial = true,
            }
            continue;
        }
        match entry.size {
            EntrySize::Bytes(value) => {
                bytes = bytes.saturating_add(value);
                any = true;
            }
            EntrySize::Unknown => partial = true,
            EntrySize::NotApplicable => {}
        }
    }
    if !any {
        return None;
    }
    let total = crate::format::bytes(bytes);
    Some(if partial {
        format!("≥ {total}")
    } else {
        total
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use files_core::{EntryId, HiddenPreference, LocalPath, SortDirection, SortKey, SortOrder};
    use std::fs;

    fn sizes(mounts: MountTable) -> FolderSizes {
        let mounts = Arc::new(mounts);
        FolderSizes::new(
            FolderSizeLimits::default(),
            Arc::new(move || (*mounts).clone()),
        )
    }

    fn model_of(folder: &Path, order: SortOrder) -> DirectoryModel {
        let mut model = DirectoryModel::new(
            Location::local(folder).unwrap(),
            order,
            HiddenPreference::default(),
        );
        let mut entries = Vec::new();
        for child in fs::read_dir(folder).unwrap() {
            let child = child.unwrap();
            let name = child.file_name().to_string_lossy().into_owned();
            let kind = if child.file_type().unwrap().is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            let mut entry = Entry::file(name, LocalPath::new(child.path()).unwrap(), kind);
            if kind == EntryKind::File {
                entry.size = EntrySize::Bytes(child.metadata().unwrap().len());
            }
            entries.push(entry);
        }
        model.insert_batch(entries);
        model
    }

    /// Pumps until nothing is pending, as the window's timer does.
    fn settle(sizes: &mut FolderSizes, model: &mut DirectoryModel, network: bool) {
        sizes.pump(model, true, network, true);
        let deadline = Instant::now() + Duration::from_secs(20);
        while sizes.is_busy() {
            assert!(Instant::now() < deadline, "sizes never arrived");
            std::thread::sleep(Duration::from_millis(5));
            sizes.pump(model, true, network, false);
        }
    }

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("big/inner")).unwrap();
        fs::write(dir.path().join("big/inner/a"), vec![0u8; 3000]).unwrap();
        fs::create_dir(dir.path().join("small")).unwrap();
        fs::write(dir.path().join("small/b"), vec![0u8; 10]).unwrap();
        fs::write(dir.path().join("file"), vec![0u8; 500]).unwrap();
        dir
    }

    fn names(model: &DirectoryModel) -> Vec<String> {
        model
            .iter_visible()
            .map(|entry| entry.name.clone())
            .collect()
    }

    fn entry<'a>(model: &'a DirectoryModel, name: &str) -> &'a Entry {
        model.get(&EntryId::Name(name.into())).unwrap()
    }

    const C: &Copy = &crate::i18n::EN_US;

    #[test]
    fn sizes_arrive_after_the_listing_resort_the_view_and_keep_the_selection() {
        let dir = tree();
        let order = SortOrder::new(SortKey::Size, SortDirection::Descending);
        let mut model = model_of(dir.path(), order);
        let small = EntryId::Name("small".into());
        model.selection_mut().select_only(small.clone());
        let mut sizes = sizes(MountTable::default());
        // Nothing is asked for while the listing is still arriving.
        assert!(!sizes.pump(&mut model, false, false, true));
        assert!(sizes.visiting().is_none());
        assert_eq!(
            sizes.label(entry(&model, "big"), true, C).text,
            "—",
            "no visit yet: nothing claims to be calculating"
        );
        sizes.pump(&mut model, true, false, true);
        assert_eq!(
            sizes.label(entry(&model, "big"), true, C).text,
            C.size_calculating
        );
        // Folders still being measured sort after every known size.
        assert_eq!(names(&model)[..], ["big", "small", "file"][..]);
        settle(&mut sizes, &mut model, false);
        assert_eq!(entry(&model, "big").size, EntrySize::Bytes(3000));
        assert_eq!(entry(&model, "small").size, EntrySize::Bytes(10));
        assert_eq!(names(&model)[..], ["big", "small", "file"][..]);
        assert!(model.selection().contains(&small));
        assert_eq!(model.selection().len(), 1);
        let label = sizes.label(entry(&model, "big"), true, C);
        assert!(label.is_value);
        assert_eq!(label.text, crate::format::bytes(3000));
        // With folder sizes off, a folder says nothing.
        assert_eq!(sizes.label(entry(&model, "big"), false, C).text, "—");
        assert!(FolderSizes::clear_model(&mut model));
        assert_eq!(entry(&model, "big").size, EntrySize::Unknown);
    }

    #[test]
    fn ascending_sort_puts_folders_still_calculating_last() {
        let dir = tree();
        let order =
            SortOrder::new(SortKey::Size, SortDirection::Ascending).with_folders_first(false);
        let mut model = model_of(dir.path(), order);
        let mut sizes = sizes(MountTable::default());
        sizes.pump(&mut model, true, false, true);
        assert_eq!(names(&model)[0], "file");
        settle(&mut sizes, &mut model, false);
        assert_eq!(names(&model)[..], ["small", "file", "big"][..]);
    }

    #[test]
    fn leaving_the_folder_cancels_its_walks_and_drops_their_answers() {
        let first = tree();
        let second = tree();
        let order = SortOrder::default();
        let mut model = model_of(first.path(), order);
        let mut sizes = sizes(MountTable::default());
        sizes.pump(&mut model, true, false, true);
        let token = sizes.visit.as_ref().unwrap().token.clone();
        let mut other = model_of(second.path(), order);
        settle(&mut sizes, &mut other, false);
        assert!(
            token.is_cancelled(),
            "the first visit's walks were told to stop"
        );
        assert_eq!(sizes.visiting(), Some(other.location()));
        // The first folder's answers, if any arrived, went nowhere: its
        // entries were never given a size, and nothing about them is cached.
        assert_eq!(entry(&model, "big").size, EntrySize::Unknown);
        assert!(
            !sizes
                .cache
                .keys()
                .any(|path| path.starts_with(first.path()))
        );
    }

    #[test]
    fn the_visit_budget_says_why_a_folder_has_no_number() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a", "b", "c"] {
            fs::create_dir(dir.path().join(name)).unwrap();
            for index in 0..5 {
                fs::write(dir.path().join(name).join(index.to_string()), b"x").unwrap();
            }
        }
        let mut model = model_of(dir.path(), SortOrder::default());
        let mut sizes = sizes(MountTable::default()).with_visit_budget(7, Duration::from_secs(60));
        settle(&mut sizes, &mut model, false);
        assert_eq!(entry(&model, "a").size, EntrySize::Bytes(5));
        let b = sizes.label(entry(&model, "b"), true, C);
        assert_eq!(b.text, "More than 2 items");
        assert_eq!(b.tooltip.as_deref(), Some(C.size_more_than_reason));
        assert!(!b.is_value);
        let c = sizes.label(entry(&model, "c"), true, C);
        assert_eq!(c.text, "—");
        assert_eq!(c.tooltip.as_deref(), Some(C.size_skipped_budget));
        assert_eq!(entry(&model, "c").size, EntrySize::Unknown);
    }

    #[test]
    fn the_watcher_forgets_a_changed_folder_and_it_is_measured_again() {
        let dir = tree();
        let mut model = model_of(dir.path(), SortOrder::default());
        let mut sizes = sizes(MountTable::default());
        settle(&mut sizes, &mut model, false);
        assert_eq!(entry(&model, "small").size, EntrySize::Bytes(10));
        // Revisiting from the cache asks for nothing.
        let mut again = model_of(dir.path(), SortOrder::default());
        sizes.visit = None;
        sizes.pump(&mut again, true, false, true);
        assert!(!sizes.is_busy());
        assert_eq!(entry(&again, "small").size, EntrySize::Bytes(10));

        fs::write(dir.path().join("small/c"), vec![0u8; 90]).unwrap();
        sizes.invalidate(dir.path(), &[dir.path().join("small")]);
        // The watcher's row for the folder comes with no size, as the
        // listing's do.
        let mut fresh = entry(&again, "small").clone();
        fresh.size = EntrySize::Unknown;
        again.insert_batch(vec![fresh]);
        sizes.pump(&mut again, true, false, true);
        assert_eq!(
            sizes.label(entry(&again, "small"), true, C).text,
            C.size_calculating
        );
        settle(&mut sizes, &mut again, false);
        assert_eq!(entry(&again, "small").size, EntrySize::Bytes(100));
        assert_eq!(
            entry(&again, "big").size,
            EntrySize::Bytes(3000),
            "a sibling is kept"
        );
        // A change inside the folder being viewed forgets the folder and
        // every cached folder above it.
        sizes.cache.insert(
            dir.path().to_path_buf(),
            (FolderSize::Unreadable, Instant::now()),
        );
        sizes.invalidate(&dir.path().join("big"), &[dir.path().join("big/new")]);
        assert!(!sizes.cache.contains_key(dir.path()));
        assert!(!sizes.cache.contains_key(&dir.path().join("big")));
        assert!(sizes.cache.contains_key(&dir.path().join("small")));
    }

    #[test]
    fn a_change_directly_inside_a_measured_folder_is_seen_by_its_watch() {
        let hub = files_platform::WatchHub::isolate_this_thread();
        let dir = tree();
        let mut model = model_of(dir.path(), SortOrder::default());
        let mut sizes = sizes(MountTable::default()).watching_with(hub);
        settle(&mut sizes, &mut model, false);
        assert_eq!(entry(&model, "small").size, EntrySize::Bytes(10));
        // The watch is placed off this thread; give it its moment.
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            fs::write(dir.path().join("small/c"), vec![0u8; 90]).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            sizes.pump(&mut model, true, false, false);
            settle(&mut sizes, &mut model, false);
            if entry(&model, "small").size == EntrySize::Bytes(100) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the change inside was never seen"
            );
        }
    }

    #[test]
    fn an_answer_asked_for_before_a_change_is_dropped() {
        let dir = tree();
        let mut model = model_of(dir.path(), SortOrder::default());
        let mut sizes = sizes(MountTable::default());
        sizes.pump(&mut model, true, false, true);
        // The change is seen while the first walk may still be running.
        sizes.invalidate(dir.path(), &[dir.path().join("small")]);
        fs::write(dir.path().join("small/c"), vec![0u8; 90]).unwrap();
        settle(&mut sizes, &mut model, false);
        assert_eq!(entry(&model, "small").size, EntrySize::Bytes(100));
    }

    #[test]
    fn network_and_fuse_folders_wait_for_the_setting() {
        let dir = tree();
        let table = tempfile::NamedTempFile::new().unwrap();
        fs::write(
            table.path(),
            format!(
                "28 1 8:2 / / rw - ext4 /dev/sda2 rw\n92 28 0:51 / {} rw - cifs //nas/s rw\n93 28 0:52 / {} rw - fuse.sshfs host: rw\n",
                dir.path().join("big").display(),
                dir.path().join("small").display(),
            ),
        )
        .unwrap();
        let mounts = files_platform::read_mount_table(table.path());
        let mut model = model_of(dir.path(), SortOrder::default());
        let mut sizes = sizes(mounts);
        settle(&mut sizes, &mut model, false);
        let big = sizes.label(entry(&model, "big"), true, C);
        assert_eq!(
            (big.text.as_str(), big.tooltip.as_deref()),
            ("—", Some(C.size_skipped_network))
        );
        let small = sizes.label(entry(&model, "small"), true, C);
        assert_eq!(small.tooltip.as_deref(), Some(C.size_skipped_fuse));
        // Turned on, the visit starts over and walks them.
        sizes.stop();
        settle(&mut sizes, &mut model, true);
        assert_eq!(entry(&model, "big").size, EntrySize::Bytes(3000));
        assert_eq!(entry(&model, "small").size, EntrySize::Bytes(10));
    }

    #[test]
    fn an_unreadable_folder_says_so_and_a_partly_readable_one_is_a_lower_bound() {
        if files_platform::current_uid() == Some(0) {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let dir = tree();
        fs::create_dir(dir.path().join("big/locked")).unwrap();
        fs::set_permissions(
            dir.path().join("big/locked"),
            fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        fs::create_dir(dir.path().join("closed")).unwrap();
        fs::set_permissions(dir.path().join("closed"), fs::Permissions::from_mode(0o000)).unwrap();
        let mut model = model_of(dir.path(), SortOrder::default());
        let mut sizes = sizes(MountTable::default());
        settle(&mut sizes, &mut model, false);
        for path in ["big/locked", "closed"] {
            fs::set_permissions(dir.path().join(path), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let closed = sizes.label(entry(&model, "closed"), true, C);
        assert_eq!(closed.text, "Can't read");
        assert_ne!(entry(&model, "closed").size, EntrySize::Bytes(0));
        let big = sizes.label(entry(&model, "big"), true, C);
        assert_eq!(big.text, format!("≥ {}", crate::format::bytes(3000)));
        assert_eq!(big.tooltip.as_deref(), Some(C.size_partial_one));
        let zh = crate::i18n::copy(crate::i18n::Locale::ZhTw);
        assert_eq!(
            sizes.label(entry(&model, "closed"), true, zh).text,
            "無法讀取"
        );
    }

    #[test]
    fn the_selection_total_counts_folders_and_says_when_it_cannot() {
        let dir = tree();
        let mut model = model_of(dir.path(), SortOrder::default());
        let mut sizes = sizes(MountTable::default());
        sizes.pump(&mut model, true, false, true);
        let all: Vec<&Entry> = model.iter_visible().collect();
        assert_eq!(
            selection_total(&all, &sizes, true, C).as_deref(),
            Some(C.size_calculating)
        );
        settle(&mut sizes, &mut model, false);
        let all: Vec<&Entry> = model.iter_visible().collect();
        assert_eq!(
            selection_total(&all, &sizes, true, C),
            Some(crate::format::bytes(3510))
        );
        assert_eq!(
            selection_total(&all, &sizes, false, C),
            Some(format!("≥ {}", crate::format::bytes(500)))
        );
        let folders: Vec<&Entry> = all
            .into_iter()
            .filter(|e| e.kind == EntryKind::Directory)
            .collect();
        assert_eq!(selection_total(&folders, &sizes, false, C), None);
    }

    #[test]
    fn a_grid_tile_carries_a_folders_size_or_state_and_leaves_out_a_dash() {
        let mut folder = Entry::file("f", LocalPath::new("/d/f").unwrap(), EntryKind::Directory);
        folder.modified = Some(files_core::FileTime::new(0, 0));
        let mut row = crate::content::rendered_row(&folder, false, false, C);
        assert_eq!(crate::content::tile_caption(&row), "1970-01-01");
        row.size = crate::format::bytes(2048);
        row.size_is_value = true;
        assert_eq!(
            crate::content::tile_caption(&row),
            format!("{} · 1970-01-01", crate::format::bytes(2048))
        );
        row.size = C.size_unreadable.to_string();
        row.size_is_value = false;
        assert_eq!(
            crate::content::tile_caption(&row),
            format!("{} · 1970-01-01", C.size_unreadable)
        );
    }

    #[test]
    fn digits_are_grouped() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1_000_000), "1,000,000");
        let zh = crate::i18n::copy(crate::i18n::Locale::ZhTw);
        let label = state_label(
            FolderSizeState::Known(FolderSize::TooMany { items: 1_000_000 }),
            zh,
        );
        assert_eq!(label.text, "超過 1,000,000 個項目");
        assert_eq!(
            state_label(FolderSizeState::Calculating, zh).text,
            "計算中…"
        );
    }
}
