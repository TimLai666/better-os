//! The framed append log every history file is written as.
//!
//! The store has to survive the machine losing power in the middle of a write,
//! and it has to do so without discarding the hours of history that were
//! already there. So a file is a header followed by length-and-checksum framed
//! records, and reading one stops at the first frame that does not verify and
//! truncates the file back to the last one that did. A half-written record at
//! the end of the file is the expected outcome of a crash, not corruption of
//! the file as a whole, and it is recovered rather than reported.
//!
//! The header carries a schema version. A file a newer Better Monitor wrote is
//! refused and kept, the same rule `manager-store` and `awake-store` follow;
//! an older one goes through [`migrate`] on the way in.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::StoreError;

/// Identifies a Better Monitor log file before anything is parsed.
pub const LOG_MAGIC: &[u8; 8] = b"BMONLOG\x01";

/// The framing version, which is separate from the record schema version.
/// Framing has never changed; a change here would mean a different file
/// layout, not different record contents.
pub const FRAMING_VERSION: u32 = 1;

/// Bytes at the start of every log file: magic, framing version, schema
/// version.
pub const HEADER_BYTES: u64 = 16;

/// A record longer than this is treated as a corrupted frame rather than
/// allocated. One downsampled sample with a bounded process list is a few
/// tens of kilobytes at worst.
pub const MAX_RECORD_BYTES: u32 = 8 * 1024 * 1024;

/// What opening a log had to repair, and what it found.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Recovery {
    /// Records that framed and checksummed correctly.
    pub records: u64,
    /// Bytes dropped from the end of the file because the last record was not
    /// completely written. Non-zero means the previous run was interrupted.
    pub truncated_bytes: u64,
}

impl Recovery {
    pub fn recovered_a_torn_write(&self) -> bool {
        self.truncated_bytes > 0
    }
}

/// CRC-32 (IEEE 802.3), computed without a table.
///
/// This is here rather than as a dependency because the workspace has no
/// network access to add one, and because a checksum this small is easier to
/// audit than to justify.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// The migration seam for record contents.
///
/// Schema version 1 is the first shipped, so there is nothing to raise yet.
/// A future version lands its step here, where every reader passes through,
/// rather than in each call site.
pub fn migrate(
    value: serde_json::Value,
    from_version: u32,
    current_version: u32,
) -> Result<serde_json::Value, u32> {
    if from_version == current_version {
        Ok(value)
    } else {
        Err(from_version)
    }
}

fn io_error(path: &Path, source: io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// One append-only file of JSON records.
#[derive(Debug)]
pub struct AppendLog {
    path: PathBuf,
    file: File,
    schema_version: u32,
    /// Offset just past the last verified record. Every append starts here.
    end: u64,
}

impl AppendLog {
    /// Opens or creates a log, recovering a torn tail if there is one.
    pub fn open(
        path: impl Into<PathBuf>,
        schema_version: u32,
    ) -> Result<(Self, Recovery), StoreError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| io_error(&path, source))?;

        let length = file
            .metadata()
            .map_err(|source| io_error(&path, source))?
            .len();

        let stored_schema = if length < HEADER_BYTES {
            // Either brand new, or a header that never landed. Neither can
            // carry records, so the file is (re)stamped.
            file.set_len(0).map_err(|source| io_error(&path, source))?;
            file.seek(SeekFrom::Start(0))
                .map_err(|source| io_error(&path, source))?;
            let mut header = Vec::with_capacity(HEADER_BYTES as usize);
            header.extend_from_slice(LOG_MAGIC);
            header.extend_from_slice(&FRAMING_VERSION.to_le_bytes());
            header.extend_from_slice(&schema_version.to_le_bytes());
            file.write_all(&header)
                .map_err(|source| io_error(&path, source))?;
            file.sync_data().map_err(|source| io_error(&path, source))?;
            schema_version
        } else {
            let mut header = [0u8; HEADER_BYTES as usize];
            file.seek(SeekFrom::Start(0))
                .map_err(|source| io_error(&path, source))?;
            file.read_exact(&mut header)
                .map_err(|source| io_error(&path, source))?;
            if &header[..8] != LOG_MAGIC {
                return Err(StoreError::NotALog { path: path.clone() });
            }
            let framing = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            if framing != FRAMING_VERSION {
                return Err(StoreError::UnsupportedFraming {
                    path: path.clone(),
                    version: framing,
                });
            }
            let stored = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
            if stored > schema_version {
                return Err(StoreError::UnsupportedSchema {
                    path: path.clone(),
                    version: stored,
                });
            }
            stored
        };

        let (end, recovery) = scan(&mut file, &path, length)?;
        if recovery.truncated_bytes > 0 {
            file.set_len(end)
                .map_err(|source| io_error(&path, source))?;
            file.sync_data().map_err(|source| io_error(&path, source))?;
        }
        file.seek(SeekFrom::Start(end))
            .map_err(|source| io_error(&path, source))?;

        Ok((
            Self {
                path,
                file,
                schema_version: stored_schema,
                end,
            },
            recovery,
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The schema version the file on disk was stamped with.
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn bytes(&self) -> u64 {
        self.end
    }

    /// Appends one record and forces it to the platter before returning.
    ///
    /// The cost of `sync_data` per record is the price of the promise that a
    /// sample the service reported as recorded is actually recoverable, which
    /// is the whole reason this store exists.
    pub fn append<T: Serialize>(&mut self, value: &T) -> Result<(), StoreError> {
        let payload = serde_json::to_vec(value).map_err(StoreError::Serialize)?;
        if payload.len() > MAX_RECORD_BYTES as usize {
            return Err(StoreError::RecordTooLarge {
                bytes: payload.len(),
                limit: MAX_RECORD_BYTES as usize,
            });
        }
        let mut frame = Vec::with_capacity(payload.len() + 8);
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&crc32(&payload).to_le_bytes());
        frame.extend_from_slice(&payload);

        self.file
            .seek(SeekFrom::Start(self.end))
            .map_err(|source| io_error(&self.path, source))?;
        self.file
            .write_all(&frame)
            .map_err(|source| io_error(&self.path, source))?;
        self.file
            .sync_data()
            .map_err(|source| io_error(&self.path, source))?;
        self.end += frame.len() as u64;
        Ok(())
    }

    /// Reads every record, migrating each one from the stamped schema version.
    pub fn read_all<T: DeserializeOwned>(
        &mut self,
        current_version: u32,
    ) -> Result<Vec<T>, StoreError> {
        let payloads = read_payloads(&mut self.file, &self.path, self.end)?;
        let mut records = Vec::with_capacity(payloads.len());
        for payload in payloads {
            let value = serde_json::from_slice::<serde_json::Value>(&payload)
                .map_err(StoreError::Deserialize)?;
            let value =
                migrate(value, self.schema_version, current_version).map_err(|version| {
                    StoreError::UnsupportedSchema {
                        path: self.path.clone(),
                        version,
                    }
                })?;
            records.push(serde_json::from_value::<T>(value).map_err(StoreError::Deserialize)?);
        }
        Ok(records)
    }

    /// Replaces the whole file with `records`, through a temporary file and a
    /// rename, so an interrupted compaction leaves the previous log intact.
    pub fn rewrite<T: Serialize>(
        &mut self,
        records: &[T],
        schema_version: u32,
    ) -> Result<(), StoreError> {
        let temporary = self.path.with_extension("log.tmp");
        {
            let mut staging = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temporary)
                .map_err(|source| io_error(&temporary, source))?;
            let mut header = Vec::with_capacity(HEADER_BYTES as usize);
            header.extend_from_slice(LOG_MAGIC);
            header.extend_from_slice(&FRAMING_VERSION.to_le_bytes());
            header.extend_from_slice(&schema_version.to_le_bytes());
            staging
                .write_all(&header)
                .map_err(|source| io_error(&temporary, source))?;
            for record in records {
                let payload = serde_json::to_vec(record).map_err(StoreError::Serialize)?;
                let mut frame = Vec::with_capacity(payload.len() + 8);
                frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                frame.extend_from_slice(&crc32(&payload).to_le_bytes());
                frame.extend_from_slice(&payload);
                staging
                    .write_all(&frame)
                    .map_err(|source| io_error(&temporary, source))?;
            }
            staging
                .sync_data()
                .map_err(|source| io_error(&temporary, source))?;
        }
        std::fs::rename(&temporary, &self.path).map_err(|source| io_error(&self.path, source))?;

        let (reopened, _) = Self::open(self.path.clone(), schema_version)?;
        *self = reopened;
        Ok(())
    }
}

/// Walks the frames and reports where the last good one ends.
fn scan(file: &mut File, path: &Path, length: u64) -> Result<(u64, Recovery), StoreError> {
    let mut recovery = Recovery::default();
    if length < HEADER_BYTES {
        return Ok((HEADER_BYTES, recovery));
    }
    file.seek(SeekFrom::Start(HEADER_BYTES))
        .map_err(|source| io_error(path, source))?;
    let mut offset = HEADER_BYTES;
    let mut frame_header = [0u8; 8];
    loop {
        if offset + 8 > length {
            break;
        }
        if file.read_exact(&mut frame_header).is_err() {
            break;
        }
        let payload_length = u32::from_le_bytes([
            frame_header[0],
            frame_header[1],
            frame_header[2],
            frame_header[3],
        ]);
        let expected = u32::from_le_bytes([
            frame_header[4],
            frame_header[5],
            frame_header[6],
            frame_header[7],
        ]);
        if payload_length > MAX_RECORD_BYTES {
            break;
        }
        if offset + 8 + payload_length as u64 > length {
            break;
        }
        let mut payload = vec![0u8; payload_length as usize];
        if file.read_exact(&mut payload).is_err() {
            break;
        }
        if crc32(&payload) != expected {
            break;
        }
        offset += 8 + payload_length as u64;
        recovery.records += 1;
    }
    recovery.truncated_bytes = length.saturating_sub(offset);
    Ok((offset, recovery))
}

fn read_payloads(file: &mut File, path: &Path, end: u64) -> Result<Vec<Vec<u8>>, StoreError> {
    file.seek(SeekFrom::Start(HEADER_BYTES))
        .map_err(|source| io_error(path, source))?;
    let mut payloads = Vec::new();
    let mut offset = HEADER_BYTES;
    let mut frame_header = [0u8; 8];
    while offset + 8 <= end {
        file.read_exact(&mut frame_header)
            .map_err(|source| io_error(path, source))?;
        let payload_length = u32::from_le_bytes([
            frame_header[0],
            frame_header[1],
            frame_header[2],
            frame_header[3],
        ]);
        let mut payload = vec![0u8; payload_length as usize];
        file.read_exact(&mut payload)
            .map_err(|source| io_error(path, source))?;
        offset += 8 + payload_length as u64;
        payloads.push(payload);
    }
    Ok(payloads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct Row {
        index: u64,
        label: String,
    }

    fn row(index: u64) -> Row {
        Row {
            index,
            label: format!("row-{index}"),
        }
    }

    fn log(directory: &tempfile::TempDir) -> AppendLog {
        AppendLog::open(directory.path().join("history.log"), 1)
            .unwrap()
            .0
    }

    #[test]
    fn the_checksum_matches_a_known_vector() {
        // The IEEE CRC-32 of "123456789" is the standard check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn records_survive_a_close_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let mut writer = log(&directory);
        for index in 0..5 {
            writer.append(&row(index)).unwrap();
        }
        drop(writer);

        let (mut reader, recovery) =
            AppendLog::open(directory.path().join("history.log"), 1).unwrap();
        assert_eq!(recovery.records, 5);
        assert!(!recovery.recovered_a_torn_write());
        assert_eq!(
            reader.read_all::<Row>(1).unwrap(),
            (0..5).map(row).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_torn_final_record_is_dropped_and_everything_before_it_is_kept() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.log");
        let mut writer = AppendLog::open(&path, 1).unwrap().0;
        for index in 0..4 {
            writer.append(&row(index)).unwrap();
        }
        let full = writer.bytes();
        drop(writer);

        // Cut the file in the middle of the last record, which is what a
        // power loss during an append produces.
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(full - 6).unwrap();
        drop(file);

        let (mut reader, recovery) = AppendLog::open(&path, 1).unwrap();
        assert_eq!(recovery.records, 3);
        assert!(recovery.recovered_a_torn_write());
        assert_eq!(
            reader.read_all::<Row>(1).unwrap(),
            (0..3).map(row).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_recovered_log_can_still_be_appended_to() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.log");
        let mut writer = AppendLog::open(&path, 1).unwrap().0;
        writer.append(&row(0)).unwrap();
        writer.append(&row(1)).unwrap();
        let full = writer.bytes();
        drop(writer);
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(full - 3)
            .unwrap();

        let (mut reopened, recovery) = AppendLog::open(&path, 1).unwrap();
        assert_eq!(recovery.records, 1);
        reopened.append(&row(9)).unwrap();
        assert_eq!(reopened.read_all::<Row>(1).unwrap(), vec![row(0), row(9)]);
    }

    #[test]
    fn a_flipped_byte_inside_a_record_stops_the_log_there() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.log");
        let mut writer = AppendLog::open(&path, 1).unwrap().0;
        for index in 0..3 {
            writer.append(&row(index)).unwrap();
        }
        drop(writer);

        let mut bytes = std::fs::read(&path).unwrap();
        // The first record's payload starts at the header plus its frame.
        let target = HEADER_BYTES as usize + 8 + 4;
        bytes[target] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();

        let (_, recovery) = AppendLog::open(&path, 1).unwrap();
        assert_eq!(recovery.records, 0);
        assert!(recovery.truncated_bytes > 0);
    }

    #[test]
    fn a_newer_schema_is_refused_and_the_file_is_left_alone() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.log");
        let mut writer = AppendLog::open(&path, 7).unwrap().0;
        writer.append(&row(1)).unwrap();
        drop(writer);

        let error = AppendLog::open(&path, 1).unwrap_err();
        assert!(matches!(
            error,
            StoreError::UnsupportedSchema { version: 7, .. }
        ));
        assert!(path.exists());
    }

    #[test]
    fn a_file_that_is_not_a_log_is_refused_rather_than_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.log");
        std::fs::write(&path, b"this is somebody else's file entirely").unwrap();
        assert!(matches!(
            AppendLog::open(&path, 1),
            Err(StoreError::NotALog { .. })
        ));
        assert!(path.exists());
    }

    #[test]
    fn the_migration_seam_refuses_a_version_it_does_not_know() {
        assert!(migrate(serde_json::json!({}), 1, 1).is_ok());
        assert_eq!(migrate(serde_json::json!({}), 0, 1), Err(0));
    }

    #[test]
    fn a_rewrite_replaces_the_file_and_keeps_it_readable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.log");
        let mut writer = AppendLog::open(&path, 1).unwrap().0;
        for index in 0..6 {
            writer.append(&row(index)).unwrap();
        }
        let kept: Vec<Row> = (4..6).map(row).collect();
        writer.rewrite(&kept, 1).unwrap();
        assert_eq!(writer.read_all::<Row>(1).unwrap(), kept);

        writer.append(&row(99)).unwrap();
        drop(writer);
        let (mut reader, recovery) = AppendLog::open(&path, 1).unwrap();
        assert_eq!(recovery.records, 3);
        assert_eq!(reader.read_all::<Row>(1).unwrap().len(), 3);
    }

    #[test]
    fn an_empty_log_reads_as_no_records_rather_than_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let mut writer = log(&directory);
        assert!(writer.read_all::<Row>(1).unwrap().is_empty());
        assert_eq!(writer.bytes(), HEADER_BYTES);
    }
}
