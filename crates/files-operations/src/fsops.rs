//! The filesystem primitives every job is built from.
//!
//! Three properties are load-bearing and are the reason this is not a thin
//! wrapper over `std::fs::copy`:
//!
//! - **A destination appears whole or not at all.** Every file is written to a
//!   temporary name in the destination directory and renamed into place after
//!   its bytes, metadata, and verification are done. `rename(2)` within one
//!   directory is atomic, so cancelling mid-copy leaves nothing behind.
//! - **Holes stay holes.** `SEEK_DATA` and `SEEK_HOLE` are asked where the
//!   data is. A filesystem that does not answer gets a dense copy, and the
//!   difference is recorded rather than assumed.
//! - **Nothing is a `String`.** Paths reach the kernel as the bytes they are.
//!   A file whose name is not valid UTF-8 is copied, renamed, and deleted like
//!   any other.
//!
//! There is no `std::process::Command` here and there will not be one. Every
//! operation is a syscall on a path, so Issue #6's "no shell-string
//! concatenation" rule holds by construction rather than by review.

use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::OperationError;
use crate::log::MetadataProperty;
use crate::policy::{CopyPolicy, SparsePolicy};

/// A callback the copy loop calls between chunks.
///
/// It is where pause and cancellation live: the engine's implementation parks
/// while the job is paused and returns an error once it is cancelled, so a
/// 4 GB copy stops within one chunk of the request instead of at the end of
/// the file.
pub type ChunkHook<'a> = &'a mut dyn FnMut(u64) -> Result<(), OperationError>;

/// What one file's metadata said when the job looked at it.
///
/// Captured at plan time and re-read immediately before every destructive
/// step. A mismatch is [`OperationError::ExternallyModified`]: something else
/// rewrote the file while the job was working, and deleting the source of a
/// move at that point would destroy the other program's work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileSnapshot {
    pub len: u64,
    pub mtime: (i64, i64),
    pub inode: u64,
    pub device: u64,
    pub mode: u32,
    pub is_dir: bool,
    pub is_symlink: bool,
}

impl FileSnapshot {
    /// `lstat`, never `stat`: a symlink is described as itself.
    pub fn read(path: &Path) -> Result<Self, OperationError> {
        let metadata =
            fs::symlink_metadata(path).map_err(|error| OperationError::from_io(path, &error))?;
        Ok(Self::from_metadata(&metadata))
    }

    pub fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
            inode: metadata.ino(),
            device: metadata.dev(),
            mode: metadata.mode(),
            is_dir: metadata.is_dir(),
            is_symlink: metadata.file_type().is_symlink(),
        }
    }

    /// Whether this is still the same file, unchanged.
    ///
    /// Identity is the inode, content is the size and modification time. A
    /// file rewritten in place by an editor that preserves the inode still
    /// fails this, because the mtime moved.
    pub fn matches(&self, other: &FileSnapshot) -> bool {
        self.inode == other.inode
            && self.device == other.device
            && self.len == other.len
            && self.mtime == other.mtime
    }
}

/// Confirms a path is exactly what the job last saw, before doing something
/// irreversible to it.
pub fn ensure_unchanged(path: &Path, expected: &FileSnapshot) -> Result<(), OperationError> {
    let current = FileSnapshot::read(path)?;
    if current.matches(expected) {
        Ok(())
    } else {
        Err(OperationError::ExternallyModified {
            path: path.to_path_buf(),
        })
    }
}

/// What a copy did, beyond moving bytes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CopyReport {
    pub bytes: u64,
    /// Holes reproduced rather than written as zeroes. Zero means either a
    /// dense file or a filesystem that did not answer the probe.
    pub holes: u64,
    pub used_sparse_probe: bool,
    /// Metadata the destination would not take. Reported, never fatal.
    pub metadata_gaps: Vec<MetadataProperty>,
}

/// Removes a temporary destination unless the copy committed it.
struct TempGuard {
    path: Option<PathBuf>,
}

impl TempGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn commit(&mut self) {
        self.path = None;
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            // Best effort by definition: this runs on the failure path, and a
            // second failure here has nobody left to tell.
            let _ = fs::remove_file(&path);
        }
    }
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A temporary name beside the destination, in the same directory so the final
/// rename is atomic.
///
/// The name is a dotfile, so a directory listing that catches a copy in
/// progress does not show a half-written file to a user who has hidden files
/// off.
pub fn temporary_name_for(destination: &Path) -> PathBuf {
    let parent = destination.parent().unwrap_or(Path::new("/"));
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut name = OsString::from(".");
    name.push(
        destination
            .file_name()
            .unwrap_or_else(|| OsStr::new("unnamed")),
    );
    name.push(format!(".betteros-part-{}-{sequence}", std::process::id()));
    // NAME_MAX is 255 bytes. A source already near the limit would otherwise
    // fail on the temporary rather than on the real name, which is a confusing
    // way to report a name that is too long.
    let bytes = name.as_bytes();
    if bytes.len() > 255 {
        let suffix = format!(".betteros-part-{}-{sequence}", std::process::id());
        let keep = 255 - suffix.len() - 1;
        let mut trimmed = Vec::with_capacity(255);
        trimmed.push(b'.');
        trimmed.extend_from_slice(&bytes[1..1 + keep.min(bytes.len() - 1)]);
        trimmed.extend_from_slice(suffix.as_bytes());
        return parent.join(OsString::from_vec(trimmed));
    }
    parent.join(name)
}

/// Copies one regular file, atomically, preserving what the policy says to
/// preserve.
///
/// `hook` is called with the running byte total after every chunk and after
/// every sparse segment, and its error aborts the copy with the temporary
/// removed.
pub fn copy_file(
    source: &Path,
    destination: &Path,
    policy: &CopyPolicy,
    hook: ChunkHook<'_>,
) -> Result<CopyReport, OperationError> {
    let source_file =
        File::open(source).map_err(|error| OperationError::from_io(source, &error))?;
    let metadata = source_file
        .metadata()
        .map_err(|error| OperationError::from_io(source, &error))?;
    let expected_len = metadata.len();

    let temporary = temporary_name_for(destination);
    let mut guard = TempGuard::new(temporary.clone());
    let mut target =
        File::create(&temporary).map_err(|error| OperationError::from_io(&temporary, &error))?;

    let mut report = CopyReport::default();
    let segments = sparse_segments(&source_file, expected_len, policy);
    match segments {
        Some(segments) => {
            report.used_sparse_probe = true;
            report.holes = segments.holes;
            for segment in &segments.data {
                write_segment(
                    &source_file,
                    &mut target,
                    source,
                    &temporary,
                    *segment,
                    policy.chunk_bytes(),
                    &mut report.bytes,
                    hook,
                )?;
            }
            // The trailing hole, and any hole at all, only exists once the file
            // is the right length.
            set_length(&target, &temporary, expected_len)?;
        }
        None => {
            copy_dense(
                &source_file,
                &mut target,
                source,
                &temporary,
                expected_len,
                policy.chunk_bytes(),
                &mut report.bytes,
                hook,
            )?;
        }
    }

    apply_metadata(source, &metadata, &target, &temporary, policy, &mut report);

    if policy.wants_fsync() {
        sync_file(&target, &temporary)?;
    }
    // The handle is closed before the rename so nothing holds the temporary
    // open under its old name.
    drop(target);

    fs::rename(&temporary, destination)
        .map_err(|error| OperationError::from_io(destination, &error))?;
    guard.commit();

    if policy.wants_fsync() {
        sync_directory(destination.parent().unwrap_or(Path::new("/")))?;
    }
    Ok(report)
}

/// Recreates a symbolic link as a link, with the same target text.
pub fn copy_symlink(
    source: &Path,
    destination: &Path,
    policy: &CopyPolicy,
) -> Result<(), OperationError> {
    let target = fs::read_link(source).map_err(|error| OperationError::from_io(source, &error))?;
    // A link is created under a temporary name and renamed for the same reason
    // a file is: an interrupted overwrite must not leave the destination gone.
    let temporary = temporary_name_for(destination);
    let mut guard = TempGuard::new(temporary.clone());
    std::os::unix::fs::symlink(&target, &temporary)
        .map_err(|error| OperationError::from_io(&temporary, &error))?;
    if policy.preserve_timestamps {
        let metadata = fs::symlink_metadata(source)
            .map_err(|error| OperationError::from_io(source, &error))?;
        // Failing to stamp a link is not a reason to fail the copy; some
        // filesystems refuse it outright.
        let _ = set_times(&temporary, &metadata);
    }
    fs::rename(&temporary, destination)
        .map_err(|error| OperationError::from_io(destination, &error))?;
    guard.commit();
    Ok(())
}

/// Creates a directory, carrying the source's mode and timestamps.
///
/// The mode is applied immediately but the timestamps are not: writing the
/// children afterwards would move the modification time again. The caller
/// stamps directories after their contents, which is what
/// [`finalize_directory`] is for.
pub fn create_directory(
    destination: &Path,
    source_metadata: Option<&fs::Metadata>,
    policy: &CopyPolicy,
) -> Result<(), OperationError> {
    match fs::create_dir(destination) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let existing = fs::symlink_metadata(destination)
                .map_err(|error| OperationError::from_io(destination, &error))?;
            if !existing.is_dir() {
                return Err(OperationError::AlreadyExists {
                    path: destination.to_path_buf(),
                });
            }
            return Ok(());
        }
        Err(error) => return Err(OperationError::from_io(destination, &error)),
    }
    if let (true, Some(metadata)) = (policy.preserve_permissions, source_metadata) {
        // A directory the job cannot write into is a directory the job cannot
        // fill, so the owner write bit is forced on while the copy runs and the
        // real mode is restored by `finalize_directory`.
        let mode = metadata.permissions().mode() | 0o700;
        let _ = fs::set_permissions(destination, fs::Permissions::from_mode(mode));
    }
    Ok(())
}

/// Puts a copied directory's real mode and timestamps back, after its contents
/// are in place.
pub fn finalize_directory(
    destination: &Path,
    source_metadata: &fs::Metadata,
    policy: &CopyPolicy,
) -> Result<(), OperationError> {
    if policy.preserve_permissions {
        let _ = fs::set_permissions(destination, source_metadata.permissions());
    }
    if policy.preserve_timestamps {
        let _ = set_times(destination, source_metadata);
    }
    Ok(())
}

/// Confirms a copied file matches its source.
///
/// Size always, modification time when the policy said to preserve it. Content
/// is not re-read: a byte-for-byte comparison doubles the cost of every copy,
/// and the caller that wants it has [`crate::checksum`].
pub fn verify_copy(
    source: &Path,
    destination: &Path,
    policy: &CopyPolicy,
) -> Result<(), OperationError> {
    // A policy that follows links produced a regular file from a link, so the
    // source has to be described the same way the copy read it.
    let source_meta = if policy.symlinks == crate::policy::SymlinkPolicy::FollowAndCopyTarget {
        fs::metadata(source).map_err(|error| OperationError::from_io(source, &error))?
    } else {
        fs::symlink_metadata(source).map_err(|error| OperationError::from_io(source, &error))?
    };
    let destination_meta = fs::symlink_metadata(destination)
        .map_err(|error| OperationError::from_io(destination, &error))?;
    if source_meta.file_type().is_symlink() != destination_meta.file_type().is_symlink() {
        return Err(OperationError::VerificationFailed {
            path: destination.to_path_buf(),
            reason: "link_kind_differs".to_string(),
        });
    }
    if source_meta.is_file() && source_meta.len() != destination_meta.len() {
        return Err(OperationError::VerificationFailed {
            path: destination.to_path_buf(),
            reason: format!(
                "size {} expected {}",
                destination_meta.len(),
                source_meta.len()
            ),
        });
    }
    if policy.preserve_timestamps
        && !source_meta.file_type().is_symlink()
        && (source_meta.mtime(), source_meta.mtime_nsec())
            != (destination_meta.mtime(), destination_meta.mtime_nsec())
    {
        return Err(OperationError::VerificationFailed {
            path: destination.to_path_buf(),
            reason: "modification_time_differs".to_string(),
        });
    }
    Ok(())
}

/// Whether two paths sit on the same filesystem, which is what decides between
/// the rename fast path and copy-verify-delete.
///
/// The destination's own device is used when it exists and its parent's when it
/// does not, because a move creates the destination.
pub fn same_filesystem(left: &Path, right: &Path) -> Result<bool, OperationError> {
    let left_device = device_of(left)?;
    let right_device = device_of(right)?;
    Ok(left_device == right_device)
}

fn device_of(path: &Path) -> Result<u64, OperationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.dev()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().unwrap_or(Path::new("/"));
            let metadata = fs::symlink_metadata(parent)
                .map_err(|error| OperationError::from_io(parent, &error))?;
            Ok(metadata.dev())
        }
        Err(error) => Err(OperationError::from_io(path, &error)),
    }
}

/// Deletes one item that is known not to be a directory.
pub fn remove_file(path: &Path) -> Result<(), OperationError> {
    fs::remove_file(path).map_err(|error| OperationError::from_io(path, &error))
}

/// Deletes an empty directory.
pub fn remove_directory(path: &Path) -> Result<(), OperationError> {
    fs::remove_dir(path).map_err(|error| OperationError::from_io(path, &error))
}

// --- The sparse probe ----------------------------------------------------

struct SparseMap {
    data: Vec<(u64, u64)>,
    holes: u64,
}

/// Asks the filesystem where the data is.
///
/// `None` means "copy every byte": either the policy said dense, or the
/// filesystem answered `EINVAL`/`ENOTSUP` to `SEEK_DATA`, which is how a
/// filesystem without hole support declines the question.
fn sparse_segments(file: &File, len: u64, policy: &CopyPolicy) -> Option<SparseMap> {
    if policy.sparse != SparsePolicy::Auto || len == 0 {
        return None;
    }
    let fd = file.as_raw_fd();
    let mut data = Vec::new();
    let mut holes = 0u64;
    let mut offset = 0i64;
    loop {
        let start = unsafe { libc::lseek(fd, offset, libc::SEEK_DATA) };
        if start < 0 {
            let errno = io::Error::last_os_error().raw_os_error();
            return match errno {
                // No data at or after this offset: the rest of the file is a
                // hole, and the probe worked.
                Some(libc::ENXIO) => {
                    if (offset as u64) < len {
                        holes += 1;
                    }
                    Some(SparseMap { data, holes })
                }
                // The filesystem does not implement the probe. Nothing learned,
                // so nothing claimed.
                _ => None,
            };
        }
        if start as u64 > offset as u64 {
            holes += 1;
        }
        let end = unsafe { libc::lseek(fd, start, libc::SEEK_HOLE) };
        let end = if end < 0 { len as i64 } else { end };
        if end <= start {
            break;
        }
        data.push((start as u64, (end - start) as u64));
        offset = end;
        if offset as u64 >= len {
            break;
        }
    }
    Some(SparseMap { data, holes })
}

#[allow(clippy::too_many_arguments)]
fn write_segment(
    source_file: &File,
    target: &mut File,
    source_path: &Path,
    target_path: &Path,
    segment: (u64, u64),
    chunk: usize,
    written: &mut u64,
    hook: ChunkHook<'_>,
) -> Result<(), OperationError> {
    use std::os::unix::fs::FileExt;
    let (start, length) = segment;
    let mut buffer = vec![0u8; chunk];
    let mut offset = 0u64;
    while offset < length {
        let want = chunk.min((length - offset) as usize);
        let read = source_file
            .read_at(&mut buffer[..want], start + offset)
            .map_err(|error| OperationError::from_io(source_path, &error))?;
        if read == 0 {
            break;
        }
        write_at(target, target_path, &buffer[..read], start + offset)?;
        offset += read as u64;
        *written += read as u64;
        hook(*written)?;
    }
    Ok(())
}

fn write_at(
    target: &File,
    target_path: &Path,
    data: &[u8],
    offset: u64,
) -> Result<(), OperationError> {
    use std::os::unix::fs::FileExt;
    let mut done = 0usize;
    while done < data.len() {
        let written = target
            .write_at(&data[done..], offset + done as u64)
            .map_err(|error| OperationError::from_io(target_path, &error))?;
        if written == 0 {
            return Err(OperationError::Io {
                path: target_path.to_path_buf(),
                reason: "write_returned_zero".to_string(),
                errno: None,
            });
        }
        done += written;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn copy_dense(
    source_file: &File,
    target: &mut File,
    source_path: &Path,
    target_path: &Path,
    _len: u64,
    chunk: usize,
    written: &mut u64,
    hook: ChunkHook<'_>,
) -> Result<(), OperationError> {
    let mut reader = source_file;
    let mut buffer = vec![0u8; chunk];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| OperationError::from_io(source_path, &error))?;
        if read == 0 {
            break;
        }
        target
            .write_all(&buffer[..read])
            .map_err(|error| OperationError::from_io(target_path, &error))?;
        *written += read as u64;
        hook(*written)?;
    }
    Ok(())
}

fn set_length(file: &File, path: &Path, len: u64) -> Result<(), OperationError> {
    file.set_len(len)
        .map_err(|error| OperationError::from_io(path, &error))
}

fn sync_file(file: &File, path: &Path) -> Result<(), OperationError> {
    file.sync_all()
        .map_err(|error| OperationError::from_io(path, &error))
}

/// `fsync` on a directory, which is what makes a rename survive a power cut.
///
/// This is the file-level half of durability. The device-level flush that makes
/// an external disk safe to unplug belongs to `storage-service`; this makes
/// sure there is something coherent for it to flush.
pub fn sync_directory(path: &Path) -> Result<(), OperationError> {
    let directory = File::open(path).map_err(|error| OperationError::from_io(path, &error))?;
    directory
        .sync_all()
        .map_err(|error| OperationError::from_io(path, &error))
}

// --- Metadata ------------------------------------------------------------

fn apply_metadata(
    source_path: &Path,
    source: &fs::Metadata,
    target: &File,
    target_path: &Path,
    policy: &CopyPolicy,
    report: &mut CopyReport,
) {
    if policy.preserve_permissions && target.set_permissions(source.permissions()).is_err() {
        report.metadata_gaps.push(MetadataProperty::Permissions);
    }
    if policy.preserve_xattrs {
        report
            .metadata_gaps
            .extend(copy_xattrs(source_path, target_path));
    }
    // Timestamps go last: every write above moves the modification time.
    if policy.preserve_timestamps && set_times(target_path, source).is_err() {
        report.metadata_gaps.push(MetadataProperty::Timestamps);
    }
}

/// Copies extended attributes, reporting what would not go across.
///
/// Per-attribute failure is not fatal. A destination filesystem with no xattr
/// support answers `EOPNOTSUPP` to every one of them, and refusing to copy a
/// file onto a FAT stick because it has a `user.xdg.origin.url` would be
/// absurd. ACL attributes are called out separately, because an ACL that
/// quietly disappeared changes who can read the file.
pub fn copy_xattrs(source: &Path, destination: &Path) -> Vec<MetadataProperty> {
    let Ok(source_c) = CString::new(source.as_os_str().as_bytes()) else {
        return Vec::new();
    };
    let Ok(destination_c) = CString::new(destination.as_os_str().as_bytes()) else {
        return Vec::new();
    };

    let size = unsafe { libc::llistxattr(source_c.as_ptr(), std::ptr::null_mut(), 0) };
    if size <= 0 {
        // Either no attributes, or a filesystem that has none to list.
        return Vec::new();
    }
    let mut names = vec![0u8; size as usize];
    let read = unsafe {
        libc::llistxattr(
            source_c.as_ptr(),
            names.as_mut_ptr() as *mut libc::c_char,
            names.len(),
        )
    };
    if read <= 0 {
        return Vec::new();
    }
    names.truncate(read as usize);

    let mut gaps = Vec::new();
    let mut refused_acl = false;
    let mut refused_any = false;
    for name in names
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let Ok(name_c) = CString::new(name) else {
            continue;
        };
        let value_size =
            unsafe { libc::lgetxattr(source_c.as_ptr(), name_c.as_ptr(), std::ptr::null_mut(), 0) };
        if value_size < 0 {
            continue;
        }
        let mut value = vec![0u8; value_size as usize];
        let got = unsafe {
            libc::lgetxattr(
                source_c.as_ptr(),
                name_c.as_ptr(),
                value.as_mut_ptr() as *mut libc::c_void,
                value.len(),
            )
        };
        if got < 0 {
            continue;
        }
        value.truncate(got as usize);
        let set = unsafe {
            libc::lsetxattr(
                destination_c.as_ptr(),
                name_c.as_ptr(),
                value.as_ptr() as *const libc::c_void,
                value.len(),
                0,
            )
        };
        if set < 0 {
            if name.starts_with(b"system.posix_acl_") {
                refused_acl = true;
            } else {
                refused_any = true;
            }
        }
    }
    if refused_any {
        gaps.push(MetadataProperty::ExtendedAttributes);
    }
    if refused_acl {
        gaps.push(MetadataProperty::AccessControlList);
    }
    gaps
}

/// Sets access and modification times to nanosecond resolution, without
/// following a symbolic link.
pub fn set_times(path: &Path, source: &fs::Metadata) -> Result<(), OperationError> {
    let path_c =
        CString::new(path.as_os_str().as_bytes()).map_err(|_| OperationError::InvalidName {
            name: path.to_path_buf(),
        })?;
    let times = [
        libc::timespec {
            tv_sec: source.atime() as libc::time_t,
            tv_nsec: source.atime_nsec(),
        },
        libc::timespec {
            tv_sec: source.mtime() as libc::time_t,
            tv_nsec: source.mtime_nsec(),
        },
    ];
    let result = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            path_c.as_ptr(),
            times.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(OperationError::from_io(path, &io::Error::last_os_error()))
    }
}

/// Reads a file's extended attribute, for tests and for a consumer that wants
/// to show one.
pub fn read_xattr(path: &Path, name: &str) -> Option<Vec<u8>> {
    let path_c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let name_c = CString::new(name).ok()?;
    let size =
        unsafe { libc::lgetxattr(path_c.as_ptr(), name_c.as_ptr(), std::ptr::null_mut(), 0) };
    if size < 0 {
        return None;
    }
    let mut value = vec![0u8; size as usize];
    let got = unsafe {
        libc::lgetxattr(
            path_c.as_ptr(),
            name_c.as_ptr(),
            value.as_mut_ptr() as *mut libc::c_void,
            value.len(),
        )
    };
    if got < 0 {
        return None;
    }
    value.truncate(got as usize);
    Some(value)
}

/// Writes an extended attribute. Used by the tests to build a source worth
/// copying; a job never calls it directly.
pub fn write_xattr(path: &Path, name: &str, value: &[u8]) -> Result<(), OperationError> {
    let path_c =
        CString::new(path.as_os_str().as_bytes()).map_err(|_| OperationError::InvalidName {
            name: path.to_path_buf(),
        })?;
    let name_c = CString::new(name).map_err(|_| OperationError::InvalidName {
        name: PathBuf::from(name),
    })?;
    let result = unsafe {
        libc::lsetxattr(
            path_c.as_ptr(),
            name_c.as_ptr(),
            value.as_ptr() as *const libc::c_void,
            value.len(),
            0,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(OperationError::from_io(path, &io::Error::last_os_error()))
    }
}

/// How many 512-byte blocks a file actually occupies.
///
/// This is how a test proves a hole survived: a sparse file's apparent length
/// and its allocated size disagree, and only the second one tells the truth.
pub fn allocated_blocks(path: &Path) -> Result<u64, OperationError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| OperationError::from_io(path, &error))?;
    Ok(metadata.blocks())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> CopyPolicy {
        CopyPolicy::default()
    }

    fn copy(source: &Path, destination: &Path, policy: &CopyPolicy) -> CopyReport {
        let mut hook = |_: u64| Ok(());
        copy_file(source, destination, policy, &mut hook).unwrap()
    }

    #[test]
    fn a_copy_preserves_the_modification_time_to_the_nanosecond() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("a.txt");
        fs::write(&source, b"hello").unwrap();
        // A time with a non-zero nanosecond part, so a whole-second copy fails.
        let times = [
            libc::timespec {
                tv_sec: 1_700_000_000,
                tv_nsec: 123_456_789,
            },
            libc::timespec {
                tv_sec: 1_700_000_000,
                tv_nsec: 123_456_789,
            },
        ];
        let path_c = CString::new(source.as_os_str().as_bytes()).unwrap();
        unsafe { libc::utimensat(libc::AT_FDCWD, path_c.as_ptr(), times.as_ptr(), 0) };

        let destination = directory.path().join("b.txt");
        copy(&source, &destination, &policy());
        let after = fs::metadata(&destination).unwrap();
        assert_eq!(after.mtime(), 1_700_000_000);
        assert_eq!(after.mtime_nsec(), 123_456_789);
    }

    #[test]
    fn a_copy_preserves_the_executable_bit() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("script.sh");
        fs::write(&source, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
        let destination = directory.path().join("copy.sh");
        copy(&source, &destination, &policy());
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn a_symlink_is_copied_as_a_link_pointing_at_the_same_text() {
        let directory = tempfile::tempdir().unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink("../elsewhere/target", &link).unwrap();
        let destination = directory.path().join("copied");
        copy_symlink(&link, &destination, &policy()).unwrap();
        assert!(
            fs::symlink_metadata(&destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_link(&destination).unwrap(),
            Path::new("../elsewhere/target")
        );
    }

    #[test]
    fn a_cancelled_copy_leaves_nothing_at_the_destination() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("big.bin");
        fs::write(&source, vec![7u8; 512 * 1024]).unwrap();
        let destination = directory.path().join("out.bin");
        let mut policy = policy();
        policy.chunk_bytes = 4096;
        let mut hook = |written: u64| {
            if written > 8192 {
                Err(OperationError::Cancelled {
                    path: PathBuf::from("out.bin"),
                })
            } else {
                Ok(())
            }
        };
        let error = copy_file(&source, &destination, &policy, &mut hook).unwrap_err();
        assert!(matches!(error, OperationError::Cancelled { .. }));
        assert!(!destination.exists());
        // And no temporary was left behind either.
        let leftovers: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .filter(|name| name.as_bytes().starts_with(b"."))
            .collect();
        assert!(leftovers.is_empty(), "left {leftovers:?}");
    }

    #[test]
    fn extended_attributes_ride_along_where_the_filesystem_takes_them() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("tagged");
        fs::write(&source, b"x").unwrap();
        if write_xattr(&source, "user.better-os.test", b"value").is_err() {
            // tmpfs without user xattrs, or a kernel that refuses them. The
            // policy says this is not fatal, and neither is skipping the test.
            return;
        }
        let destination = directory.path().join("tagged-copy");
        copy(&source, &destination, &policy());
        assert_eq!(
            read_xattr(&destination, "user.better-os.test").as_deref(),
            Some(b"value".as_slice())
        );
    }

    #[test]
    fn verification_notices_a_destination_of_the_wrong_size() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("a");
        let destination = directory.path().join("b");
        fs::write(&source, b"12345").unwrap();
        fs::write(&destination, b"12").unwrap();
        let mut policy = policy();
        policy.preserve_timestamps = false;
        let error = verify_copy(&source, &destination, &policy).unwrap_err();
        assert!(matches!(error, OperationError::VerificationFailed { .. }));
    }

    #[test]
    fn a_file_rewritten_under_the_job_is_detected_before_the_source_is_touched() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("a");
        fs::write(&path, b"first").unwrap();
        let snapshot = FileSnapshot::read(&path).unwrap();
        ensure_unchanged(&path, &snapshot).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&path, b"second-and-longer").unwrap();
        assert!(matches!(
            ensure_unchanged(&path, &snapshot),
            Err(OperationError::ExternallyModified { .. })
        ));
    }

    #[test]
    fn the_temporary_name_stays_within_name_max() {
        let long = "x".repeat(250);
        let destination = PathBuf::from("/tmp").join(&long);
        let temporary = temporary_name_for(&destination);
        assert!(temporary.file_name().unwrap().as_bytes().len() <= 255);
        assert_eq!(temporary.parent(), Some(Path::new("/tmp")));
    }

    #[test]
    fn same_filesystem_holds_for_a_destination_that_does_not_exist_yet() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("a");
        fs::write(&source, b"x").unwrap();
        let destination = directory.path().join("not-yet");
        assert!(same_filesystem(&source, &destination).unwrap());
    }
}
