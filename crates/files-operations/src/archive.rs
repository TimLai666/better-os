//! Writing an archive.
//!
//! An archive job is one item: the archive file. The sources are walked again
//! when the item runs, with the same walker a copy uses, and every file,
//! directory, and symbolic link goes into one stream. The progress total is
//! the bytes of file content the walk counted at planning, and pause and
//! cancel land between chunks of the file being read, the way they do for a
//! copy.
//!
//! The archive is written under a temporary name beside its destination and
//! renamed into place only when the stream is finished, which is the same
//! promise a copied file makes: a cancelled, failed, or interrupted archive
//! leaves nothing under the real name, and the temporary is removed on every
//! exit path.
//!
//! What goes in, per format:
//!
//! | | zip | tar, tar.gz, tar.zst |
//! | --- | --- | --- |
//! | Names | UTF-8 only; any other name refuses the job naming the entry | bytes, as the filesystem has them |
//! | Content | deflate | stored; the whole stream is gzip or zstd |
//! | Permission bits | yes | yes |
//! | Modification time | local time, two-second resolution, 1980 to 2107 | seconds |
//! | Symbolic links | as links | as links |
//! | Hard links | each path stored as its own file | each path stored as its own file |
//! | Sockets, fifos, devices | left out, logged | left out, logged |

use std::cell::Cell;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use files_core::location::LocalPath;

use crate::error::OperationError;
use crate::exec::{ItemOutcome, JobControl, Settled, settle_destination};
use crate::fsops::{self, TempGuard};
use crate::log::LogEvent;
use crate::plan::{ItemKind, Plan, PlanItem, WalkOrder, walk_source};
use crate::policy::CopyPolicy;
use crate::spec::ArchiveFormat;

/// A special file left out of an archive, logged against its path.
pub const SPECIAL_FILE_SKIPPED: &str = "files.archive.entry.special_file_skipped";

/// Runs an archive item: `item.destination` is the archive to create.
pub(crate) fn archive_one(
    item: &PlanItem,
    sources: &[LocalPath],
    format: ArchiveFormat,
    policy: &CopyPolicy,
    control: &mut dyn JobControl,
) -> Result<ItemOutcome, OperationError> {
    let requested = item
        .destination
        .clone()
        .unwrap_or_else(|| item.source.clone());
    let destination = match settle_destination(None, &requested, true, control)? {
        Settled::Proceed(destination) => destination,
        Settled::Finished(outcome) => return Ok(outcome),
    };

    // The walk that names what goes in. Each source is stored under its own
    // name, so the walker's destination paths are the archive's entry names.
    let mut plan = Plan::default();
    for source in sources {
        let name = source
            .as_path()
            .file_name()
            .map_or_else(|| PathBuf::from("unnamed"), PathBuf::from);
        walk_source(
            source.as_path(),
            Some(&name),
            WalkOrder::Prologue,
            policy,
            &mut plan,
        );
    }
    if let Some((_, error)) = plan.unreadable.into_iter().next() {
        return Ok(ItemOutcome::Failed(error));
    }

    let temporary = fsops::temporary_name_for(&destination);
    let mut guard = TempGuard::new(temporary.clone());
    let file = match File::create(&temporary) {
        Ok(file) => file,
        Err(error) => {
            return Ok(ItemOutcome::Failed(OperationError::from_io(
                &temporary, &error,
            )));
        }
    };

    let mut writer = EntryWriter {
        archive: &destination,
        temporary: &temporary,
        policy,
        done: 0,
        since_checkpoint: 0,
    };
    let written = match format {
        ArchiveFormat::Zip => writer.zip(file, &plan.items, control),
        ArchiveFormat::Tar => writer
            .tar(BufWriter::new(file), &plan.items, control)
            .and_then(|buffered| {
                buffered
                    .into_inner()
                    .map_err(|error| OperationError::from_io(&temporary, error.error()))
            }),
        ArchiveFormat::TarGz => {
            let encoder =
                flate2::write::GzEncoder::new(BufWriter::new(file), flate2::Compression::default());
            writer
                .tar(encoder, &plan.items, control)
                .and_then(|encoder| {
                    encoder
                        .finish()
                        .map_err(|error| OperationError::from_io(&temporary, &error))
                })
                .and_then(|buffered| {
                    buffered
                        .into_inner()
                        .map_err(|error| OperationError::from_io(&temporary, error.error()))
                })
        }
        ArchiveFormat::TarZst => writer.tar_zst(file, &plan.items, control),
    };
    let file = match written {
        Ok(file) => file,
        Err(error @ OperationError::Cancelled { .. }) => return Err(error),
        Err(error) => return Ok(ItemOutcome::Failed(error)),
    };

    if policy.wants_fsync()
        && let Err(error) = file.sync_all()
    {
        return Ok(ItemOutcome::Failed(OperationError::from_io(
            &temporary, &error,
        )));
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, &destination) {
        return Ok(ItemOutcome::Failed(OperationError::from_io(
            &destination,
            &error,
        )));
    }
    guard.commit();
    if policy.wants_fsync()
        && let Err(error) = fsops::sync_directory(destination.parent().unwrap_or(Path::new("/")))
    {
        return Ok(ItemOutcome::Failed(error));
    }
    control.log(Some(destination.clone()), LogEvent::Created);
    Ok(ItemOutcome::Done {
        bytes: writer.done,
        verified: destination.is_file(),
    })
}

/// The state one archive's entries are written with.
struct EntryWriter<'a> {
    archive: &'a Path,
    temporary: &'a Path,
    policy: &'a CopyPolicy,
    /// Bytes of file content read so far: the item's progress.
    done: u64,
    since_checkpoint: usize,
}

impl EntryWriter<'_> {
    fn write_error(&self, error: &io::Error) -> OperationError {
        OperationError::from_io(self.temporary, error)
    }

    fn refused(&self, entry: &Path, reason: &str) -> OperationError {
        OperationError::ArchiveEntryRefused {
            path: self.archive.to_path_buf(),
            entry: entry.to_path_buf(),
            reason: reason.to_string(),
        }
    }

    /// Reports progress and honours a pause or a cancel, once per chunk.
    fn advance(&mut self, read: usize, control: &mut dyn JobControl) -> Result<(), OperationError> {
        self.done += read as u64;
        self.since_checkpoint += read;
        if self.since_checkpoint >= self.policy.chunk_bytes() {
            self.since_checkpoint = 0;
            control.item_bytes(self.done);
            control.checkpoint()?;
        }
        Ok(())
    }

    /// Opens a source file for reading, with its metadata.
    fn open(&self, source: &Path) -> Result<(File, fs::Metadata), OperationError> {
        let file = File::open(source).map_err(|error| OperationError::from_io(source, &error))?;
        let metadata = file
            .metadata()
            .map_err(|error| OperationError::from_io(source, &error))?;
        Ok((file, metadata))
    }

    /// A file read to the length it had when it was opened, and not a byte
    /// more or less: an archive header states the size before the content, so
    /// a file that grew or shrank meanwhile would corrupt the stream. The
    /// plan's snapshot is checked afterwards, so a rewrite is reported as what
    /// it is.
    fn finished_reading(
        &self,
        item: &PlanItem,
        expected: u64,
        read: u64,
    ) -> Result<(), OperationError> {
        if read != expected {
            return Err(OperationError::ExternallyModified {
                path: item.source.clone(),
            });
        }
        if let Some(snapshot) = &item.snapshot
            && !snapshot.is_symlink
        {
            fsops::ensure_unchanged(&item.source, snapshot)?;
        }
        Ok(())
    }

    // --- zip --------------------------------------------------------------

    fn zip(
        &mut self,
        file: File,
        items: &[PlanItem],
        control: &mut dyn JobControl,
    ) -> Result<File, OperationError> {
        use zip::write::SimpleFileOptions;

        let mut zip = zip::ZipWriter::new(BufWriter::new(file)).set_auto_large_file();
        let temporary = self.temporary;
        let zip_error = |error: zip::result::ZipError| -> OperationError {
            match error {
                zip::result::ZipError::Io(error) => OperationError::from_io(temporary, &error),
                other => OperationError::Io {
                    path: temporary.to_path_buf(),
                    reason: other.to_string(),
                    errno: None,
                },
            }
        };
        let mut buffer = vec![0u8; self.policy.chunk_bytes().min(1024 * 1024)];
        for item in items {
            control.checkpoint()?;
            let Some(entry) = item.destination.as_deref() else {
                continue;
            };
            let Some(name) = entry.to_str() else {
                if item.kind == ItemKind::DirectoryEpilogue {
                    continue;
                }
                return Err(self.refused(entry, "files.archive.entry.name_not_utf8"));
            };
            match item.kind {
                ItemKind::DirectoryEpilogue => {}
                ItemKind::Other => {
                    control.log(
                        Some(item.source.clone()),
                        LogEvent::Note {
                            text: SPECIAL_FILE_SKIPPED.to_string(),
                        },
                    );
                }
                ItemKind::Directory => {
                    let metadata = fs::metadata(&item.source)
                        .map_err(|error| OperationError::from_io(&item.source, &error))?;
                    let options = SimpleFileOptions::default()
                        .unix_permissions(metadata.mode())
                        .last_modified_time(dos_time(metadata.mtime()));
                    zip.add_directory(name, options).map_err(&zip_error)?;
                }
                ItemKind::Symlink => {
                    let target = fs::read_link(&item.source)
                        .map_err(|error| OperationError::from_io(&item.source, &error))?;
                    let Some(target) = target.to_str() else {
                        return Err(self.refused(entry, "files.archive.entry.name_not_utf8"));
                    };
                    let metadata = fs::symlink_metadata(&item.source)
                        .map_err(|error| OperationError::from_io(&item.source, &error))?;
                    let options =
                        SimpleFileOptions::default().last_modified_time(dos_time(metadata.mtime()));
                    zip.add_symlink(name, target, options).map_err(&zip_error)?;
                }
                ItemKind::File => {
                    let (mut source, metadata) = self.open(&item.source)?;
                    let options = SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated)
                        .unix_permissions(metadata.mode())
                        .last_modified_time(dos_time(metadata.mtime()));
                    zip.start_file(name, options).map_err(&zip_error)?;
                    let expected = metadata.len();
                    let mut read_total = 0u64;
                    let mut limited = (&mut source).take(expected);
                    loop {
                        let read = limited
                            .read(&mut buffer)
                            .map_err(|error| OperationError::from_io(&item.source, &error))?;
                        if read == 0 {
                            break;
                        }
                        zip.write_all(&buffer[..read])
                            .map_err(|error| self.write_error(&error))?;
                        read_total += read as u64;
                        self.advance(read, control)?;
                    }
                    self.finished_reading(item, expected, read_total)?;
                }
            }
        }
        let buffered = zip.finish().map_err(&zip_error)?;
        buffered
            .into_inner()
            .map_err(|error| self.write_error(error.error()))
    }

    // --- tar --------------------------------------------------------------

    /// Writes every entry as a tar stream into `writer`, and hands the writer
    /// back finished, so the caller can close whatever compression wraps it.
    fn tar<W: Write>(
        &mut self,
        writer: W,
        items: &[PlanItem],
        control: &mut dyn JobControl,
    ) -> Result<W, OperationError> {
        let mut builder = tar::Builder::new(writer);
        builder.follow_symlinks(false);
        for item in items {
            control.checkpoint()?;
            let Some(entry) = item.destination.as_deref() else {
                continue;
            };
            match item.kind {
                ItemKind::DirectoryEpilogue => {}
                ItemKind::Other => {
                    control.log(
                        Some(item.source.clone()),
                        LogEvent::Note {
                            text: SPECIAL_FILE_SKIPPED.to_string(),
                        },
                    );
                }
                ItemKind::Directory => {
                    let metadata = fs::metadata(&item.source)
                        .map_err(|error| OperationError::from_io(&item.source, &error))?;
                    let mut header = tar_header(&metadata);
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                    builder
                        .append_data(&mut header, entry, io::empty())
                        .map_err(|error| self.tar_error(entry, &error))?;
                }
                ItemKind::Symlink => {
                    let target = fs::read_link(&item.source)
                        .map_err(|error| OperationError::from_io(&item.source, &error))?;
                    let metadata = fs::symlink_metadata(&item.source)
                        .map_err(|error| OperationError::from_io(&item.source, &error))?;
                    let mut header = tar_header(&metadata);
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_size(0);
                    builder
                        .append_link(&mut header, entry, &target)
                        .map_err(|error| self.tar_error(entry, &error))?;
                }
                ItemKind::File => {
                    let (source, metadata) = self.open(&item.source)?;
                    let mut header = tar_header(&metadata);
                    header.set_entry_type(tar::EntryType::Regular);
                    let expected = metadata.len();
                    header.set_size(expected);
                    let failure = Rc::new(Cell::new(None));
                    let read = Rc::new(Cell::new(0u64));
                    let reader = ProgressReader {
                        inner: source.take(expected),
                        writer: self,
                        control: &mut *control,
                        path: &item.source,
                        read: Rc::clone(&read),
                        failure: Rc::clone(&failure),
                    };
                    let appended = builder.append_data(&mut header, entry, reader);
                    if let Some(error) = failure.take() {
                        return Err(error);
                    }
                    appended.map_err(|error| self.tar_error(entry, &error))?;
                    self.finished_reading(item, expected, read.get())?;
                }
            }
        }
        builder
            .into_inner()
            .map_err(|error| self.write_error(&error))
    }

    /// A tar error is either the output failing, which carries an errno, or
    /// the tar crate refusing a name it cannot encode.
    fn tar_error(&self, entry: &Path, error: &io::Error) -> OperationError {
        if error.raw_os_error().is_some() {
            self.write_error(error)
        } else {
            self.refused(entry, "files.archive.entry.unusable_name")
        }
    }

    /// A tar stream compressed with zstd.
    ///
    /// `ruzstd`'s compressor pulls its input from a reader until the reader
    /// ends, while the tar builder pushes into a writer, so the compressor
    /// runs on a thread of its own and the two are joined by a bounded
    /// channel. The compressor cannot report an I/O error — it unwraps them —
    /// so it is given a reader that never fails and an output that never fails
    /// either: the output keeps the first real error, raises a flag the tar
    /// side sees on its next write, and discards the rest.
    fn tar_zst(
        &mut self,
        file: File,
        items: &[PlanItem],
        control: &mut dyn JobControl,
    ) -> Result<File, OperationError> {
        let failed = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            let (sender, receiver) = sync_channel::<Vec<u8>>(4);
            let output = KeepFirstError {
                inner: BufWriter::new(file),
                error: None,
                failed: Arc::clone(&failed),
            };
            let compressor = std::thread::Builder::new()
                .name("files-archive-zstd".to_string())
                .spawn_scoped(scope, move || {
                    let mut output = output;
                    let mut encoder = ruzstd::encoding::FrameCompressor::new(
                        ruzstd::encoding::CompressionLevel::Fastest,
                    );
                    encoder.set_source(ChannelReader {
                        receiver,
                        current: Vec::new(),
                        position: 0,
                    });
                    encoder.set_drain(&mut output);
                    encoder.compress();
                    drop(encoder);
                    output.finish()
                })
                .map_err(|error| self.write_error(&error))?;

            let channel = ChannelWriter {
                sender: Some(sender),
                buffer: Vec::with_capacity(CHANNEL_BLOCK),
                failed: Arc::clone(&failed),
            };
            let written = self
                .tar(channel, items, control)
                .and_then(|mut channel| channel.close().map_err(|error| self.write_error(&error)));
            // Whatever happened on this side, the channel is closed by now
            // (dropped with the builder on an error), so the compressor sees
            // the end of its input and finishes.
            let joined = compressor.join();
            let file = match joined {
                Ok(Ok(file)) => file,
                Ok(Err(error)) => return Err(self.write_error(&error)),
                Err(_) => {
                    return Err(OperationError::Io {
                        path: self.temporary.to_path_buf(),
                        reason: "files.archive.error.compressor_stopped".to_string(),
                        errno: None,
                    });
                }
            };
            written?;
            Ok(file)
        })
    }
}

/// The tar header for a filesystem entry, with the permission bits and not
/// the file type in its mode field, the way GNU tar writes it.
fn tar_header(metadata: &fs::Metadata) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_metadata(metadata);
    header.set_mode(metadata.mode() & 0o7777);
    header
}

/// A file being read into a tar entry, reporting progress as it goes.
///
/// `tar::Builder` copies from a reader itself, so a pause or a cancel has to
/// happen inside `read`. A cancellation cannot travel through `io::Error`
/// intact, so it is left in `failure` for the caller to return as the
/// cancellation it is.
struct ProgressReader<'a, 'w, R> {
    inner: R,
    writer: &'a mut EntryWriter<'w>,
    control: &'a mut dyn JobControl,
    path: &'a Path,
    read: Rc<Cell<u64>>,
    failure: Rc<Cell<Option<OperationError>>>,
}

impl<R: Read> Read for ProgressReader<'_, '_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer).inspect_err(|error| {
            self.failure
                .set(Some(OperationError::from_io(self.path, error)));
        })?;
        self.read.set(self.read.get() + read as u64);
        if let Err(error) = self.writer.advance(read, self.control) {
            self.failure.set(Some(error));
            return Err(io::Error::other("files.archive.stopped"));
        }
        Ok(read)
    }
}

/// How much the tar side hands the compressor at once.
const CHANNEL_BLOCK: usize = 128 * 1024;

/// The tar side of the channel to the zstd compressor.
struct ChannelWriter {
    sender: Option<SyncSender<Vec<u8>>>,
    buffer: Vec<u8>,
    failed: Arc<AtomicBool>,
}

impl ChannelWriter {
    fn send(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let block = std::mem::replace(&mut self.buffer, Vec::with_capacity(CHANNEL_BLOCK));
        match &self.sender {
            Some(sender) => sender
                .send(block)
                .map_err(|_| io::Error::other("files.archive.error.compressor_stopped")),
            None => Err(io::Error::other("files.archive.error.compressor_stopped")),
        }
    }

    /// Sends what is left and ends the compressor's input.
    fn close(&mut self) -> io::Result<()> {
        let sent = self.send();
        self.sender = None;
        sent
    }
}

impl Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.failed.load(Ordering::Acquire) {
            return Err(io::Error::other("files.archive.error.output_failed"));
        }
        self.buffer.extend_from_slice(data);
        if self.buffer.len() >= CHANNEL_BLOCK {
            self.send()?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.send()
    }
}

/// The compressor side of the channel. It never fails: a closed channel is
/// the end of the input.
struct ChannelReader {
    receiver: Receiver<Vec<u8>>,
    current: Vec<u8>,
    position: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        while self.position == self.current.len() {
            match self.receiver.recv() {
                Ok(block) => {
                    self.current = block;
                    self.position = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let available = &self.current[self.position..];
        let count = available.len().min(buffer.len());
        buffer[..count].copy_from_slice(&available[..count]);
        self.position += count;
        Ok(count)
    }
}

/// An output that keeps its first error instead of returning it, for a
/// caller that panics on one.
struct KeepFirstError<W: Write> {
    inner: W,
    error: Option<io::Error>,
    failed: Arc<AtomicBool>,
}

impl KeepFirstError<BufWriter<File>> {
    fn finish(self) -> io::Result<File> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.inner.into_inner().map_err(|error| error.into_error())
    }
}

impl<W: Write> Write for KeepFirstError<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.error.is_none()
            && let Err(error) = self.inner.write_all(data)
        {
            self.error = Some(error);
            self.failed.store(true, Ordering::Release);
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.error.is_none()
            && let Err(error) = self.inner.flush()
        {
            self.error = Some(error);
            self.failed.store(true, Ordering::Release);
        }
        Ok(())
    }
}

// --- Zip timestamps -------------------------------------------------------

/// A zip timestamp for a Unix time.
///
/// Zip stores a local date and time with two-second resolution and no time
/// zone, which is how every other zip tool reads it back, so the conversion
/// goes through the C library's local time. A time outside what the format
/// can hold, 1980 to 2107, is stored as the format's own earliest date.
pub(crate) fn dos_time(seconds: i64) -> zip::DateTime {
    let Some(tm) = local_time(seconds) else {
        return zip::DateTime::default();
    };
    let (Ok(year), Ok(month), Ok(day), Ok(hour), Ok(minute), Ok(second)) = (
        u16::try_from(tm.tm_year + 1900),
        u8::try_from(tm.tm_mon + 1),
        u8::try_from(tm.tm_mday),
        u8::try_from(tm.tm_hour),
        u8::try_from(tm.tm_min),
        u8::try_from(tm.tm_sec),
    ) else {
        return zip::DateTime::default();
    };
    zip::DateTime::from_date_and_time(year, month, day, hour, minute, second).unwrap_or_default()
}

/// The Unix time a zip timestamp names, read as local time the way it was
/// written. `None` for a date the C library cannot place.
pub(crate) fn unix_time_of(time: zip::DateTime) -> Option<i64> {
    // SAFETY: `tm` is a plain C struct for which all-zero is a valid value,
    // and `mktime` only reads and normalises the struct it is given.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = i32::from(time.year()) - 1900;
    tm.tm_mon = i32::from(time.month()) - 1;
    tm.tm_mday = i32::from(time.day());
    tm.tm_hour = i32::from(time.hour());
    tm.tm_min = i32::from(time.minute());
    tm.tm_sec = i32::from(time.second());
    // Let the C library decide whether daylight saving applied then.
    tm.tm_isdst = -1;
    // SAFETY: `tm` is a valid, initialised struct owned by this frame.
    let seconds = unsafe { libc::mktime(&mut tm) };
    (seconds != -1).then_some(seconds)
}

fn local_time(seconds: i64) -> Option<libc::tm> {
    let time = libc::time_t::try_from(seconds).ok()?;
    // SAFETY: as above; `localtime_r` writes only into the struct it is given
    // and returns null when the time cannot be represented.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to values owned by this frame.
    let result = unsafe { libc::localtime_r(&time, &mut tm) };
    (!result.is_null()).then_some(tm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zip_timestamp_round_trips_through_local_time() {
        // An even second, since the format keeps two-second steps.
        let stamp = 1_614_834_368;
        assert_eq!(unix_time_of(dos_time(stamp)), Some(stamp));
    }

    #[test]
    fn a_time_the_format_cannot_hold_becomes_its_earliest_date() {
        let early = dos_time(0);
        assert_eq!((early.year(), early.month(), early.day()), (1980, 1, 1));
    }
}
