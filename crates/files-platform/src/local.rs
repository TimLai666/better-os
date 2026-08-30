//! Local directory listing.
//!
//! The listing is progressive by construction. `read_dir` is an iterator over
//! the kernel's `getdents` buffer, so entries are produced as they are read;
//! this reader turns each one into an [`Entry`] and pushes it into the sink,
//! flushing a batch as soon as one fills. Nothing collects the directory
//! first, so the time to the first visible entries does not depend on how many
//! entries there are in total.
//!
//! Metadata is a per-entry `lstat`, done as part of the same pass. It is the
//! expensive half — a hundred thousand `stat` calls is real work — which is
//! why the cancellation check sits between entries rather than between
//! batches: an abandoned listing stops after at most one more `stat`.
//!
//! Every per-entry failure is recorded and the listing continues. A directory
//! with one unreadable file lists the rest of the files.

use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use files_core::entry::{
    Entry, EntryBody, EntryKind, EntrySize, FileFacts, FileTime, PermissionsSummary, SymlinkStatus,
};
use files_core::error::ListingError;
use files_core::listing::{Cancelled, DirectoryReader, ListingRequest, ListingSink};
use files_core::location::LocalPath;

use crate::hidden::read_hidden_rules;
use crate::mime::MimeDetector;
use crate::mounts::MountTable;

/// How a listing is produced.
#[derive(Clone, Default)]
pub struct ReaderConfig {
    /// Detects a type per entry. Absent by default: detection costs a lookup
    /// per entry and a list view that shows no type column should not pay for
    /// it.
    pub mime: Option<Arc<dyn MimeDetector>>,
    /// The mount table used to tag entries with the device they sit on, and to
    /// notice a read-only medium. Read once per listing, not per entry.
    pub mounts: Option<Arc<MountTable>>,
    /// Follow symlinks to report the target's kind and size. On by default,
    /// because a list that shows every symlink as "unknown" is not useful; the
    /// link status is reported either way.
    pub resolve_symlinks: bool,
}

impl ReaderConfig {
    pub fn new() -> Self {
        Self {
            mime: None,
            mounts: None,
            resolve_symlinks: true,
        }
    }

    pub fn with_mime(mut self, detector: Arc<dyn MimeDetector>) -> Self {
        self.mime = Some(detector);
        self
    }

    pub fn with_mounts(mut self, mounts: Arc<MountTable>) -> Self {
        self.mounts = Some(mounts);
        self
    }

    pub fn resolving_symlinks(mut self, resolve: bool) -> Self {
        self.resolve_symlinks = resolve;
        self
    }
}

impl std::fmt::Debug for ReaderConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReaderConfig")
            .field("mime", &self.mime.is_some())
            .field("mounts", &self.mounts.is_some())
            .field("resolve_symlinks", &self.resolve_symlinks)
            .finish()
    }
}

/// Reads local directories on background threads.
#[derive(Clone, Debug, Default)]
pub struct LocalDirectoryReader {
    config: ReaderConfig,
}

impl LocalDirectoryReader {
    pub fn new() -> Self {
        Self {
            config: ReaderConfig::new(),
        }
    }

    pub fn with_config(config: ReaderConfig) -> Self {
        Self { config }
    }
}

impl DirectoryReader for LocalDirectoryReader {
    /// Spawns a reader thread and returns immediately.
    ///
    /// The thread owns the sink, so if it panics the sink's `Drop` still
    /// reports the listing as cancelled and the view is never left waiting.
    fn start(&self, request: ListingRequest, sink: ListingSink) {
        let config = self.config.clone();
        // A thread that cannot be spawned drops the sink, whose `Drop` reports
        // the listing as cancelled. The view is never left waiting for a batch
        // that is not coming.
        let _ = thread::Builder::new()
            .name("files-listing".to_string())
            .spawn(move || run(&request, &config, sink));
    }
}

/// Reads a directory on the calling thread, for a caller that already has a
/// background thread. This is what the reader thread runs.
pub fn list_directory_blocking(request: &ListingRequest, config: &ReaderConfig, sink: ListingSink) {
    run(request, config, sink);
}

fn run(request: &ListingRequest, config: &ReaderConfig, mut sink: ListingSink) {
    let Some(directory) = request.location.as_local_path() else {
        sink.fail(ListingError::NotListable(request.location.kind()));
        return;
    };
    match list_into(directory, request, config, &mut sink) {
        Ok(Outcome::Complete) => {
            let _ = sink.finish();
        }
        // The sink reports cancellation when it is dropped. Nothing else to
        // send: the consumer already knows it asked for this.
        Ok(Outcome::Cancelled) => {}
        Err(error) => sink.fail(error),
    }
}

enum Outcome {
    Complete,
    Cancelled,
}

fn list_into(
    directory: &LocalPath,
    request: &ListingRequest,
    config: &ReaderConfig,
    sink: &mut ListingSink,
) -> Result<Outcome, ListingError> {
    let path = directory.as_path();
    let rules = read_hidden_rules(path).hiding_backup_files(request.hide_backup_files);
    let device = config
        .mounts
        .as_ref()
        .and_then(|mounts| mounts.owner_of(path));
    let read_only_medium = config
        .mounts
        .as_ref()
        .is_some_and(|mounts| mounts.is_read_only(path));

    let iterator = fs::read_dir(path).map_err(|error| classify(path, &error))?;
    let uid = users_effective_uid();
    let gid = users_effective_gid();

    for item in iterator {
        // Checked before the `stat`, so an abandoned listing pays for at most
        // one more entry's metadata.
        if sink.is_cancelled() {
            return Ok(Outcome::Cancelled);
        }
        let item = match item {
            Ok(item) => item,
            Err(error) => {
                let recorded = classify(path, &error);
                if sink.skip("", recorded).is_err() {
                    return Ok(Outcome::Cancelled);
                }
                continue;
            }
        };
        let name = item.file_name().to_string_lossy().into_owned();
        let entry_path = match directory.join_name(&name) {
            Ok(entry_path) => entry_path,
            Err(_) => {
                // A name the location model refuses is a name no consumer
                // should be handed a path for. It is reported, not listed.
                if sink
                    .skip(
                        name,
                        ListingError::Io {
                            path: path.to_string_lossy().into_owned(),
                            reason: "unrepresentable_name".to_string(),
                        },
                    )
                    .is_err()
                {
                    return Ok(Outcome::Cancelled);
                }
                continue;
            }
        };
        let mut entry = build_entry(&item, name, entry_path, config, uid, gid);
        entry.hidden = rules.classify(&entry.name);
        entry.permissions.read_only_medium = read_only_medium;
        if let EntryBody::File(facts) = &mut entry.body {
            facts.device = device.clone();
        }
        if let Some(detector) = &config.mime
            && entry.kind == EntryKind::File
        {
            entry.mime = detector.detect(&entry.name);
        }
        if sink.push(entry).is_err() {
            return Ok(Outcome::Cancelled);
        }
    }
    // Anything buffered goes out before completion is announced.
    if sink.flush() == Err(Cancelled) {
        return Ok(Outcome::Cancelled);
    }
    Ok(Outcome::Complete)
}

fn build_entry(
    item: &DirEntry,
    name: String,
    path: LocalPath,
    config: &ReaderConfig,
    uid: u32,
    gid: u32,
) -> Entry {
    // `lstat` first: the entry's own identity, before any link is followed.
    let link_metadata = item.metadata();
    let is_symlink = link_metadata
        .as_ref()
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);

    let mut symlink = SymlinkStatus::None;
    let mut effective = link_metadata.as_ref().ok().cloned();

    if is_symlink {
        let target = fs::read_link(path.as_path()).unwrap_or_default();
        if config.resolve_symlinks {
            match fs::metadata(path.as_path()) {
                Ok(resolved) => {
                    symlink = SymlinkStatus::Resolved {
                        target: target.clone(),
                    };
                    effective = Some(resolved);
                }
                Err(error) => {
                    // ELOOP is a symlink cycle; the kernel gave up following
                    // it, which is exactly the answer to report. Anything else
                    // is a link whose target is not there.
                    symlink = if error.raw_os_error() == Some(40) {
                        SymlinkStatus::Loop { target }
                    } else {
                        SymlinkStatus::Broken { target }
                    };
                    effective = None;
                }
            }
        } else {
            symlink = SymlinkStatus::Resolved { target };
        }
    }

    let kind = match &effective {
        Some(metadata) => kind_of(metadata),
        None => EntryKind::Unknown,
    };
    let size = match &effective {
        Some(metadata) if metadata.is_file() => EntrySize::Bytes(metadata.len()),
        // A directory's own size is its directory-entry block, which is not
        // what a user means by the size of a folder. Left unmeasured rather
        // than reported as a number that means something else.
        Some(_) => EntrySize::Unknown,
        None => EntrySize::Unknown,
    };
    let modified = effective
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .map(FileTime::from_system_time);
    let permissions = effective
        .as_ref()
        .map(|metadata| summarize_permissions(metadata, uid, gid))
        .unwrap_or(PermissionsSummary::UNKNOWN);

    Entry {
        name,
        kind,
        size,
        modified,
        permissions,
        hidden: files_core::entry::HiddenState::Visible,
        mime: None,
        body: EntryBody::File(FileFacts {
            path,
            symlink,
            device: None,
        }),
    }
}

fn kind_of(metadata: &Metadata) -> EntryKind {
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_socket()
        || file_type.is_fifo()
        || file_type.is_block_device()
        || file_type.is_char_device()
    {
        EntryKind::Special
    } else {
        EntryKind::Unknown
    }
}

/// Summarizes the mode bits for the effective user.
///
/// Root is reported as able to read and write everything, which is what it
/// is; the GUI does not run as root, so this path exists for correctness
/// rather than for the normal case.
fn summarize_permissions(metadata: &Metadata, uid: u32, gid: u32) -> PermissionsSummary {
    let mode = metadata.permissions().mode();
    let (read, write, execute) = if uid == 0 {
        (true, true, mode & 0o111 != 0)
    } else if metadata.uid() == uid {
        (mode & 0o400 != 0, mode & 0o200 != 0, mode & 0o100 != 0)
    } else if metadata.gid() == gid {
        (mode & 0o040 != 0, mode & 0o020 != 0, mode & 0o010 != 0)
    } else {
        (mode & 0o004 != 0, mode & 0o002 != 0, mode & 0o001 != 0)
    };
    PermissionsSummary {
        readable: read,
        writable: write,
        executable: execute,
        mode: Some(mode & 0o7777),
        read_only_medium: false,
    }
}

fn users_effective_uid() -> u32 {
    // `std` does not expose `geteuid`, and this crate does not link `libc`
    // just to ask. The owner comparison is what matters and the effective uid
    // of a desktop process is its real uid; reading it from the process's own
    // status keeps the dependency list unchanged.
    read_status_id("Uid:").unwrap_or(u32::MAX)
}

fn users_effective_gid() -> u32 {
    read_status_id("Gid:").unwrap_or(u32::MAX)
}

fn read_status_id(field: &str) -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            // real, effective, saved, filesystem. The effective one is second.
            return rest.split_whitespace().nth(1)?.parse().ok();
        }
    }
    None
}

fn classify(path: &Path, error: &io::Error) -> ListingError {
    let path_text = path.to_string_lossy().into_owned();
    match error.kind() {
        io::ErrorKind::PermissionDenied => ListingError::PermissionDenied { path: path_text },
        io::ErrorKind::NotFound => ListingError::NotFound { path: path_text },
        io::ErrorKind::NotADirectory => ListingError::NotADirectory { path: path_text },
        _ => match error.raw_os_error() {
            // ELOOP
            Some(40) => ListingError::SymlinkLoop { path: path_text },
            // ENAMETOOLONG
            Some(36) => ListingError::NameTooLong { path: path_text },
            // ENODEV and ENXIO: the medium went away mid-read. Distinct from a
            // generic failure, because a view says something different about
            // a disk that was unplugged.
            Some(19) | Some(6) => ListingError::DeviceLost { path: path_text },
            _ => ListingError::Io {
                path: path_text,
                reason: error.kind().to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use files_core::listing::{ListingEvent, ListingSession};
    use files_core::location::Location;
    use std::os::unix::fs::symlink;

    fn collect(path: &Path, config: &ReaderConfig) -> (Vec<Entry>, Option<ListingEvent>) {
        let request = ListingRequest::new(Location::local(path).unwrap()).with_batch_size(4);
        let (mut session, sink) = ListingSession::start(&request);
        run(&request, config, sink);
        let mut entries = Vec::new();
        let mut terminal = None;
        for event in session.drain() {
            match event {
                ListingEvent::Batch(batch) => entries.extend(batch.entries),
                other => terminal = Some(other),
            }
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        (entries, terminal)
    }

    #[test]
    fn a_directory_lists_with_typed_metadata() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("notes.txt"), b"hello").unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();

        let (entries, terminal) = collect(root.path(), &ReaderConfig::new());
        assert_eq!(entries.len(), 2);
        let folder = &entries[0];
        assert_eq!(folder.name, "folder");
        assert_eq!(folder.kind, EntryKind::Directory);
        assert_eq!(folder.size, EntrySize::Unknown);
        let file = &entries[1];
        assert_eq!(file.kind, EntryKind::File);
        assert_eq!(file.size, EntrySize::Bytes(5));
        assert!(file.modified.is_some());
        assert!(file.permissions.readable);
        assert!(matches!(terminal, Some(ListingEvent::Complete(_))));
    }

    #[test]
    fn dotfiles_and_hidden_file_names_are_marked_without_being_dropped() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".config"), b"").unwrap();
        fs::write(root.path().join("build"), b"").unwrap();
        fs::write(root.path().join("src"), b"").unwrap();
        fs::write(root.path().join(".hidden"), "build\n").unwrap();

        let (entries, _) = collect(root.path(), &ReaderConfig::new());
        let by_name = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("{name} missing from listing"))
        };
        assert_eq!(
            by_name(".config").hidden,
            files_core::entry::HiddenState::Hidden(files_core::entry::HiddenReason::Dotfile)
        );
        assert_eq!(
            by_name("build").hidden,
            files_core::entry::HiddenState::Hidden(
                files_core::entry::HiddenReason::DirectoryHiddenFile
            )
        );
        assert_eq!(
            by_name("src").hidden,
            files_core::entry::HiddenState::Visible
        );
    }

    #[test]
    fn a_broken_symlink_is_listed_with_its_target_named() {
        let root = tempfile::tempdir().unwrap();
        symlink("/nonexistent-target", root.path().join("dangling")).unwrap();
        let (entries, _) = collect(root.path(), &ReaderConfig::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryKind::Unknown);
        assert!(matches!(entries[0].symlink(), SymlinkStatus::Broken { .. }));
    }

    #[test]
    fn a_symlink_loop_is_reported_as_a_loop_and_the_directory_still_lists() {
        let root = tempfile::tempdir().unwrap();
        symlink(root.path().join("b"), root.path().join("a")).unwrap();
        symlink(root.path().join("a"), root.path().join("b")).unwrap();
        fs::write(root.path().join("real.txt"), b"x").unwrap();

        let (entries, terminal) = collect(root.path(), &ReaderConfig::new());
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[0].symlink(), SymlinkStatus::Loop { .. }));
        assert!(matches!(entries[1].symlink(), SymlinkStatus::Loop { .. }));
        assert_eq!(entries[2].name, "real.txt");
        assert!(matches!(terminal, Some(ListingEvent::Complete(_))));
    }

    #[test]
    fn a_missing_directory_fails_with_a_typed_error() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("absent");
        let (entries, terminal) = collect(&missing, &ReaderConfig::new());
        assert!(entries.is_empty());
        assert!(matches!(
            terminal,
            Some(ListingEvent::Failed {
                error: ListingError::NotFound { .. },
                ..
            })
        ));
    }

    #[test]
    fn a_path_that_is_a_file_is_refused_as_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("plain.txt");
        fs::write(&file, b"x").unwrap();
        let (_, terminal) = collect(&file, &ReaderConfig::new());
        assert!(matches!(
            terminal,
            Some(ListingEvent::Failed {
                error: ListingError::NotADirectory { .. },
                ..
            })
        ));
    }

    #[test]
    fn an_unreadable_directory_reports_permission_denied() {
        let root = tempfile::tempdir().unwrap();
        let locked = root.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let (_, terminal) = collect(&locked, &ReaderConfig::new());
        // Restore before the assertion so the temporary directory can be
        // cleaned up even when the assertion fails.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if users_effective_uid() == 0 {
            // Root reads it anyway, so there is nothing to assert.
            return;
        }
        assert!(matches!(
            terminal,
            Some(ListingEvent::Failed {
                error: ListingError::PermissionDenied { .. },
                ..
            })
        ));
    }

    #[test]
    fn a_name_that_is_not_valid_utf8_is_still_listed() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let root = tempfile::tempdir().unwrap();
        let raw = OsStr::from_bytes(b"broken-\xff-name");
        fs::write(root.path().join(raw), b"x").unwrap();
        let (entries, _) = collect(root.path(), &ReaderConfig::new());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].name.contains("broken-"));
        assert!(entries[0].as_local_path().is_some());
    }

    #[test]
    fn a_very_long_name_is_listed_rather_than_failing_the_directory() {
        let root = tempfile::tempdir().unwrap();
        // 255 bytes is the maximum a single component may be on ext4; longer
        // fails at creation, so the boundary case is the longest legal one.
        let long = "l".repeat(255);
        fs::write(root.path().join(&long), b"x").unwrap();
        let (entries, terminal) = collect(root.path(), &ReaderConfig::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name.len(), 255);
        assert!(matches!(terminal, Some(ListingEvent::Complete(_))));
    }

    #[test]
    fn a_deeply_nested_directory_still_lists() {
        let root = tempfile::tempdir().unwrap();
        // Each component is near the 255-byte maximum, so the accumulated path
        // is several kilobytes — long enough to exercise the long-path handling
        // without needing a path over `PATH_MAX`, which cannot be created
        // through absolute paths at all.
        let component = "d".repeat(200);
        let mut deep = root.path().to_path_buf();
        for _ in 0..15 {
            deep = deep.join(&component);
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("leaf.txt"), b"x").unwrap();
        assert!(deep.as_os_str().len() > 3_000);

        let (entries, terminal) = collect(&deep, &ReaderConfig::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "leaf.txt");
        assert!(matches!(terminal, Some(ListingEvent::Complete(_))));
    }

    #[test]
    fn a_path_longer_than_the_kernel_allows_is_reported_as_such() {
        // Building a real over-length path requires walking into it with
        // relative components, which changes the process's working directory
        // and is not safe to do from a test. What is testable, and what the
        // listing actually depends on, is that the kernel's answer is
        // translated rather than folded into a generic I/O error.
        let error = io::Error::from_raw_os_error(36); // ENAMETOOLONG
        assert!(matches!(
            classify(Path::new("/very/long"), &error),
            ListingError::NameTooLong { .. }
        ));
    }

    #[test]
    fn a_device_that_disappears_mid_listing_is_reported_as_a_lost_device() {
        // A test cannot unplug a disk, so this covers the translation the
        // listing depends on: `ENODEV` and `ENXIO` mean the medium went away,
        // which a view says something different about than a generic failure.
        for raw in [19i32, 6] {
            let error = io::Error::from_raw_os_error(raw);
            assert!(
                matches!(
                    classify(Path::new("/media/user/STICK"), &error),
                    ListingError::DeviceLost { .. }
                ),
                "errno {raw} must be reported as a lost device"
            );
        }
        // A permission error is not a lost device, and neither is a plain
        // failure; the three stay distinguishable.
        assert!(matches!(
            classify(
                Path::new("/media/user/STICK"),
                &io::Error::from(io::ErrorKind::PermissionDenied)
            ),
            ListingError::PermissionDenied { .. }
        ));
        assert!(matches!(
            classify(
                Path::new("/media/user/STICK"),
                &io::Error::from(io::ErrorKind::Other)
            ),
            ListingError::Io { .. }
        ));
    }

    #[test]
    fn two_names_differing_only_in_case_are_two_entries() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Report.txt"), b"a").unwrap();
        fs::write(root.path().join("report.txt"), b"bb").unwrap();
        let (entries, _) = collect(root.path(), &ReaderConfig::new());
        // On a case-insensitive filesystem the second write replaces the
        // first, and this asserts the case that actually occurred rather than
        // assuming the host is case-sensitive.
        if entries.len() == 1 {
            return;
        }
        assert_eq!(entries.len(), 2);
        assert_ne!(entries[0].id(), entries[1].id());
    }

    #[test]
    fn cancelling_stops_the_reader_partway_through_a_large_directory() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..2_000 {
            fs::write(root.path().join(format!("f{index:05}")), b"x").unwrap();
        }
        let request =
            ListingRequest::new(Location::local(root.path()).unwrap()).with_batch_size(16);
        let (mut session, sink) = ListingSession::start(&request);
        let token = sink.token().clone();
        let config = ReaderConfig::new();
        let mut delivered = 0usize;
        // Cancel from the consumer's side once the first batch has landed,
        // which is exactly what navigating away does.
        std::thread::scope(|scope| {
            let reader = scope.spawn(move || run(&request, &config, sink));
            loop {
                let events = session.drain();
                let batched: usize = events
                    .iter()
                    .filter_map(|event| match event {
                        ListingEvent::Batch(batch) => Some(batch.entries.len()),
                        _ => None,
                    })
                    .sum();
                delivered += batched;
                if batched > 0 {
                    token.cancel();
                    break;
                }
                if session.is_complete() {
                    break;
                }
                std::hint::spin_loop();
            }
            reader.join().unwrap();
            delivered += session
                .drain()
                .iter()
                .filter_map(|event| match event {
                    ListingEvent::Batch(batch) => Some(batch.entries.len()),
                    _ => None,
                })
                .sum::<usize>();
        });
        assert!(
            delivered < 2_000,
            "the reader delivered {delivered} entries after cancellation"
        );
    }
}
