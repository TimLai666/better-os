//! Extracting an archive, which is untrusted input.
//!
//! An archive is a list of names and contents somebody else chose. Every one
//! of those names is a path this job is being asked to write, so each is
//! checked before anything is written for it, and an entry that fails a check
//! stops the extraction with an error that names it.
//!
//! ## The rules
//!
//! | Entry | Refused when | Reason key |
//! | --- | --- | --- |
//! | Any | its name starts with `/` | `files.archive.entry.absolute_path` |
//! | Any | a component of its name is `..` | `files.archive.entry.parent_traversal` |
//! | Any | a component is not a usable filename (NUL, over 255 bytes) | `files.archive.entry.unusable_name` |
//! | Any | a directory on its way is a symbolic link | `files.archive.entry.through_link` |
//! | Any | it would replace a directory or a link, or be a link replacing anything | `files.archive.entry.replaces_existing` |
//! | Symbolic link | its target is absolute, or resolves above the extraction folder | `files.archive.entry.link_outside` |
//! | Hard link | its target is absolute, climbs with `..`, or passes through a link | `files.archive.entry.hard_link_outside` |
//! | Hard link | its target is not a regular file this archive already extracted | `files.archive.entry.hard_link_target_missing` |
//!
//! A symbolic link's target is resolved the way the kernel would, against the
//! links already extracted, and a `..` is trusted only when it climbs out of a
//! directory that really exists: after a name that does not exist yet, a later
//! entry could make that name a link and change what the `..` means. Because
//! nothing is ever written through a link and no link or directory is ever
//! replaced, a link that resolved inside when it was checked still does when
//! the extraction ends.
//!
//! Regular files that appear twice are the one replacement allowed: the later
//! one wins, which is what a tar appended to itself means.
//!
//! ## Limits and what is left behind
//!
//! [`crate::policy::ExtractLimits`] bounds the bytes written and the entries
//! read. The bytes are counted as they are written, so an entry whose header
//! understates its size is stopped too; a zip, whose sizes are declared up
//! front, is also refused before anything is written when its declarations
//! already exceed the limit. A zip's entry count and the size of its central
//! directory are read from its end record first ([`crate::zip_end`]), and one
//! declaring more entries than the limit or a directory over
//! [`crate::policy::MAX_ZIP_CENTRAL_DIRECTORY_BYTES`] is refused before the
//! directory is read.
//!
//! Everything is extracted into a temporary directory beside the destination
//! and renamed to the archive's folder only once every entry is in. A refused
//! entry, a limit, a damaged archive, a cancel, or a crash therefore never
//! leaves a half-extracted folder under the real name, and on every path but a
//! crash the temporary directory is removed.
//!
//! Permission bits are carried with the set-user-ID, set-group-ID, and sticky
//! bits dropped: an archive does not get to hand out privilege. Ownership is
//! not carried, for the same reason a copy does not carry it.

use std::cell::Cell;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{ArchiveLimit, OperationError};
use crate::exec::{ItemOutcome, JobControl, Settled, settle_destination};
use crate::fsops;
use crate::log::LogEvent;
use crate::plan::PlanItem;
use crate::policy::{CopyPolicy, ExtractLimits, MAX_ZIP_CENTRAL_DIRECTORY_BYTES};
use crate::spec::{ArchiveFormat, is_usable_name};
use crate::zip_end;

/// The reason keys an [`OperationError::ArchiveEntryRefused`] carries.
pub mod reason {
    pub const ABSOLUTE_PATH: &str = "files.archive.entry.absolute_path";
    pub const PARENT_TRAVERSAL: &str = "files.archive.entry.parent_traversal";
    pub const UNUSABLE_NAME: &str = "files.archive.entry.unusable_name";
    pub const THROUGH_LINK: &str = "files.archive.entry.through_link";
    pub const REPLACES_EXISTING: &str = "files.archive.entry.replaces_existing";
    pub const LINK_OUTSIDE: &str = "files.archive.entry.link_outside";
    pub const HARD_LINK_OUTSIDE: &str = "files.archive.entry.hard_link_outside";
    pub const HARD_LINK_TARGET_MISSING: &str = "files.archive.entry.hard_link_target_missing";
    /// Logged, not refused: a device node, a fifo, or a socket entry, which a
    /// file manager has no business creating.
    pub const SPECIAL_ENTRY_SKIPPED: &str = "files.archive.entry.special_entry_skipped";
}

/// The longest symbolic link target read from a zip entry, which stores the
/// target as the entry's content. `PATH_MAX` is 4096 on Linux, so anything
/// longer could not be created anyway.
const MAX_LINK_TARGET: u64 = 4096;

/// How many links one target may pass through before it is treated as a loop,
/// the kernel's own limit.
const MAX_LINK_HOPS: u32 = 40;

/// Runs an extract item: `item.source` is the archive and `item.destination`
/// the folder it is extracted into.
pub(crate) fn extract_one(
    item: &PlanItem,
    policy: &CopyPolicy,
    control: &mut dyn JobControl,
) -> Result<ItemOutcome, OperationError> {
    let archive = item.source.as_path();
    let Some(requested) = item.destination.clone() else {
        return Ok(ItemOutcome::Failed(OperationError::Io {
            path: archive.to_path_buf(),
            reason: "no_destination".to_string(),
            errno: None,
        }));
    };
    let format = match detect_format(archive) {
        Ok(format) => format,
        Err(error) => return Ok(ItemOutcome::Failed(error)),
    };
    let target = match settle_destination(Some(archive), &requested, false, control)? {
        Settled::Proceed(target) => target,
        Settled::Finished(outcome) => return Ok(outcome),
    };

    let staging = fsops::temporary_name_for(&target);
    if let Err(error) = fs::create_dir(&staging) {
        return Ok(ItemOutcome::Failed(OperationError::from_io(
            &target, &error,
        )));
    }
    let mut guard = StagingGuard {
        path: Some(staging.clone()),
    };
    let file = match File::open(archive) {
        Ok(file) => file,
        Err(error) => {
            return Ok(ItemOutcome::Failed(OperationError::from_io(
                archive, &error,
            )));
        }
    };
    let read = Rc::new(Cell::new(0u64));
    let source = Counting {
        inner: BufReader::new(file),
        read: Rc::clone(&read),
    };
    let mut unpacker = Unpacker {
        archive,
        root: &staging,
        target: &target,
        limits: policy.extract_limits,
        entries: 0,
        written: 0,
        read,
        total: item.bytes,
        chunk: policy.chunk_bytes(),
        since_checkpoint: 0,
        fsync: policy.wants_fsync(),
        buffer: vec![0u8; policy.chunk_bytes().min(1024 * 1024)],
        directories: Vec::new(),
    };
    let unpacked = match format {
        ArchiveFormat::Zip => unpacker.zip(source, control),
        ArchiveFormat::Tar => unpacker.tar(source, control),
        ArchiveFormat::TarGz => unpacker.tar(flate2::bufread::MultiGzDecoder::new(source), control),
        ArchiveFormat::TarZst => match ZstdFrames::new(source) {
            Ok(frames) => unpacker.tar(frames, control),
            Err(error) => Err(unpacker.read_error(&error)),
        },
    };
    match unpacked {
        Ok(()) => {}
        Err(error @ OperationError::Cancelled { .. }) => return Err(error),
        Err(error) => return Ok(ItemOutcome::Failed(error)),
    }

    // The folder appears whole. Something that took its name while the
    // extraction ran is not replaced; `rename(2)` would replace an empty
    // directory silently.
    if fs::symlink_metadata(&target).is_ok() {
        return Ok(ItemOutcome::Failed(OperationError::AlreadyExists {
            path: target,
        }));
    }
    if let Err(error) = fs::rename(&staging, &target) {
        return Ok(ItemOutcome::Failed(OperationError::from_io(
            &target, &error,
        )));
    }
    guard.commit();
    unpacker.finish_directories();
    if policy.wants_fsync()
        && let Err(error) = fsops::sync_directory(target.parent().unwrap_or(Path::new("/")))
    {
        return Ok(ItemOutcome::Failed(error));
    }
    control.log(Some(target.clone()), LogEvent::Created);
    control.item_bytes(item.bytes);
    Ok(ItemOutcome::Done {
        bytes: item.bytes,
        verified: target.is_dir(),
    })
}

/// The format an archive is in, from its first bytes, and from its name only
/// when they say nothing: a tar from before POSIX has no magic number.
pub fn detect_format(archive: &Path) -> Result<ArchiveFormat, OperationError> {
    let mut file = File::open(archive).map_err(|error| OperationError::from_io(archive, &error))?;
    let mut head = [0u8; 265];
    let mut filled = 0;
    while filled < head.len() {
        match file.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(OperationError::from_io(archive, &error)),
        }
    }
    let head = &head[..filled];
    let sniffed = if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        Some(ArchiveFormat::Zip)
    } else if head.starts_with(&[0x1f, 0x8b]) {
        Some(ArchiveFormat::TarGz)
    } else if head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Some(ArchiveFormat::TarZst)
    } else if head.get(257..262) == Some(b"ustar".as_slice()) {
        Some(ArchiveFormat::Tar)
    } else {
        None
    };
    sniffed
        .or_else(|| archive.file_name().and_then(ArchiveFormat::from_file_name))
        .ok_or_else(|| OperationError::ArchiveUnreadable {
            path: archive.to_path_buf(),
            reason: "files.archive.error.unknown_format".to_string(),
        })
}

/// Removes a half-extracted directory unless the extraction committed it.
struct StagingGuard {
    path: Option<PathBuf>,
}

impl StagingGuard {
    fn commit(&mut self) {
        self.path = None;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            // The directory is this job's own temporary, and `remove_dir_all`
            // does not follow the links inside it.
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// The archive being read, counting what it has handed over: the
/// extraction's progress.
struct Counting<R> {
    inner: R,
    read: Rc<Cell<u64>>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.read.set(self.read.get() + read as u64);
        Ok(read)
    }
}

impl<R: BufRead> BufRead for Counting<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.read.set(self.read.get() + amount as u64);
        self.inner.consume(amount);
    }
}

impl<R: Seek> Seek for Counting<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

/// Every zstd frame in a stream, one after another.
///
/// `ruzstd`'s decoder reads one frame. `zstd` itself writes one frame per
/// input, and concatenating two `.zst` files is a valid `.zst` file, so the
/// next frame is started whenever one ends and input remains.
struct ZstdFrames<R: BufRead> {
    current: Option<ruzstd::decoding::StreamingDecoder<R, ruzstd::decoding::FrameDecoder>>,
}

impl<R: BufRead> ZstdFrames<R> {
    fn new(source: R) -> io::Result<Self> {
        let decoder = ruzstd::decoding::StreamingDecoder::new(source).map_err(io::Error::other)?;
        Ok(Self {
            current: Some(decoder),
        })
    }
}

impl<R: BufRead> Read for ZstdFrames<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            let Some(decoder) = self.current.as_mut() else {
                return Ok(0);
            };
            let read = decoder.read(buffer)?;
            if read > 0 || buffer.is_empty() {
                return Ok(read);
            }
            let Some(finished) = self.current.take() else {
                return Ok(0);
            };
            let (mut source, frame) = finished.into_parts();
            if source.fill_buf()?.is_empty() {
                return Ok(0);
            }
            self.current = Some(
                ruzstd::decoding::StreamingDecoder::new_with_decoder(source, frame)
                    .map_err(io::Error::other)?,
            );
        }
    }
}

/// The state of one extraction.
struct Unpacker<'a> {
    archive: &'a Path,
    /// The temporary directory everything is written into.
    root: &'a Path,
    /// The folder `root` becomes, which is what an error names.
    target: &'a Path,
    limits: ExtractLimits,
    entries: u64,
    written: u64,
    /// Archive bytes read so far, and the archive's size: the progress.
    read: Rc<Cell<u64>>,
    total: u64,
    chunk: usize,
    since_checkpoint: usize,
    fsync: bool,
    buffer: Vec<u8>,
    /// Directories with the mode and time they get once their contents are
    /// in, relative to the folder.
    directories: Vec<(PathBuf, Option<u32>, Option<SystemTime>)>,
}

impl Unpacker<'_> {
    fn refused(&self, entry: &[u8], reason: &str) -> OperationError {
        OperationError::ArchiveEntryRefused {
            path: self.archive.to_path_buf(),
            entry: PathBuf::from(OsStr::from_bytes(entry)),
            reason: reason.to_string(),
        }
    }

    /// Reading the archive failed: the disk, which has an errno, or the
    /// format, which does not.
    fn read_error(&self, error: &io::Error) -> OperationError {
        if error.raw_os_error().is_some() {
            OperationError::from_io(self.archive, error)
        } else {
            OperationError::ArchiveUnreadable {
                path: self.archive.to_path_buf(),
                reason: error.to_string(),
            }
        }
    }

    fn zip_error(&self, error: zip::result::ZipError) -> OperationError {
        match error {
            zip::result::ZipError::Io(error) => self.read_error(&error),
            other => OperationError::ArchiveUnreadable {
                path: self.archive.to_path_buf(),
                reason: other.to_string(),
            },
        }
    }

    /// Writing failed. The error names the path in the folder, not in the
    /// temporary directory, which is gone by the time anyone reads it.
    fn write_error(&self, path: &Path, error: &io::Error) -> OperationError {
        let shown = path.strip_prefix(self.root).map_or_else(
            |_| path.to_path_buf(),
            |relative| self.target.join(relative),
        );
        OperationError::from_io(shown, error)
    }

    fn progress(&self, control: &mut dyn JobControl) {
        control.item_bytes(self.read.get().min(self.total));
    }

    /// Counts an entry against the limit, and lets a pause or a cancel in
    /// between entries.
    fn begin_entry(&mut self, control: &mut dyn JobControl) -> Result<(), OperationError> {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(OperationError::ArchiveLimitExceeded {
                path: self.archive.to_path_buf(),
                limit: ArchiveLimit::Entries,
                maximum: self.limits.max_entries,
            });
        }
        self.progress(control);
        control.checkpoint()
    }

    /// An entry's name as components under the folder. `None` is the folder
    /// itself, which a tar names `./`.
    fn components(&self, entry: &[u8]) -> Result<Option<Vec<OsString>>, OperationError> {
        let parts = components(entry).map_err(|reason| self.refused(entry, reason))?;
        Ok((!parts.is_empty()).then_some(parts))
    }

    /// Makes sure every directory above an entry exists and is a real
    /// directory, creating the ones that are missing, and returns the last.
    fn parent(&self, entry: &[u8], parts: &[OsString]) -> Result<PathBuf, OperationError> {
        let mut path = self.root.to_path_buf();
        for part in &parts[..parts.len() - 1] {
            path.push(part);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(self.refused(entry, reason::THROUGH_LINK));
                }
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err(self.refused(entry, reason::REPLACES_EXISTING)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&path).map_err(|error| self.write_error(&path, &error))?;
                }
                Err(error) => return Err(self.write_error(&path, &error)),
            }
        }
        Ok(path)
    }

    /// Where a new non-directory entry goes, once it is known nothing but a
    /// regular file is there. A regular file is removed when `replace_file`,
    /// and refused otherwise.
    fn vacant(
        &self,
        entry: &[u8],
        parts: &[OsString],
        replace_file: bool,
    ) -> Result<PathBuf, OperationError> {
        let mut path = self.parent(entry, parts)?;
        path.push(&parts[parts.len() - 1]);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
            Err(error) => Err(self.write_error(&path, &error)),
            Ok(metadata) if replace_file && metadata.is_file() => {
                fs::remove_file(&path).map_err(|error| self.write_error(&path, &error))?;
                Ok(path)
            }
            Ok(_) => Err(self.refused(entry, reason::REPLACES_EXISTING)),
        }
    }

    fn directory(
        &mut self,
        entry: &[u8],
        mode: Option<u32>,
        modified: Option<SystemTime>,
    ) -> Result<(), OperationError> {
        let Some(parts) = self.components(entry)? else {
            return Ok(());
        };
        let mut path = self.parent(entry, &parts)?;
        path.push(&parts[parts.len() - 1]);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(self.refused(entry, reason::REPLACES_EXISTING)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|error| self.write_error(&path, &error))?;
            }
            Err(error) => return Err(self.write_error(&path, &error)),
        }
        let relative = parts.iter().collect::<PathBuf>();
        self.directories.push((relative, mode, modified));
        Ok(())
    }

    fn file(
        &mut self,
        entry: &[u8],
        mode: Option<u32>,
        modified: Option<SystemTime>,
        content: &mut dyn Read,
        control: &mut dyn JobControl,
    ) -> Result<(), OperationError> {
        let Some(parts) = self.components(entry)? else {
            return Err(self.refused(entry, reason::UNUSABLE_NAME));
        };
        let path = self.vacant(entry, &parts, true)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| self.write_error(&path, &error))?;
        loop {
            let read = content
                .read(&mut self.buffer)
                .map_err(|error| self.read_error(&error))?;
            if read == 0 {
                break;
            }
            if self.written + read as u64 > self.limits.max_bytes {
                return Err(OperationError::ArchiveLimitExceeded {
                    path: self.archive.to_path_buf(),
                    limit: ArchiveLimit::UnpackedBytes,
                    maximum: self.limits.max_bytes,
                });
            }
            file.write_all(&self.buffer[..read])
                .map_err(|error| self.write_error(&path, &error))?;
            self.written += read as u64;
            self.since_checkpoint += read;
            if self.since_checkpoint >= self.chunk {
                self.since_checkpoint = 0;
                self.progress(control);
                control.checkpoint()?;
            }
        }
        // Best effort, as for a copy: a filesystem that refuses a mode or a
        // time still gets the content.
        let _ = file.set_permissions(fs::Permissions::from_mode(
            mode.map_or(0o644, |mode| mode & 0o777),
        ));
        if let Some(modified) = modified {
            let _ = file.set_modified(modified);
        }
        if self.fsync {
            file.sync_all()
                .map_err(|error| self.write_error(&path, &error))?;
        }
        Ok(())
    }

    fn symlink(&mut self, entry: &[u8], target: &[u8]) -> Result<(), OperationError> {
        let Some(parts) = self.components(entry)? else {
            return Err(self.refused(entry, reason::UNUSABLE_NAME));
        };
        if target.is_empty() || target.contains(&0) {
            return Err(self.refused(entry, reason::UNUSABLE_NAME));
        }
        // The directories above the link are created first, so resolving the
        // target sees the real tree.
        let path = self.parent(entry, &parts)?;
        if !resolves_inside(self.root, &parts[..parts.len() - 1], target) {
            return Err(self.refused(entry, reason::LINK_OUTSIDE));
        }
        let path = path.join(&parts[parts.len() - 1]);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(self.write_error(&path, &error)),
            Ok(_) => return Err(self.refused(entry, reason::REPLACES_EXISTING)),
        }
        std::os::unix::fs::symlink(OsStr::from_bytes(target), &path)
            .map_err(|error| self.write_error(&path, &error))
    }

    fn hard_link(&mut self, entry: &[u8], target: &[u8]) -> Result<(), OperationError> {
        let Some(parts) = self.components(entry)? else {
            return Err(self.refused(entry, reason::UNUSABLE_NAME));
        };
        // A hard link's target is another entry's name, relative to the
        // archive's root rather than to the link's own directory.
        let target_parts = match components(target) {
            Ok(target_parts) if !target_parts.is_empty() => target_parts,
            _ => return Err(self.refused(entry, reason::HARD_LINK_OUTSIDE)),
        };
        let mut existing = self.root.to_path_buf();
        for (index, part) in target_parts.iter().enumerate() {
            existing.push(part);
            let last = index + 1 == target_parts.len();
            match fs::symlink_metadata(&existing) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(self.refused(entry, reason::HARD_LINK_OUTSIDE));
                }
                Ok(metadata) if !last && metadata.is_dir() => {}
                Ok(metadata) if last && metadata.is_file() => {}
                _ => return Err(self.refused(entry, reason::HARD_LINK_TARGET_MISSING)),
            }
        }
        let path = self.vacant(entry, &parts, false)?;
        fs::hard_link(&existing, &path).map_err(|error| self.write_error(&path, &error))
    }

    fn skipped(&self, entry: &[u8], control: &mut dyn JobControl) {
        control.log(
            Some(self.target.join(OsStr::from_bytes(entry))),
            LogEvent::Note {
                text: reason::SPECIAL_ENTRY_SKIPPED.to_string(),
            },
        );
    }

    // --- tar --------------------------------------------------------------

    fn tar<R: Read>(
        &mut self,
        reader: R,
        control: &mut dyn JobControl,
    ) -> Result<(), OperationError> {
        let mut archive = tar::Archive::new(reader);
        let entries = archive.entries().map_err(|error| self.read_error(&error))?;
        for entry in entries {
            let mut entry = entry.map_err(|error| self.read_error(&error))?;
            let kind = entry.header().entry_type();
            if kind.is_pax_global_extensions() {
                // Archive-wide metadata, not an entry.
                continue;
            }
            self.begin_entry(control)?;
            let name = entry.path_bytes().into_owned();
            let mode = entry.header().mode().ok();
            let modified = entry
                .header()
                .mtime()
                .ok()
                .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds));
            let link = entry
                .link_name_bytes()
                .map(|target| target.into_owned())
                .unwrap_or_default();
            if kind.is_dir() {
                self.directory(&name, mode, modified)?;
            } else if kind.is_symlink() {
                self.symlink(&name, &link)?;
            } else if kind.is_hard_link() {
                self.hard_link(&name, &link)?;
            } else if kind.is_file() || kind.is_contiguous() || kind.is_gnu_sparse() {
                self.file(&name, mode, modified, &mut entry, control)?;
            } else {
                self.skipped(&name, control);
            }
        }
        Ok(())
    }

    // --- zip --------------------------------------------------------------

    fn zip<R: Read + Seek>(
        &mut self,
        mut reader: R,
        control: &mut dyn JobControl,
    ) -> Result<(), OperationError> {
        // The `zip` crate reads the whole central directory into memory
        // before it can count it, so the count and the directory's size are
        // taken from the end record first, and an archive declaring too much
        // is refused before its directory is read.
        let declared = match zip_end::read_declaration(&mut reader) {
            Ok(Ok(declared)) => declared,
            Ok(Err(reason)) => {
                return Err(OperationError::ArchiveUnreadable {
                    path: self.archive.to_path_buf(),
                    reason: reason.to_string(),
                });
            }
            Err(error) => return Err(self.read_error(&error)),
        };
        if declared.entries > self.limits.max_entries {
            return Err(OperationError::ArchiveLimitExceeded {
                path: self.archive.to_path_buf(),
                limit: ArchiveLimit::Entries,
                maximum: self.limits.max_entries,
            });
        }
        if declared.directory_bytes > MAX_ZIP_CENTRAL_DIRECTORY_BYTES {
            return Err(OperationError::ArchiveLimitExceeded {
                path: self.archive.to_path_buf(),
                limit: ArchiveLimit::CentralDirectoryBytes,
                maximum: MAX_ZIP_CENTRAL_DIRECTORY_BYTES,
            });
        }

        let mut archive = zip::ZipArchive::new(reader).map_err(|error| self.zip_error(error))?;
        // The directory itself is counted again, because the `zip` crate
        // looks for an earlier end record on its own when the last one does
        // not lead to a directory. Its entries' sizes are declared up front
        // too, so an archive that says it unpacks too big is refused before
        // anything is written. What is written is still counted, because a
        // declaration can lie.
        if archive.len() as u64 > self.limits.max_entries {
            return Err(OperationError::ArchiveLimitExceeded {
                path: self.archive.to_path_buf(),
                limit: ArchiveLimit::Entries,
                maximum: self.limits.max_entries,
            });
        }
        let mut declared = 0u64;
        for index in 0..archive.len() {
            let entry = archive
                .by_index_raw(index)
                .map_err(|error| self.zip_error(error))?;
            declared = declared.saturating_add(entry.size());
        }
        if declared > self.limits.max_bytes {
            return Err(OperationError::ArchiveLimitExceeded {
                path: self.archive.to_path_buf(),
                limit: ArchiveLimit::UnpackedBytes,
                maximum: self.limits.max_bytes,
            });
        }

        for index in 0..archive.len() {
            self.begin_entry(control)?;
            let mut entry = archive
                .by_index(index)
                .map_err(|error| self.zip_error(error))?;
            // The decoded name: a zip without the UTF-8 flag spells its names
            // in code page 437, which is what `name` translates.
            let name = entry.name().as_bytes().to_vec();
            let mode = entry.unix_mode();
            let modified = entry
                .last_modified()
                .and_then(crate::archive::unix_time_of)
                .and_then(|seconds| u64::try_from(seconds).ok())
                .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds));
            if entry.is_symlink() {
                let mut target = Vec::new();
                (&mut entry)
                    .take(MAX_LINK_TARGET + 1)
                    .read_to_end(&mut target)
                    .map_err(|error| self.read_error(&error))?;
                if target.len() as u64 > MAX_LINK_TARGET {
                    return Err(self.refused(&name, reason::UNUSABLE_NAME));
                }
                self.symlink(&name, &target)?;
            } else if entry.is_dir() {
                self.directory(&name, mode, modified)?;
            } else {
                self.file(&name, mode, modified, &mut entry, control)?;
            }
        }
        Ok(())
    }

    /// Gives each directory its own mode and time, deepest first, once
    /// nothing more will be written into it. After the rename, so a directory
    /// the archive marks read-only cannot stop the rest of the extraction or
    /// the cleanup of a failed one.
    fn finish_directories(&self) {
        for (relative, mode, modified) in self.directories.iter().rev() {
            let path = self.target.join(relative);
            if let Some(mode) = mode {
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(mode & 0o777));
            }
            if let Some(modified) = modified
                && let Ok(directory) = File::open(&path)
            {
                let _ = directory.set_modified(*modified);
            }
        }
    }
}

/// An entry name split into the components it would create, or the reason it
/// is refused.
fn components(entry: &[u8]) -> Result<Vec<OsString>, &'static str> {
    if entry.first() == Some(&b'/') {
        return Err(reason::ABSOLUTE_PATH);
    }
    let mut parts = Vec::new();
    for part in entry.split(|byte| *byte == b'/') {
        match part {
            b"" | b"." => {}
            b".." => return Err(reason::PARENT_TRAVERSAL),
            name => {
                let name = OsStr::from_bytes(name);
                if !is_usable_name(name) {
                    return Err(reason::UNUSABLE_NAME);
                }
                parts.push(name.to_os_string());
            }
        }
    }
    Ok(parts)
}

/// Whether a link at `root/start/…` whose target is `target` resolves inside
/// `root`, following the links already there the way the kernel would.
///
/// A `..` counts only when it climbs out of a directory that exists now: a
/// name that does not exist yet could still become a link and move it.
fn resolves_inside(root: &Path, start: &[OsString], target: &[u8]) -> bool {
    if target.first() == Some(&b'/') {
        return false;
    }
    let mut stack: Vec<OsString> = start.to_vec();
    let mut pending: VecDeque<Vec<u8>> = target
        .split(|byte| *byte == b'/')
        .map(<[u8]>::to_vec)
        .collect();
    let mut hops = 0;
    while let Some(part) = pending.pop_front() {
        match part.as_slice() {
            b"" | b"." => {}
            b".." => {
                if stack.is_empty() {
                    return false;
                }
                let here: PathBuf = root.join(stack.iter().collect::<PathBuf>());
                match fs::symlink_metadata(&here) {
                    Ok(metadata) if metadata.is_dir() => {
                        stack.pop();
                    }
                    _ => return false,
                }
            }
            name => {
                stack.push(OsString::from_vec(name.to_vec()));
                let here: PathBuf = root.join(stack.iter().collect::<PathBuf>());
                let is_link = fs::symlink_metadata(&here)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink());
                if is_link {
                    hops += 1;
                    if hops > MAX_LINK_HOPS {
                        return false;
                    }
                    let Ok(link) = fs::read_link(&here) else {
                        return false;
                    };
                    let link = link.into_os_string().into_vec();
                    if link.first() == Some(&b'/') {
                        return false;
                    }
                    stack.pop();
                    for component in link.split(|byte| *byte == b'/').rev() {
                        pending.push_front(component.to_vec());
                    }
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_split_into_components_and_the_unsafe_ones_named() {
        assert_eq!(
            components(b"./a//b/./c.txt").unwrap(),
            vec![
                OsString::from("a"),
                OsString::from("b"),
                OsString::from("c.txt")
            ]
        );
        assert!(components(b"./").unwrap().is_empty());
        assert_eq!(components(b"/etc/passwd"), Err(reason::ABSOLUTE_PATH));
        assert_eq!(components(b"a/../../b"), Err(reason::PARENT_TRAVERSAL));
        assert_eq!(
            components(b"a/..hidden"),
            Ok(vec![OsString::from("a"), OsString::from("..hidden")])
        );
        assert_eq!(components(b"a\0b"), Err(reason::UNUSABLE_NAME));
        assert_eq!(components(&[b'x'; 256]), Err(reason::UNUSABLE_NAME));
    }

    #[test]
    fn a_target_is_resolved_against_the_links_already_there() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        fs::create_dir_all(root.join("usr/bin")).unwrap();
        fs::create_dir_all(root.join("usr/lib")).unwrap();
        let bin = [OsString::from("usr"), OsString::from("bin")];
        assert!(resolves_inside(root, &bin, b"../lib/libx.so"));
        assert!(resolves_inside(root, &bin, b"../../usr"));
        assert!(!resolves_inside(root, &bin, b"../../../etc"));
        assert!(!resolves_inside(root, &bin, b"/usr/lib"));
        // A name that does not exist yet cannot be climbed out of.
        assert!(!resolves_inside(root, &bin, b"later/../x"));
        // A link to the root, then `..` through it, is above the root.
        std::os::unix::fs::symlink(".", root.join("here")).unwrap();
        assert!(!resolves_inside(root, &[], b"here/.."));
        assert!(resolves_inside(root, &[], b"here/usr"));
        // A loop is not followed forever.
        std::os::unix::fs::symlink("loop", root.join("loop")).unwrap();
        assert!(!resolves_inside(root, &[], b"loop/x"));
    }

    #[test]
    fn concatenated_zstd_frames_decode_as_one_stream() {
        let mut stream = ruzstd::encoding::compress_to_vec(
            &b"first "[..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        stream.extend(ruzstd::encoding::compress_to_vec(
            &b"second"[..],
            ruzstd::encoding::CompressionLevel::Fastest,
        ));
        let mut decoded = Vec::new();
        ZstdFrames::new(BufReader::new(&stream[..]))
            .unwrap()
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, b"first second");
    }

    #[test]
    fn the_format_is_read_from_the_content_before_the_name() {
        let root = tempfile::tempdir().unwrap();
        let misnamed = root.path().join("really-a-gzip.zip");
        fs::write(&misnamed, [0x1f, 0x8b, 0x08, 0x00]).unwrap();
        assert_eq!(detect_format(&misnamed).unwrap(), ArchiveFormat::TarGz);
        let old_tar = root.path().join("v7.tar");
        fs::write(&old_tar, [0u8; 600]).unwrap();
        assert_eq!(detect_format(&old_tar).unwrap(), ArchiveFormat::Tar);
        let text = root.path().join("notes.txt");
        fs::write(&text, b"hello").unwrap();
        assert_eq!(
            detect_format(&text).unwrap_err().key(),
            "files.operation.error.archive_unreadable"
        );
    }
}
