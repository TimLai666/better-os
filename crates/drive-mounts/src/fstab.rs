//! Better OS's block in `/etc/fstab` (ADR 0032): read, change, and write
//! back, touching no line outside the block.
//!
//! The block is the lines between [`BEGIN`] and [`END`]. Every line in it is
//! one [`Entry`], in exactly the form [`Entry::line`] writes; anything else
//! in the block, a block that is not closed, or two blocks, makes the file
//! malformed and nothing is written. Lines outside the block are kept byte
//! for byte; one that cannot be a mount entry at all (more than six fields)
//! is malformed too, since a file `mount` itself would misread is no file to
//! add to.

use std::path::Path;

use crate::ids::{mount_point, valid_uuid};

/// The line the block starts with.
pub const BEGIN: &str =
    "# >>> Better OS drive mounting (written by Better Manager; do not edit inside this block)";
/// The line the block ends with.
pub const END: &str = "# <<< Better OS drive mounting";

/// The options every entry carries: `nosuid` and `nodev` so nothing on the
/// drive can raise privileges or open a device node (UDisks2's own defaults
/// for a drive a person mounts), `nofail` so a missing drive never stops the
/// boot, `x-gvfs-show` so the desktop shows it under its own name, and a
/// short wait for a device that is not there. Not `noexec`: a data drive
/// holds programs people run (an AppImage, a script, a game library), and
/// UDisks2 does not set it either (ADR 0032).
pub const BASE_OPTIONS: &[&str] = &[
    "nosuid",
    "nodev",
    "nofail",
    "x-gvfs-show",
    "x-systemd.device-timeout=10s",
];

/// Filesystems an entry may name.
pub const ENTRY_FILESYSTEMS: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "hfsplus",
];

/// Filesystems with no owners on disk: the person who turned the drive on
/// owns its files.
pub const OWNERLESS: &[&str] = &["vfat", "exfat", "ntfs", "ntfs3"];

/// One of Better OS's lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub uuid: String,
    pub filesystem: String,
    /// The owner a FAT, exFAT or NTFS drive's files are given.
    pub owner: Option<(u32, u32)>,
}

impl Entry {
    /// An entry for `uuid`, refused for a UUID or filesystem outside the
    /// accepted sets.
    pub fn new(uuid: &str, filesystem: &str, owner: Option<(u32, u32)>) -> Option<Self> {
        (valid_uuid(uuid) && ENTRY_FILESYSTEMS.contains(&filesystem)).then(|| Self {
            uuid: uuid.to_string(),
            filesystem: filesystem.to_string(),
            owner: owner.filter(|_| OWNERLESS.contains(&filesystem)),
        })
    }

    pub fn options(&self) -> String {
        let mut options: Vec<String> = BASE_OPTIONS.iter().map(|o| o.to_string()).collect();
        if let Some((uid, gid)) = self.owner {
            options.push(format!("uid={uid}"));
            options.push(format!("gid={gid}"));
        }
        options.join(",")
    }

    /// `UUID=… /mnt/better-os/… type options 0 0`.
    pub fn line(&self) -> String {
        format!(
            "UUID={} {} {} {} 0 0",
            self.uuid,
            mount_point(&self.uuid).display(),
            self.filesystem,
            self.options()
        )
    }

    /// The entry a block line is, when it is exactly one this module writes.
    fn parse(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [source, point, filesystem, options, "0", "0"] = fields.as_slice() else {
            return None;
        };
        let uuid = source.strip_prefix("UUID=")?;
        if !valid_uuid(uuid) || Path::new(point) != mount_point(uuid) {
            return None;
        }
        let mut owner = (None, None);
        let mut base = Vec::new();
        for option in options.split(',') {
            if let Some(uid) = option.strip_prefix("uid=") {
                owner.0 = Some(uid.parse::<u32>().ok()?);
            } else if let Some(gid) = option.strip_prefix("gid=") {
                owner.1 = Some(gid.parse::<u32>().ok()?);
            } else {
                base.push(option);
            }
        }
        if base != BASE_OPTIONS {
            return None;
        }
        let owner = match owner {
            (Some(uid), Some(gid)) => Some((uid, gid)),
            (None, None) => None,
            _ => return None,
        };
        let entry = Self::new(uuid, filesystem, owner)?;
        (entry.line() == fields.join(" ")).then_some(entry)
    }
}

/// Why a file was not written.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FstabError {
    #[error("the file is not text")]
    NotText,
    #[error("line {0} is not a mount entry")]
    BadLine(usize),
    #[error("line {0} begins Better OS's block, which never ends")]
    Unclosed(usize),
    #[error("line {0} begins a second Better OS block")]
    TwoBlocks(usize),
    #[error("line {0} ends a Better OS block that never began")]
    StrayEnd(usize),
    #[error("line {0}, inside Better OS's block, is not a line Better OS wrote")]
    ForeignInBlock(usize),
}

/// The names a drive goes by, to find a line someone else wrote for it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DriveNames {
    pub uuid: String,
    pub label: Option<String>,
    pub partuuid: Option<String>,
    /// Its device node and the links to it (`/dev/sdb1`,
    /// `/dev/disk/by-uuid/…`).
    pub device_paths: Vec<String>,
}

/// A line outside the block that names a drive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignLine {
    /// 1-based.
    pub line: usize,
    pub mount_point: String,
    /// Whether it says `noauto`, so the system does not mount it at boot.
    pub noauto: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Fstab {
    /// Every line before the block (or every line, when there is none).
    before: Vec<String>,
    entries: Vec<Entry>,
    after: Vec<String>,
    /// Whether the text ended with a newline.
    final_newline: bool,
    /// How many lines the block took in the text read (0 without one), so
    /// lines after it keep their real numbers.
    block_len: usize,
}

/// `\040` and the other octal escapes fstab uses for spaces.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && let Some(octal) = field.get(i + 1..i + 4)
            && let Ok(value) = u8::from_str_radix(octal, 8)
        {
            out.push(value);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

impl Fstab {
    /// Reads `text`, refusing a file that is malformed in any way above.
    pub fn parse(text: &str) -> Result<Self, FstabError> {
        if text.contains('\0') {
            return Err(FstabError::NotText);
        }
        let mut before = Vec::new();
        let mut entries = Vec::new();
        let mut after = Vec::new();
        // 0 before the block, 1 inside, 2 after.
        let mut stage = 0;
        let mut began = 0;
        let mut block_len = 0;
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            let trimmed = line.trim();
            // The whole marker line, never a comment that starts the same.
            if trimmed == BEGIN {
                match stage {
                    0 => {
                        stage = 1;
                        began = number;
                        block_len = 1;
                        continue;
                    }
                    _ => return Err(FstabError::TwoBlocks(number)),
                }
            }
            if trimmed == END {
                if stage != 1 {
                    return Err(FstabError::StrayEnd(number));
                }
                stage = 2;
                block_len += 1;
                continue;
            }
            if stage == 1 {
                block_len += 1;
                match Entry::parse(trimmed) {
                    Some(entry) => entries.push(entry),
                    None => return Err(FstabError::ForeignInBlock(number)),
                }
                continue;
            }
            if !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && trimmed.split_whitespace().count() > 6
            {
                return Err(FstabError::BadLine(number));
            }
            if stage == 0 {
                before.push(line.to_string());
            } else {
                after.push(line.to_string());
            }
        }
        if stage == 1 {
            return Err(FstabError::Unclosed(began));
        }
        Ok(Self {
            before,
            entries,
            after,
            final_newline: text.is_empty() || text.ends_with('\n'),
            block_len,
        })
    }

    /// Reads a file's bytes, which must be UTF-8.
    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, FstabError> {
        std::str::from_utf8(bytes)
            .map_err(|_| FstabError::NotText)
            .and_then(Self::parse)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, uuid: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.uuid.eq_ignore_ascii_case(uuid))
    }

    /// Adds or replaces the entry for its UUID.
    pub fn set(&mut self, entry: Entry) {
        match self
            .entries
            .iter_mut()
            .find(|known| known.uuid.eq_ignore_ascii_case(&entry.uuid))
        {
            Some(known) => *known = entry,
            None => self.entries.push(entry),
        }
    }

    /// Removes the entry for `uuid`; whether there was one.
    pub fn remove(&mut self, uuid: &str) -> bool {
        let count = self.entries.len();
        self.entries
            .retain(|entry| !entry.uuid.eq_ignore_ascii_case(uuid));
        self.entries.len() != count
    }

    /// Removes every entry, and so the block.
    pub fn clear(&mut self) -> Vec<Entry> {
        std::mem::take(&mut self.entries)
    }

    /// A line outside the block that names this drive.
    pub fn foreign_line(&self, names: &DriveNames) -> Option<ForeignLine> {
        let block_lines = self.block_len;
        let numbered = self
            .before
            .iter()
            .enumerate()
            .map(|(i, l)| (i + 1, l))
            .chain(
                self.after
                    .iter()
                    .enumerate()
                    .map(|(i, l)| (self.before.len() + block_lines + i + 1, l)),
            );
        for (number, line) in numbered {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = trimmed.split_whitespace().collect();
            let source = unescape(fields[0]);
            let matches = if let Some(uuid) = source.strip_prefix("UUID=") {
                unquote(uuid).eq_ignore_ascii_case(&names.uuid)
            } else if let Some(label) = source.strip_prefix("LABEL=") {
                names.label.as_deref() == Some(unquote(label))
            } else if let Some(partuuid) = source.strip_prefix("PARTUUID=") {
                names
                    .partuuid
                    .as_deref()
                    .is_some_and(|known| known.eq_ignore_ascii_case(unquote(partuuid)))
            } else {
                names.device_paths.contains(&source)
                    || source == format!("/dev/disk/by-uuid/{}", names.uuid)
            };
            if matches {
                return Some(ForeignLine {
                    line: number,
                    mount_point: fields.get(1).map(|p| unescape(p)).unwrap_or_default(),
                    noauto: fields
                        .get(3)
                        .is_some_and(|options| options.split(',').any(|o| o == "noauto")),
                });
            }
        }
        None
    }

    /// The file again: the lines outside the block as they were, the block
    /// where it was (at the end when it is new), and no block when there are
    /// no entries.
    pub fn render(&self) -> String {
        let mut lines: Vec<String> = self.before.clone();
        if !self.entries.is_empty() {
            lines.push(BEGIN.to_string());
            lines.extend(self.entries.iter().map(Entry::line));
            lines.push(END.to_string());
        }
        lines.extend(self.after.iter().cloned());
        let mut text = lines.join("\n");
        if !lines.is_empty() && (self.final_newline || !self.entries.is_empty()) {
            text.push('\n');
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM: &str = "# /etc/fstab: static file system information.\n\
UUID=1111-2222-root / ext4 errors=remount-ro 0 1\n\
UUID=7A3B-11CC /boot/efi vfat umask=0077 0 1\n\
/swapfile none swap sw 0 0\n";

    fn ntfs() -> Entry {
        Entry::new("01D9F00DBEEF1234", "ntfs", Some((1000, 1000))).unwrap()
    }

    #[test]
    fn adding_then_removing_gives_the_file_back_byte_for_byte() {
        let mut fstab = Fstab::parse(SYSTEM).unwrap();
        fstab.set(ntfs());
        let written = fstab.render();
        assert!(written.starts_with(SYSTEM));
        assert!(written.ends_with(&format!(
            "{BEGIN}\nUUID=01D9F00DBEEF1234 /mnt/better-os/01D9F00DBEEF1234 ntfs nosuid,nodev,nofail,x-gvfs-show,x-systemd.device-timeout=10s,uid=1000,gid=1000 0 0\n{END}\n"
        )));
        let mut again = Fstab::parse(&written).unwrap();
        assert_eq!(again.entries(), &[ntfs()]);
        assert!(again.remove("01D9F00DBEEF1234"));
        assert_eq!(again.render(), SYSTEM);
        assert!(!again.remove("01D9F00DBEEF1234"));
    }

    #[test]
    fn a_block_in_the_middle_stays_where_it_is_and_others_lines_are_kept() {
        let text = format!(
            "# mine\n{BEGIN}\n{}\n{END}\n  # an indented comment\n//server/share /srv cifs\\040x credentials=/root/c 0 0\n",
            ntfs().line()
        );
        let mut fstab = Fstab::parse(&text).unwrap();
        fstab.set(Entry::new("ABCD-1234", "ext4", Some((1, 1))).unwrap());
        let out = fstab.render();
        assert!(out.starts_with(&format!("# mine\n{BEGIN}\n")));
        assert!(out.contains("UUID=ABCD-1234 /mnt/better-os/ABCD-1234 ext4 nosuid,nodev,nofail,x-gvfs-show,x-systemd.device-timeout=10s 0 0\n"));
        assert!(out.ends_with(
            "{END}\n  # an indented comment\n//server/share /srv cifs\\040x credentials=/root/c 0 0\n"
                .replace("{END}", END)
                .as_str()
        ));
    }

    #[test]
    fn a_malformed_file_is_refused() {
        let entry = ntfs().line();
        for (text, error) in [
            (
                format!("{SYSTEM}{BEGIN}\n{entry}\n"),
                FstabError::Unclosed(5),
            ),
            (
                format!("{BEGIN}\n{END}\n{BEGIN}\n{END}\n"),
                FstabError::TwoBlocks(3),
            ),
            (format!("{SYSTEM}{END}\n"), FstabError::StrayEnd(5)),
            (
                format!("{BEGIN}\nUUID=1111-2222-root / ext4 defaults 0 1\n{END}\n"),
                FstabError::ForeignInBlock(2),
            ),
            (
                format!("{BEGIN}\n{} \n{END}\n", entry.replace("nosuid,", "")),
                FstabError::ForeignInBlock(2),
            ),
            (
                format!(
                    "{BEGIN}\n{}\n{END}\n",
                    entry.replace("/mnt/better-os/01D9F00DBEEF1234", "/etc")
                ),
                FstabError::ForeignInBlock(2),
            ),
            (
                "UUID=x / ext4 defaults 0 1 extra\n".to_string(),
                FstabError::BadLine(1),
            ),
            ("UUID=x /\0 ext4\n".to_string(), FstabError::NotText),
        ] {
            assert_eq!(Fstab::parse(&text), Err(error), "{text}");
        }
        assert_eq!(Fstab::parse_bytes(b"\xff\xfe"), Err(FstabError::NotText));
        assert!(Fstab::parse("").is_ok());
    }

    #[test]
    fn only_accepted_values_make_an_entry() {
        assert!(Entry::new("ABCD-1234", "swap", None).is_none());
        assert!(Entry::new("../x", "ext4", None).is_none());
        assert_eq!(
            Entry::new("ABCD-1234", "ext4", Some((1000, 1000)))
                .unwrap()
                .owner,
            None
        );
    }

    #[test]
    fn a_line_someone_else_wrote_is_found_by_any_name() {
        let text = format!(
            "{SYSTEM}LABEL=Data\\040Disk /data ext4 defaults,noauto 0 2\nPARTUUID=\"aa-bb\" /x ntfs defaults 0 0\n/dev/sdc1 /y vfat defaults 0 0\n{BEGIN}\n{}\n{END}\n/dev/disk/by-uuid/CAFE /z ext4 defaults 0 0\n",
            ntfs().line()
        );
        let fstab = Fstab::parse(&text).unwrap();
        let names = |uuid: &str| DriveNames {
            uuid: uuid.to_string(),
            ..DriveNames::default()
        };
        assert_eq!(
            fstab.foreign_line(&names("1111-2222-ROOT")).unwrap().line,
            2
        );
        let label = DriveNames {
            label: Some("Data Disk".into()),
            ..names("x")
        };
        let found = fstab.foreign_line(&label).unwrap();
        assert_eq!(
            (found.line, found.noauto, found.mount_point.as_str()),
            (5, true, "/data")
        );
        let part = DriveNames {
            partuuid: Some("AA-BB".into()),
            ..names("x")
        };
        assert_eq!(fstab.foreign_line(&part).unwrap().line, 6);
        let node = DriveNames {
            device_paths: vec!["/dev/sdc1".into()],
            ..names("x")
        };
        assert_eq!(fstab.foreign_line(&node).unwrap().line, 7);
        assert_eq!(fstab.foreign_line(&names("CAFE")).unwrap().line, 11);
        // Better OS's own line is not someone else's.
        assert_eq!(fstab.foreign_line(&names("01D9F00DBEEF1234")), None);
    }

    #[test]
    fn every_written_line_carries_nosuid_and_nodev() {
        let mut fstab = Fstab::parse(SYSTEM).unwrap();
        fstab.set(ntfs());
        fstab.set(Entry::new("ABCD-1234", "ext4", None).unwrap());
        let text = fstab.render();
        let block: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("UUID=") && line.contains("/mnt/better-os/"))
            .collect();
        assert_eq!(block.len(), 2);
        for line in block {
            let options: Vec<&str> = line.split_whitespace().nth(3).unwrap().split(',').collect();
            assert!(
                options.contains(&"nosuid") && options.contains(&"nodev"),
                "{line}"
            );
            assert!(
                !options.contains(&"noexec") && !options.contains(&"suid"),
                "{line}"
            );
        }
        // A line of ours without them is not ours.
        let weak = ntfs().line().replace("nosuid,nodev,", "");
        assert_eq!(
            Fstab::parse(&format!("{BEGIN}\n{weak}\n{END}\n")),
            Err(FstabError::ForeignInBlock(2))
        );
    }

    #[test]
    fn a_uuid_is_found_and_removed_whatever_its_case() {
        let mut fstab = Fstab::parse("").unwrap();
        fstab.set(Entry::new("abcd-1234", "vfat", None).unwrap());
        assert!(fstab.entry("ABCD-1234").is_some());
        fstab.set(Entry::new("ABCD-1234", "vfat", None).unwrap());
        assert_eq!(fstab.entries().len(), 1);
        assert!(fstab.remove("Abcd-1234"));
        assert!(fstab.entries().is_empty());
    }

    #[test]
    fn only_the_whole_marker_line_begins_the_block() {
        let text = "# >>> Better OS drive mounting notes: see the wiki\n\
UUID=1111-2222-root / ext4 defaults 0 1\n";
        let mut fstab = Fstab::parse(text).unwrap();
        assert!(fstab.entries().is_empty());
        fstab.set(ntfs());
        let out = fstab.render();
        assert!(out.starts_with(text));
        assert_eq!(out.matches(BEGIN).count(), 1);
    }

    #[test]
    fn lines_after_an_empty_block_keep_their_numbers() {
        let text = format!("# one\n{BEGIN}\n{END}\nLABEL=Data /data ext4 defaults 0 2\n");
        let fstab = Fstab::parse(&text).unwrap();
        let names = DriveNames {
            uuid: "x".into(),
            label: Some("Data".into()),
            ..DriveNames::default()
        };
        assert_eq!(fstab.foreign_line(&names).unwrap().line, 4);
    }
}
