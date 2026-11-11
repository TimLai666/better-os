//! What a zip declares at its end, read before the zip reader is.
//!
//! The `zip` crate reads a whole central directory into memory before it can
//! say how many entries an archive holds, so a limit checked on the opened
//! archive is checked after the memory is spent. The end of a zip states the
//! entry count and the directory's size in fixed fields, and this module reads
//! those fields and nothing else.
//!
//! The end-of-central-directory record is the last signature in the final
//! 65,557 bytes — the record's 22 bytes plus the longest comment it can carry —
//! whose comment fits inside the file. That is the first record the `zip` crate
//! itself accepts, and the only place Info-ZIP's `unzip` looks. When one of its
//! counts is saturated and a ZIP64 locator sits right before it, the ZIP64
//! record the locator points to declares the real values, as it does for the
//! `zip` crate.

use std::io::{self, Read, Seek, SeekFrom};

const END_SIGNATURE: &[u8] = b"PK\x05\x06";
const LOCATOR_SIGNATURE: &[u8] = b"PK\x06\x07";
const ZIP64_END_SIGNATURE: &[u8] = b"PK\x06\x06";
const END_LENGTH: usize = 22;
const LOCATOR_LENGTH: u64 = 20;
/// A ZIP64 end record without its extensible data.
const ZIP64_END_LENGTH: u64 = 56;
/// The record and the longest comment it can carry.
const TAIL: u64 = END_LENGTH as u64 + u16::MAX as u64;

/// The reason keys a zip whose end cannot be read is refused with.
pub mod reason {
    /// No end-of-central-directory record in the last 65,557 bytes.
    pub const END_RECORD_MISSING: &str = "files.archive.error.zip_end_record_missing";
    /// A ZIP64 locator that does not lead to a ZIP64 end record ending where
    /// the locator begins.
    pub const ZIP64_END_RECORD_UNUSABLE: &str = "files.archive.error.zip64_end_record_unusable";
}

/// What a zip says about its central directory before it is read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Declared {
    /// The larger of the two counts the record carries, entries on this disk
    /// and entries in all.
    pub entries: u64,
    /// The central directory's size, plus a ZIP64 record's extensible data,
    /// which the `zip` crate also holds in memory.
    pub directory_bytes: u64,
}

/// Reads the declaration at the end of `reader`. The outer error is the
/// disk's; the inner one is a reason key for an archive whose end is not where
/// the format puts it.
pub fn read_declaration<R: Read + Seek>(
    reader: &mut R,
) -> io::Result<Result<Declared, &'static str>> {
    let length = reader.seek(SeekFrom::End(0))?;
    let start = length.saturating_sub(TAIL);
    reader.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::with_capacity((length - start) as usize);
    reader
        .by_ref()
        .take(length - start)
        .read_to_end(&mut tail)?;

    let Some(at) = (0..(tail.len() + 1).saturating_sub(END_LENGTH))
        .rev()
        .find(|&at| {
            tail[at..].starts_with(END_SIGNATURE)
                && at + END_LENGTH + usize::from(u16_at(&tail, at + 20)) <= tail.len()
        })
    else {
        return Ok(Err(reason::END_RECORD_MISSING));
    };
    let record = &tail[at..at + END_LENGTH];
    let on_disk = u16_at(record, 8);
    let total = u16_at(record, 10);
    let size = u32_at(record, 12);
    let offset = u32_at(record, 16);
    let classic = Declared {
        entries: u64::from(on_disk.max(total)),
        directory_bytes: u64::from(size),
    };
    if total != u16::MAX && size != u32::MAX && offset != u32::MAX {
        return Ok(Ok(classic));
    }

    // A saturated field: the counts may be in a ZIP64 record. Without a
    // locator right before this record there is none, and the `zip` crate
    // reads the classic values too.
    let end_at = start + at as u64;
    let Some(locator_at) = end_at.checked_sub(LOCATOR_LENGTH) else {
        return Ok(Ok(classic));
    };
    let mut locator = [0u8; LOCATOR_LENGTH as usize];
    reader.seek(SeekFrom::Start(locator_at))?;
    reader.read_exact(&mut locator)?;
    if !locator.starts_with(LOCATOR_SIGNATURE) {
        return Ok(Ok(classic));
    }
    let stated = u64_at(&locator, 8);
    let disks = u32_at(&locator, 16);
    if disks > 1 {
        return Ok(Err(reason::ZIP64_END_RECORD_UNUSABLE));
    }
    // Where the locator says, which is right when nothing was prepended to
    // the archive, then directly before the locator, which is where a record
    // without extensible data sits even when a self-extracting stub shifted
    // every offset the archive states.
    let candidates = [Some(stated), locator_at.checked_sub(ZIP64_END_LENGTH)];
    for record_at in candidates.into_iter().flatten() {
        if record_at.saturating_add(ZIP64_END_LENGTH) > locator_at {
            continue;
        }
        let mut record = [0u8; ZIP64_END_LENGTH as usize];
        reader.seek(SeekFrom::Start(record_at))?;
        reader.read_exact(&mut record)?;
        let record_size = u64_at(&record, 4);
        // The record must end exactly where the locator begins, which is the
        // `zip` crate's own test of it.
        if !record.starts_with(ZIP64_END_SIGNATURE)
            || record_size < ZIP64_END_LENGTH - 12
            || record_at
                .checked_add(12)
                .and_then(|end| end.checked_add(record_size))
                != Some(locator_at)
        {
            continue;
        }
        let extensible = record_size - (ZIP64_END_LENGTH - 12);
        return Ok(Ok(Declared {
            entries: u64_at(&record, 24).max(u64_at(&record, 32)),
            directory_bytes: u64_at(&record, 40).saturating_add(extensible),
        }));
    }
    Ok(Err(reason::ZIP64_END_RECORD_UNUSABLE))
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn written(comment: &[u8], files: usize) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..files {
            writer
                .start_file(
                    format!("{index}.txt"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(b"content").unwrap();
        }
        writer
            .set_raw_comment(comment.to_vec().into_boxed_slice())
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn declared(bytes: &[u8]) -> Result<Declared, &'static str> {
        read_declaration(&mut Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn a_written_zip_declares_its_entries_and_directory() {
        let bytes = written(b"", 3);
        let archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let directory = bytes.len() as u64 - archive.central_directory_start() - END_LENGTH as u64;
        assert_eq!(
            declared(&bytes),
            Ok(Declared {
                entries: 3,
                directory_bytes: directory,
            })
        );
    }

    #[test]
    fn a_comment_that_looks_like_an_end_record_is_not_taken_for_one() {
        // A signature inside the comment whose own comment length would run
        // past the end of the file: the record before it is the real one.
        let mut comment = b"note PK\x05\x06".to_vec();
        comment.extend_from_slice(&[0xff; 18]);
        let bytes = written(&comment, 2);
        assert_eq!(declared(&bytes).unwrap().entries, 2);
        assert_eq!(zip::ZipArchive::new(Cursor::new(&bytes)).unwrap().len(), 2);
    }

    #[test]
    fn bytes_with_no_end_record_are_refused() {
        assert_eq!(
            declared(b"PK\x03\x04 nothing else"),
            Err(reason::END_RECORD_MISSING)
        );
        assert_eq!(declared(b""), Err(reason::END_RECORD_MISSING));
        // An end record more than a maximal comment away from the end.
        let mut bytes = written(b"", 1);
        bytes.extend(std::iter::repeat_n(0u8, TAIL as usize));
        assert_eq!(declared(&bytes), Err(reason::END_RECORD_MISSING));
    }

    #[test]
    fn a_locator_that_leads_nowhere_is_refused() {
        let mut bytes = vec![0u8; 100];
        bytes.extend_from_slice(LOCATOR_SIGNATURE);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(END_SIGNATURE);
        bytes.extend_from_slice(&[0u8; 4]);
        bytes.extend_from_slice(&[0xff; 12]);
        bytes.extend_from_slice(&[0u8; 2]);
        assert_eq!(declared(&bytes), Err(reason::ZIP64_END_RECORD_UNUSABLE));
    }

    /// A reader over a file of `length` bytes that are all zero but for the
    /// end record, remembering the lowest offset anything was read from.
    struct Huge {
        length: u64,
        position: u64,
        end: Vec<u8>,
        lowest: u64,
    }

    impl Read for Huge {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let left = self.length.saturating_sub(self.position);
            let count = (buffer.len() as u64).min(left) as usize;
            let end_at = self.length - self.end.len() as u64;
            for (index, byte) in buffer[..count].iter_mut().enumerate() {
                let at = self.position + index as u64;
                *byte = if at >= end_at {
                    self.end[(at - end_at) as usize]
                } else {
                    0
                };
            }
            if count > 0 {
                self.lowest = self.lowest.min(self.position);
            }
            self.position += count as u64;
            Ok(count)
        }
    }

    impl Seek for Huge {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            self.position = match to {
                SeekFrom::Start(at) => at,
                SeekFrom::End(delta) => self.length.saturating_add_signed(delta),
                SeekFrom::Current(delta) => self.position.saturating_add_signed(delta),
            };
            Ok(self.position)
        }
    }

    #[test]
    fn only_the_end_of_the_file_is_read() {
        // Ten gibibytes declaring ten million entries in a directory that
        // fills most of it.
        let length = 10 << 30;
        let mut end = END_SIGNATURE.to_vec();
        end.extend_from_slice(&[0u8; 4]);
        end.extend_from_slice(&[0xff; 12]);
        end.extend_from_slice(&[0u8; 2]);
        let mut zip64 = ZIP64_END_SIGNATURE.to_vec();
        zip64.extend_from_slice(&44u64.to_le_bytes());
        zip64.extend_from_slice(&[0u8; 12]);
        zip64.extend_from_slice(&10_000_000u64.to_le_bytes());
        zip64.extend_from_slice(&10_000_000u64.to_le_bytes());
        zip64.extend_from_slice(&(8u64 << 30).to_le_bytes());
        zip64.extend_from_slice(&(1u64 << 30).to_le_bytes());
        let record_at = length - (zip64.len() as u64 + LOCATOR_LENGTH + END_LENGTH as u64);
        let mut locator = LOCATOR_SIGNATURE.to_vec();
        locator.extend_from_slice(&0u32.to_le_bytes());
        locator.extend_from_slice(&record_at.to_le_bytes());
        locator.extend_from_slice(&1u32.to_le_bytes());
        let mut tail = zip64;
        tail.extend(locator);
        tail.extend(end);
        let mut reader = Huge {
            length,
            position: 0,
            end: tail,
            lowest: u64::MAX,
        };
        let declared = read_declaration(&mut reader).unwrap().unwrap();
        assert_eq!(declared.entries, 10_000_000);
        assert_eq!(declared.directory_bytes, 8 << 30);
        assert!(
            reader.lowest >= length - TAIL,
            "read from {} of {length}",
            reader.lowest
        );
    }
}
