//! Whether a drive holds something the running system is using (ticket 171).
//!
//! UDisks2 marks the drive the system runs from with `HintSystem` only when
//! it is internal. A system installed on a USB drive, a live USB, `/home` or
//! swap on an external disk is a hot-pluggable removable drive like any
//! stick, and nothing in UDisks2's answer says otherwise. Ejecting or
//! formatting it takes the running system, or a part of it, down with it.
//!
//! So the answer is worked out here, once, and Better Files' menu, the
//! storage service, and `manager-daemon` all ask [`SystemDrives`], so a drive
//! is never safe to one of them and in use to another.
//!
//! **The rule is the right way round.** A drive is the person's only when
//! everything of it that is mounted is mounted where removable media is
//! mounted for a person ([`is_person_media`]: `/media/…`, `/run/media/…`,
//! Better OS's own `/mnt/better-os/…`). A drive is held when any volume on
//! it, or under an unlocked container, logical volume or loop file on it, is
//! mounted anywhere else (`/`, `/home`, `/srv`, `/var/lib/docker`, a
//! booted ISO's `/isodevice`…), holds swap in use (a partition or a swap
//! file), or is named by an `/etc/fstab` line outside Better OS's own block.
//!
//! **Unknown is never Free.** A drive is [`SystemUse::Free`] only when the
//! mount table could be read, every held mount and swap was traced to a
//! drive, and this drive is not one of them. Anything short of that is
//! [`SystemUse::Unknown`], and Unknown is treated as held.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::PlatformDevice;
use crate::roots::Roots;

/// The line Better OS's block in `/etc/fstab` starts with (ADR 0032). The
/// block's lines are Better OS's own mounts under [`MOUNT_BASE`], not the
/// system's; `drive-mounts` writes it and reads these two constants.
pub const FSTAB_BEGIN: &str =
    "# >>> Better OS drive mounting (written by Better Manager; do not edit inside this block)";
/// The line Better OS's block ends with.
pub const FSTAB_END: &str = "# <<< Better OS drive mounting";
/// Where Better OS mounts the internal drives it was asked to (ADR 0032).
pub const MOUNT_BASE: &str = "/mnt/better-os";

/// Whose names count as a person's under `/media/<name>/` and
/// `/run/media/<name>/`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum People {
    /// Any name: tables parsed from text in a test, where no account is.
    #[default]
    Anyone,
    /// These account names (a test's).
    Listed(Vec<String>),
    /// The system's accounts in the login range (`UID_MIN`..`UID_MAX` of
    /// `login.defs`), each name looked up through the system's account
    /// database when asked, so a directory or network account counts too.
    System { uid_min: u32, uid_max: u32 },
    /// Every mount point but `/`: a fake UDisks2 on a private bus whose
    /// volumes are mounted under a temporary directory. Built only for
    /// tests ([`SystemSources::every_mount_personal`]).
    EveryMount,
}

impl People {
    fn knows(&self, name: &str) -> bool {
        match self {
            People::Anyone | People::EveryMount => true,
            People::Listed(names) => names.iter().any(|known| known == name),
            People::System { uid_min, uid_max } => {
                account_uid(name).is_some_and(|uid| uid >= *uid_min && uid <= *uid_max)
            }
        }
    }

    /// The login range `login.defs` under `etc` names, 1000..=60000 where it
    /// says nothing.
    pub fn system(etc: &Path) -> Self {
        let defs = std::fs::read_to_string(etc.join("login.defs")).unwrap_or_default();
        let value = |key: &str, default: u32| {
            defs.lines()
                .rev()
                .find_map(|line| {
                    let mut fields = line.split_whitespace();
                    (fields.next()? == key).then(|| fields.next()?.parse().ok())?
                })
                .unwrap_or(default)
        };
        People::System {
            uid_min: value("UID_MIN", 1000),
            uid_max: value("UID_MAX", 60000),
        }
    }
}

/// An account's uid, from the system's account database.
fn account_uid(name: &str) -> Option<u32> {
    let name = std::ffi::CString::new(name).ok()?;
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is to a live, writable value of the right type
    // and size; `found` is only read after the call says it is set.
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        )
    };
    (status == 0 && !found.is_null()).then_some(entry.pw_uid)
}

/// Where removable media is mounted for a person: UDisks2's
/// `/media/<name>/…` and `/run/media/<name>/…` where `<name>` is one of
/// `people`, and Better OS's own mount root. A bare `/media/backup` (a
/// systemd mount unit, an autofs map, a service's own `mount`) is the
/// system's, as is anything else mounted elsewhere.
pub fn is_person_media(point: &Path, people: &People) -> bool {
    if *people == People::EveryMount {
        return point != Path::new("/");
    }
    if point.starts_with(MOUNT_BASE) && point != Path::new(MOUNT_BASE) {
        return true;
    }
    ["/media", "/run/media"].iter().any(|root| {
        let Ok(rest) = point.strip_prefix(root) else {
            return false;
        };
        let mut parts = rest.components();
        match (parts.next(), parts.next()) {
            (Some(name), Some(_)) => people.knows(&name.as_os_str().to_string_lossy()),
            _ => false,
        }
    })
}

/// What, if anything, of the running system a drive holds.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemUse {
    /// Nothing the running system uses is on it.
    Free,
    /// The running system's root (`/`) is on it.
    RunningSystem,
    /// Something of it is mounted outside a person's media folders (`/home`,
    /// `/boot/efi`, `/srv`, a live session's medium).
    SystemFolder { path: PathBuf },
    /// Swap the system is using is on it.
    Swap,
    /// A line in `/etc/fstab` that Better OS did not write names it.
    SystemTable,
    /// It could not be told: treated as held.
    #[default]
    Unknown,
}

impl SystemUse {
    /// Whether ejecting, unmounting, powering off or formatting it may be
    /// offered and carried out.
    pub fn permits_removal(&self) -> bool {
        matches!(self, SystemUse::Free)
    }

    /// The typed reason a refused request carries
    /// (`storage.system.running_system`, …); `None` for a free drive.
    pub fn refusal(&self) -> Option<&'static str> {
        match self {
            SystemUse::Free => None,
            SystemUse::RunningSystem => Some("storage.system.running_system"),
            SystemUse::SystemFolder { .. } => Some("storage.system.folder"),
            SystemUse::Swap => Some("storage.system.swap"),
            SystemUse::SystemTable => Some("storage.system.table"),
            SystemUse::Unknown => Some("storage.system.unknown"),
        }
    }

    /// The reason a refusal names, back from its code.
    pub fn from_refusal(code: &str) -> Option<SystemUse> {
        match code {
            "storage.system.running_system" => Some(SystemUse::RunningSystem),
            "storage.system.swap" => Some(SystemUse::Swap),
            "storage.system.table" => Some(SystemUse::SystemTable),
            "storage.system.unknown" => Some(SystemUse::Unknown),
            _ => code
                .strip_prefix("storage.system.folder")
                .map(|rest| SystemUse::SystemFolder {
                    path: PathBuf::from(rest.strip_prefix(':').unwrap_or("")),
                }),
        }
    }

    /// The refusal with its path, for a folder: `storage.system.folder:/home`.
    pub fn refusal_detail(&self) -> Option<String> {
        let code = self.refusal()?;
        Some(match self {
            SystemUse::SystemFolder { path } => format!("{code}:{}", path.display()),
            _ => code.to_string(),
        })
    }

    /// How strongly it holds a drive, for keeping the strongest of several.
    fn rank(&self) -> u8 {
        match self {
            SystemUse::RunningSystem => 5,
            SystemUse::SystemFolder { .. } => 4,
            SystemUse::Swap => 3,
            SystemUse::SystemTable => 2,
            SystemUse::Unknown => 1,
            SystemUse::Free => 0,
        }
    }

    /// The stronger of two answers about the same drive.
    pub fn strongest(self, other: SystemUse) -> SystemUse {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// One thing the running system uses that lives on block devices: a mount
/// outside a person's media folders, or a swap in use, with every way it is
/// named.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemSource {
    pub held: SystemUse,
    /// Device nodes: what the kernel names (`/dev/mapper/vg-root`), what that
    /// resolves to (`/dev/dm-0`), and every node under it, down to the
    /// partitions (`/dev/sda3`), a loop file's own device included.
    pub nodes: Vec<String>,
    /// A ZFS pool: its members are the devices UDisks2 reports as
    /// `zfs_member` with the pool's name as their label.
    pub zfs_pool: Option<String>,
}

impl SystemSource {
    /// A source named by one device node.
    pub fn node(held: SystemUse, node: impl Into<String>) -> Self {
        Self {
            held,
            nodes: vec![node.into()],
            zfs_pool: None,
        }
    }
}

/// What the kernel and the system's mount table say the running system uses,
/// read from `/proc`, `/sys` and `/etc/fstab`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemSources {
    pub sources: Vec<SystemSource>,
    /// The device each `/etc/fstab` line outside Better OS's block names
    /// (`UUID=…`, `LABEL=…`, `PARTUUID=…`, `/dev/…`), and each systemd mount
    /// unit's `What=` under `/etc/systemd/system` and `/run/systemd/system`.
    pub fstab: Vec<String>,
    /// Whose `/media/<name>/` folders are a person's ([`is_person_media`]).
    pub people: People,
}

/// Block devices that are no drive: memory and compressed swap.
fn is_memory_node(node: &str) -> bool {
    ["/dev/zram", "/dev/ram"]
        .iter()
        .any(|prefix| node.starts_with(prefix))
}

/// A mount table field, with the kernel's octal escapes (`\040` for a
/// space) undone.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && let Some(octal) = field.get(index + 1..index + 4)
            && let Ok(value) = u8::from_str_radix(octal, 8)
        {
            out.push(value);
            index += 4;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One mount, as either mount table spells it.
struct Mount {
    source: String,
    point: PathBuf,
    filesystem: String,
    /// `major:minor` from `/proc/self/mountinfo`; `None` from `/proc/mounts`.
    device: Option<(u32, u32)>,
}

/// A swap in use.
struct SwapEntry {
    name: String,
    partition: bool,
}

fn parse_mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (source, point, filesystem) = (fields.next()?, fields.next()?, fields.next()?);
            Some(Mount {
                source: unescape(source),
                point: PathBuf::from(unescape(point)),
                filesystem: filesystem.to_string(),
                device: None,
            })
        })
        .collect()
}

/// `/proc/self/mountinfo`: `id parent major:minor root point options
/// [optional…] - type source super-options`.
fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let fields: Vec<&str> = before.split_whitespace().collect();
            let (major, minor) = fields.get(2)?.split_once(':')?;
            let point = fields.get(4)?;
            let mut rest = after.split_whitespace();
            let (filesystem, source) = (rest.next()?, rest.next()?);
            Some(Mount {
                source: unescape(source),
                point: PathBuf::from(unescape(point)),
                filesystem: filesystem.to_string(),
                device: Some((major.parse().ok()?, minor.parse().ok()?)),
            })
        })
        .collect()
}

fn parse_swaps(text: &str) -> Vec<SwapEntry> {
    // `Filename Type Size Used Priority`, after a header line.
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (name, kind) = (fields.next()?, fields.next()?);
            let name = unescape(name);
            (!is_memory_node(&name)).then_some(SwapEntry {
                name,
                partition: kind == "partition",
            })
        })
        .collect()
}

/// The device each fstab line outside Better OS's block names. Lines for
/// filesystems on no device (`proc`, `tmpfs`, `none`) name nothing.
pub fn parse_fstab(text: &str) -> Vec<String> {
    let mut inside = false;
    let mut specs = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == FSTAB_BEGIN {
            inside = true;
            continue;
        }
        if trimmed == FSTAB_END {
            inside = false;
            continue;
        }
        if inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(spec) = trimmed.split_whitespace().next() else {
            continue;
        };
        let spec = unquoted_tag(&unescape(spec));
        let names_device = spec.starts_with("/dev/")
            || ["UUID=", "LABEL=", "PARTUUID=", "PARTLABEL="]
                .iter()
                .any(|prefix| spec.starts_with(prefix));
        if names_device {
            specs.push(spec);
        }
    }
    specs
}

/// `UUID="abcd"` as `UUID=abcd`: fstab and mount units may quote a tag's
/// value.
fn unquoted_tag(spec: &str) -> String {
    match spec.split_once('=') {
        Some((tag, value)) if !spec.starts_with('/') => {
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .or_else(|| {
                    value
                        .strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                })
                .unwrap_or(value);
            format!("{tag}={value}")
        }
        _ => spec.to_string(),
    }
}

/// The `What=` of every `.mount` unit in `directory`, and of the `.mount`
/// unit each `.automount` there stands for, as table entries.
fn mount_unit_sources(directory: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let unit = if name.ends_with(".mount") {
            path
        } else if let Some(stem) = name.strip_suffix(".automount") {
            directory.join(format!("{stem}.mount"))
        } else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&unit) else {
            continue;
        };
        for line in text.lines() {
            if let Some(what) = line.trim().strip_prefix("What=") {
                let what = unquoted_tag(what.trim());
                if !what.is_empty() && !found.contains(&what) {
                    found.push(what);
                }
            }
        }
    }
    found
}

/// Whether an fstab line's device field names `device`, in any of its
/// forms: a tag, or a `/dev/disk/by-*` link to one.
fn fstab_names(spec: &str, device: &PlatformDevice) -> bool {
    let same = |value: Option<&String>, wanted: &str| {
        value.is_some_and(|value| value.eq_ignore_ascii_case(wanted))
    };
    let spec = unquoted_tag(spec);
    let spec = match spec.strip_prefix("/dev/disk/") {
        Some(link) if !names_node(&spec, device) => match link.split_once('/') {
            Some(("by-uuid", value)) => format!("UUID={value}"),
            Some(("by-label", value)) => format!("LABEL={}", unescape_udev(value)),
            Some(("by-partuuid", value)) => format!("PARTUUID={value}"),
            Some(("by-partlabel", value)) => format!("PARTLABEL={}", unescape_udev(value)),
            _ => spec.clone(),
        },
        _ => spec.clone(),
    };
    let spec = spec.as_str();
    if let Some(uuid) = spec.strip_prefix("UUID=") {
        same(device.block.id_uuid.as_ref(), uuid)
    } else if let Some(label) = spec.strip_prefix("LABEL=") {
        device.block.id_label.as_deref() == Some(label)
    } else if let Some(partuuid) = spec.strip_prefix("PARTUUID=") {
        same(device.block.partition_uuid.as_ref(), partuuid)
    } else if let Some(label) = spec.strip_prefix("PARTLABEL=") {
        device.block.symlinks.iter().any(|link| {
            link.strip_prefix("/dev/disk/by-partlabel/")
                .is_some_and(|value| unescape_udev(value) == label)
        })
    } else {
        names_node(spec, device)
    }
}

impl SystemSources {
    /// No mount, no swap, no fstab line, and every mount point a person's:
    /// for a fake UDisks2 on a private bus whose volumes are mounted under a
    /// temporary directory. Never what a machine is read as, and not in a
    /// shipped binary.
    #[cfg(any(test, feature = "test-support"))]
    pub fn every_mount_personal() -> Self {
        Self {
            people: People::EveryMount,
            ..Self::default()
        }
    }

    fn is_personal(&self, point: &Path) -> bool {
        is_person_media(point, &self.people)
    }

    /// What `/proc/self/mounts` (or `mountinfo`), `/proc/swaps` and
    /// `/etc/fstab` text say, each source with its own node only: no link is
    /// followed and no file is read.
    pub fn parse(mounts: &str, swaps: &str) -> Self {
        let mounts = if mounts.contains(" - ") {
            parse_mountinfo(mounts)
        } else {
            parse_mounts(mounts)
        };
        Self::from_tables(
            &mounts,
            &parse_swaps(swaps),
            Vec::new(),
            People::Anyone,
            |_, _, _| Vec::new(),
        )
    }

    /// The same, with an fstab.
    pub fn parse_with_fstab(mounts: &str, swaps: &str, fstab: &str) -> Self {
        let mut found = Self::parse(mounts, swaps);
        found.fstab = parse_fstab(fstab);
        found
    }

    /// Builds the sources; `trace(node, device number, file)` adds what a
    /// node, a mount's device number or a file's device resolves to.
    fn from_tables(
        mounts: &[Mount],
        swaps: &[SwapEntry],
        fstab: Vec<String>,
        people: People,
        trace: impl Fn(Option<&str>, Option<(u32, u32)>, Option<&Path>) -> Vec<String>,
    ) -> Self {
        let mut sources = Vec::new();
        for mount in mounts {
            if is_person_media(&mount.point, &people) {
                continue;
            }
            let held = if mount.point == Path::new("/") {
                SystemUse::RunningSystem
            } else {
                SystemUse::SystemFolder {
                    path: mount.point.clone(),
                }
            };
            let block_number = mount.device.filter(|(major, _)| *major != 0);
            let mut source = SystemSource {
                held,
                ..SystemSource::default()
            };
            match mount.filesystem.as_str() {
                // `rpool/ROOT/ubuntu`: the pool is the first segment.
                "zfs" => {
                    source.zfs_pool = mount.source.split('/').next().map(str::to_string);
                }
                // `/dev/sda1:/dev/sdb1`: every member.
                "bcachefs" => {
                    for member in mount.source.split(':').filter(|m| m.starts_with("/dev/")) {
                        source.nodes.extend(trace(Some(member), None, None));
                        source.nodes.push(member.to_string());
                    }
                }
                _ if mount.source.starts_with("/dev/") => {
                    if is_memory_node(&mount.source) {
                        continue;
                    }
                    source.nodes.push(mount.source.clone());
                    source
                        .nodes
                        .extend(trace(Some(&mount.source), block_number, None));
                }
                // No device path but a real block device number (`/dev/root`
                // spelled another way, a filesystem that names its source
                // freely): resolved through the number.
                _ if block_number.is_some() => {
                    let traced = trace(None, block_number, None);
                    if traced.is_empty() {
                        // Cannot be told which drive: kept untraced.
                        source.nodes.push(format!("untraced:{}", mount.source));
                    }
                    source.nodes.extend(traced);
                }
                // tmpfs, proc, overlay, FUSE: on no drive.
                _ => continue,
            }
            dedup(&mut source.nodes);
            sources.push(source);
        }
        for swap in swaps {
            let mut source = SystemSource {
                held: SystemUse::Swap,
                ..SystemSource::default()
            };
            if swap.partition {
                source.nodes.push(swap.name.clone());
                source.nodes.extend(trace(Some(&swap.name), None, None));
            } else {
                // A swap file: the drive of the filesystem it is on.
                source
                    .nodes
                    .extend(trace(None, None, Some(Path::new(&swap.name))));
            }
            dedup(&mut source.nodes);
            sources.push(source);
        }
        Self {
            sources,
            fstab,
            people,
        }
    }

    /// Reads the running system's mounts (`/proc/self/mountinfo`), swap and
    /// fstab under `roots` (`/etc/fstab` beside `roots.proc`), and follows
    /// each node through `/dev`'s links, `/sys/class/block/*/slaves` (LVM,
    /// LUKS, RAID), a loop device's backing file, and a mount's device
    /// number to the partitions it is on. `None` when the mount table cannot
    /// be read: then nothing about any drive is known.
    pub fn read(roots: &Roots) -> Option<Self> {
        let mounts = match std::fs::read_to_string(roots.proc.join("self/mountinfo")) {
            Ok(text) => parse_mountinfo(&text),
            Err(_) => parse_mounts(
                &std::fs::read_to_string(roots.proc.join("self/mounts"))
                    .or_else(|_| std::fs::read_to_string(roots.proc.join("mounts")))
                    .ok()?,
            ),
        };
        // No swap file is no swap.
        let swaps =
            parse_swaps(&std::fs::read_to_string(roots.proc.join("swaps")).unwrap_or_default());
        let etc = roots
            .proc
            .parent()
            .map_or_else(|| PathBuf::from("/etc"), |root| root.join("etc"));
        let mut fstab = std::fs::read_to_string(etc.join("fstab"))
            .map(|text| parse_fstab(&text))
            .unwrap_or_default();
        // Mount units an administrator or a package wrote (not the ones
        // systemd generates from fstab, under /run/systemd/generator).
        let run = roots
            .proc
            .parent()
            .map_or_else(|| PathBuf::from("/run"), |root| root.join("run"));
        for directory in [etc.join("systemd/system"), run.join("systemd/system")] {
            for what in mount_unit_sources(&directory) {
                if !fstab.contains(&what) {
                    fstab.push(what);
                }
            }
        }
        let people = People::system(&etc);
        let numbers: Vec<((u32, u32), String)> = mounts
            .iter()
            .filter(|mount| mount.source.starts_with("/dev/"))
            .filter_map(|mount| Some((mount.device?, mount.source.clone())))
            .collect();
        let trace = |node: Option<&str>, number: Option<(u32, u32)>, file: Option<&Path>| {
            let mut found = Vec::new();
            if let Some(node) = node {
                underlying(roots, &numbers, node, &mut found, 0);
            }
            if let Some(number) = number
                && let Some(node) = node_of_number(roots, &numbers, number)
            {
                underlying(roots, &numbers, &node, &mut found, 0);
            }
            if let Some(file) = file
                && let Some(number) = device_of_file(file)
                && let Some(node) = node_of_number(roots, &numbers, number)
            {
                underlying(roots, &numbers, &node, &mut found, 0);
            }
            found
        };
        Some(Self::from_tables(&mounts, &swaps, fstab, people, trace))
    }
}

fn dedup(nodes: &mut Vec<String>) {
    let mut seen = Vec::new();
    nodes.retain(|node| {
        if seen.contains(node) {
            false
        } else {
            seen.push(node.clone());
            true
        }
    });
}

/// The device a file is on, as `major:minor`.
fn device_of_file(file: &Path) -> Option<(u32, u32)> {
    use std::os::unix::fs::MetadataExt as _;
    let device = std::fs::metadata(file).ok()?.dev();
    Some((libc::major(device), libc::minor(device)))
}

/// The node a device number names: the block device `/sys/dev/block` lists,
/// or, for a filesystem's anonymous number (btrfs, `0:…`), the device the
/// mount with that number came from.
fn node_of_number(
    roots: &Roots,
    numbers: &[((u32, u32), String)],
    number: (u32, u32),
) -> Option<String> {
    if number.0 == 0 {
        return numbers
            .iter()
            .find(|(known, _)| *known == number)
            .map(|(_, source)| source.clone());
    }
    let link = std::fs::read_link(
        roots
            .sys
            .join("dev/block")
            .join(format!("{}:{}", number.0, number.1)),
    )
    .ok()?;
    let name = link.file_name()?.to_string_lossy().into_owned();
    Some(format!("/dev/{name}"))
}

/// udev's `\x20` escapes in a `/dev/disk/by-label` link name, undone.
fn unescape_udev(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && bytes.get(index + 1) == Some(&b'x')
            && let Some(hex) = name.get(index + 2..index + 4)
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            out.push(value);
            index += 4;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The loop device a loop partition is on (`loop0p1` is on `loop0`).
fn loop_parent(name: &str) -> Option<String> {
    let rest = name.strip_prefix("loop")?;
    let (number, partition) = rest.split_once('p')?;
    (!number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && !partition.is_empty()
        && partition.bytes().all(|byte| byte.is_ascii_digit()))
    .then(|| format!("loop{number}"))
}

/// `node`, the node its link resolves to, every node under it (device
/// mapper and RAID slaves), and for a loop device (or a partition of one)
/// the device its backing file is on, in that order and without repeats.
fn underlying(
    roots: &Roots,
    numbers: &[((u32, u32), String)],
    node: &str,
    into: &mut Vec<String>,
    depth: usize,
) {
    if depth > 8 || into.iter().any(|known| known == node) {
        return;
    }
    into.push(node.to_string());
    let real = std::fs::canonicalize(node)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| node.to_string());
    if real != node && !into.contains(&real) {
        into.push(real.clone());
    }
    let Some(name) = Path::new(&real)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
    else {
        return;
    };
    // A partition of a loop device has no backing file of its own.
    if let Some(parent) = loop_parent(&name) {
        underlying(roots, numbers, &format!("/dev/{parent}"), into, depth + 1);
    }
    let block = roots.sys.join("class/block").join(&name);
    // A loop device: the drive its file is on (a booted ISO's stick, a
    // snap's root). Not traceable when the file is gone: the loop then
    // stays a node no drive answers to, and the drives are unknown.
    if let Ok(backing) = std::fs::read_to_string(block.join("loop/backing_file")) {
        let backing = backing.trim().trim_end_matches(" (deleted)");
        if let Some(number) = device_of_file(Path::new(backing))
            && let Some(node) = node_of_number(roots, numbers, number)
        {
            underlying(roots, numbers, &node, into, depth + 1);
        }
    }
    let Ok(entries) = std::fs::read_dir(block.join("slaves")) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        underlying(roots, numbers, &format!("/dev/{name}"), into, depth + 1);
    }
}

/// Whether `node` names `device`: its node or one of its links. No I/O:
/// [`SystemSources::read`] has already added what each node resolves to.
pub fn names_node(node: &str, device: &PlatformDevice) -> bool {
    node == device.address.device_path
        || node == device.block.device_path
        || device.block.symlinks.iter().any(|link| link == node)
}

/// The UDisks2 drive object a volume is on: its own, or, for an unlocked
/// encrypted volume, its container's (following a chain of them).
pub fn drive_object_of<'a>(
    device: &'a PlatformDevice,
    devices: &'a [PlatformDevice],
) -> Option<&'a str> {
    let mut current = device;
    for _ in 0..8 {
        if let Some(drive) = &current.drive
            && !drive.object_path.is_empty()
            && drive.object_path != "/"
        {
            return Some(drive.object_path.as_str());
        }
        let backing = current.block.crypto_backing.as_deref()?;
        current = devices
            .iter()
            .find(|other| other.address.object_path == backing)?;
    }
    None
}

/// Which drives hold something of the running system: the one answer every
/// caller asks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemDrives {
    /// Drive object → what it holds.
    held: BTreeMap<String, SystemUse>,
    /// The mount table was read and every held source was traced to a
    /// drive: a drive not in `held` is free.
    complete: bool,
}

impl SystemDrives {
    /// Nothing known: every drive is [`SystemUse::Unknown`].
    pub fn unknown() -> Self {
        Self::default()
    }

    /// Works out which of `devices`' drives hold what `sources` names, and
    /// what UDisks2 itself reports mounted outside a person's media folders.
    /// `None` sources (the mount table could not be read) know nothing.
    pub fn find(devices: &[PlatformDevice], sources: Option<&SystemSources>) -> Self {
        let Some(sources) = sources else {
            return Self::unknown();
        };
        let mut found = Self {
            held: BTreeMap::new(),
            complete: true,
        };
        for source in &sources.sources {
            let mut traced = false;
            for device in devices.iter().filter(|device| {
                source.nodes.iter().any(|node| names_node(node, device))
                    || source.zfs_pool.as_deref().is_some_and(|pool| {
                        device.block.id_type.as_deref() == Some("zfs_member")
                            && device.block.id_label.as_deref() == Some(pool)
                    })
            }) {
                traced |= found.hold(device, devices, source.held.clone());
            }
            if !traced {
                found.complete = false;
            }
        }
        // UDisks2's own list of where each volume is mounted, for a mount the
        // kernel names in a way no link matches.
        for device in devices {
            if let Some(point) = device
                .mount_points()
                .into_iter()
                .find(|point| !sources.is_personal(point))
            {
                let held = if point == Path::new("/") {
                    SystemUse::RunningSystem
                } else {
                    SystemUse::SystemFolder { path: point }
                };
                found.hold(device, devices, held);
            }
        }
        // The administrator's own mount table.
        for spec in &sources.fstab {
            for device in devices.iter().filter(|device| fstab_names(spec, device)) {
                found.hold(device, devices, SystemUse::SystemTable);
            }
        }
        // A btrfs filesystem across several devices is held on all of them.
        let held_uuids: Vec<(String, SystemUse)> = devices
            .iter()
            .filter(|device| device.block.id_type.as_deref() == Some("btrfs"))
            .filter_map(|device| {
                let held = found.of_device(device, devices);
                let uuid = device.block.id_uuid.clone()?;
                (!held.permits_removal() && held != SystemUse::Unknown).then_some((uuid, held))
            })
            .collect();
        for (uuid, held) in held_uuids {
            for device in devices
                .iter()
                .filter(|device| device.block.id_uuid.as_deref() == Some(uuid.as_str()))
            {
                found.hold(device, devices, held.clone());
            }
        }
        found
    }

    /// Marks the drive `device` is on; false when it is on none.
    fn hold(
        &mut self,
        device: &PlatformDevice,
        devices: &[PlatformDevice],
        held: SystemUse,
    ) -> bool {
        let Some(drive) = drive_object_of(device, devices) else {
            return false;
        };
        let entry = self
            .held
            .entry(drive.to_string())
            .or_insert(SystemUse::Free);
        *entry = std::mem::replace(entry, SystemUse::Free).strongest(held);
        true
    }

    /// What the drive with this UDisks2 object holds. A drive that is held
    /// says what holds it even when something else was not traced; any
    /// other is Free only when everything was.
    pub fn of_drive(&self, drive_object: Option<&str>) -> SystemUse {
        let Some(drive) = drive_object.filter(|drive| !drive.is_empty() && *drive != "/") else {
            return SystemUse::Unknown;
        };
        match self.held.get(drive) {
            Some(held) => held.clone(),
            None if self.complete => SystemUse::Free,
            None => SystemUse::Unknown,
        }
    }

    /// What the drive a volume is on holds.
    pub fn of_device(&self, device: &PlatformDevice, devices: &[PlatformDevice]) -> SystemUse {
        self.of_drive(drive_object_of(device, devices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BlockInfo, DeviceAddress, DriveInfo};

    fn volume(object: &str, node: &str, drive: Option<&str>, mounted: &[&str]) -> PlatformDevice {
        PlatformDevice {
            address: DeviceAddress {
                object_path: object.into(),
                device_path: node.into(),
            },
            block: BlockInfo {
                device_path: node.into(),
                id_type: Some("ext4".into()),
                mount_points: mounted.iter().map(PathBuf::from).collect(),
                partition_number: Some(1),
                ..BlockInfo::default()
            },
            drive: drive.map(|drive| DriveInfo {
                object_path: drive.into(),
                removable: true,
                connection_bus: "usb".into(),
                ..DriveInfo::default()
            }),
            mount_point: mounted.first().map(PathBuf::from),
        }
    }

    fn of(sources: &SystemSources, devices: &[PlatformDevice], drive: &str) -> SystemUse {
        SystemDrives::find(devices, Some(sources)).of_drive(Some(drive))
    }

    #[test]
    fn only_a_person_s_media_folders_leave_a_drive_free() {
        for point in [
            "/media/me/STICK",
            "/run/media/me/STICK",
            "/mnt/better-os/AB12",
        ] {
            assert!(
                is_person_media(Path::new(point), &People::Anyone),
                "{point}"
            );
        }
        for point in [
            "/",
            "/media",
            "/mnt/better-os",
            "/srv",
            "/mnt/usb",
            "/home/me/usb",
        ] {
            assert!(
                !is_person_media(Path::new(point), &People::Anyone),
                "{point}"
            );
        }
    }

    /// Second review: only `/media/<account>/…` is a person's. A bare
    /// `/media/backup` (a mount unit, an autofs map) or a name that is no
    /// account in the login range is the system's.
    #[test]
    fn only_an_account_s_own_media_folder_is_a_person_s() {
        let people = People::Listed(vec!["me".into()]);
        assert!(is_person_media(Path::new("/media/me/STICK"), &people));
        assert!(is_person_media(Path::new("/run/media/me/STICK"), &people));
        for point in [
            "/media/backup",
            "/media/data",
            "/media/backup/disk1",
            "/run/media/data",
        ] {
            assert!(!is_person_media(Path::new(point), &people), "{point}");
        }
        let stick = volume("/b/sdc1", "/dev/sdc1", Some("/d/stick"), &[]);
        let devices = [stick];
        let mut sources = SystemSources::parse("/dev/sdc1 /media/data ext4 rw 0 0\n", "");
        sources.people = people.clone();
        // `parse` judged with anyone; read with these people the mount is held.
        let held = SystemSources::from_tables(
            &parse_mounts("/dev/sdc1 /media/data ext4 rw 0 0\n"),
            &[],
            Vec::new(),
            people,
            |_, _, _| Vec::new(),
        );
        assert_eq!(
            of(&held, &devices, "/d/stick"),
            SystemUse::SystemFolder {
                path: "/media/data".into()
            }
        );
        let _ = sources;
        // The login range comes from login.defs, 1000..=60000 without one.
        let etc = tempfile::tempdir().unwrap();
        assert_eq!(
            People::system(etc.path()),
            People::System {
                uid_min: 1000,
                uid_max: 60000
            }
        );
        std::fs::write(etc.path().join("login.defs"), "UID_MIN\t500\nUID_MAX 900\n").unwrap();
        assert_eq!(
            People::system(etc.path()),
            People::System {
                uid_min: 500,
                uid_max: 900
            }
        );
        // root (uid 0) is never in a login range.
        assert!(!People::system(etc.path()).knows("root"));
    }

    /// Second review: fstab may quote a tag, or name a `/dev/disk/by-*` link.
    #[test]
    fn quoted_tags_and_by_links_name_the_drive() {
        let mut data = volume("/b/sdc1", "/dev/sdc1", Some("/d/data"), &[]);
        data.block.id_uuid = Some("ABCD-1234".into());
        data.block.id_label = Some("My Data".into());
        data.block.partition_uuid = Some("0f0f-01".into());
        data.block.symlinks = vec!["/dev/disk/by-partlabel/backup\\x20part".into()];
        for spec in [
            "UUID=\"abcd-1234\"",
            "UUID='ABCD-1234'",
            "/dev/disk/by-uuid/ABCD-1234",
            "LABEL=\"My Data\"",
            "/dev/disk/by-label/My\\x20Data",
            "PARTUUID=\"0F0F-01\"",
            "/dev/disk/by-partuuid/0f0f-01",
            "PARTLABEL=\"backup part\"",
        ] {
            assert!(fstab_names(spec, &data), "{spec}");
        }
        assert!(!fstab_names("UUID=\"ffff-0000\"", &data));
        assert_eq!(
            parse_fstab("UUID=\"abcd-1234\" /data ext4 defaults 0 2\n"),
            ["UUID=abcd-1234"]
        );
    }

    /// Second review: a systemd mount unit (or the unit an automount stands
    /// for) under /etc or /run/systemd/system is a table entry; the units
    /// systemd generates from fstab live elsewhere and are not read twice.
    #[test]
    fn mount_units_are_read_as_table_entries() {
        let (root, roots) = tree();
        std::fs::write(roots.proc.join("self/mounts"), "").unwrap();
        let etc_units = root.path().join("etc/systemd/system");
        let run_units = root.path().join("run/systemd/system");
        std::fs::create_dir_all(&etc_units).unwrap();
        std::fs::create_dir_all(&run_units).unwrap();
        std::fs::write(
            etc_units.join("media-backup.mount"),
            "[Mount]\nWhat=/dev/disk/by-uuid/BACC-0001\nWhere=/media/backup\n",
        )
        .unwrap();
        std::fs::write(
            run_units.join("srv-data.automount"),
            "[Automount]\nWhere=/srv/data\n",
        )
        .unwrap();
        std::fs::write(
            run_units.join("srv-data.mount"),
            "[Mount]\nWhat=LABEL=\"Data\"\nWhere=/srv/data\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("etc/fstab"),
            "LABEL=Data /srv/data ext4 0 0\n",
        )
        .unwrap();
        let read = SystemSources::read(&roots).unwrap();
        assert_eq!(
            read.fstab,
            ["LABEL=Data", "/dev/disk/by-uuid/BACC-0001"],
            "the fstab line and the unit for it are one entry"
        );
        let mut backup = volume("/b/sdd1", "/dev/sdd1", Some("/d/backup"), &[]);
        backup.block.id_uuid = Some("BACC-0001".into());
        assert_eq!(of(&read, &[backup], "/d/backup"), SystemUse::SystemTable);
    }

    /// Second review: a system path mounted from a partition of a loop
    /// device is traced through the loop device's backing file.
    #[test]
    fn a_loop_partition_is_traced_through_its_loop_device() {
        assert_eq!(loop_parent("loop0p1").as_deref(), Some("loop0"));
        assert_eq!(loop_parent("loop12p3").as_deref(), Some("loop12"));
        assert_eq!(loop_parent("loop0"), None);
        assert_eq!(loop_parent("looper"), None);
        let (root, roots) = tree();
        let image = root.path().join("disk.img");
        std::fs::write(&image, b"img").unwrap();
        let (major, minor) = device_of_file(&image).unwrap();
        let mut mountinfo = "40 1 259:9 / /opt/tools ro - ext4 /dev/loop3p1 ro\n".to_string();
        if major == 0 {
            mountinfo.push_str(&format!(
                "41 1 {major}:{minor} / /media/me/STICK rw - btrfs /dev/sdb1 rw\n"
            ));
        } else {
            std::fs::create_dir_all(roots.sys.join("dev/block")).unwrap();
            std::os::unix::fs::symlink(
                "../../devices/usb/block/sdb/sdb1",
                roots.sys.join(format!("dev/block/{major}:{minor}")),
            )
            .unwrap();
        }
        std::fs::write(roots.proc.join("self/mountinfo"), mountinfo).unwrap();
        std::fs::create_dir_all(roots.sys.join("class/block/loop3/loop")).unwrap();
        std::fs::write(
            roots.sys.join("class/block/loop3/loop/backing_file"),
            format!("{}\n", image.display()),
        )
        .unwrap();
        let read = SystemSources::read(&roots).unwrap();
        let devices = [volume("/b/sdb1", "/dev/sdb1", Some("/d/usb"), &[])];
        let system = SystemDrives::find(&devices, Some(&read));
        assert_eq!(
            system.of_drive(Some("/d/usb")),
            SystemUse::SystemFolder {
                path: "/opt/tools".into()
            }
        );
        assert_eq!(system.of_drive(Some("/d/other")), SystemUse::Free);
    }

    #[test]
    fn a_mount_anywhere_the_system_keeps_things_holds_its_drive() {
        let stick = volume("/b/sdc1", "/dev/sdc1", Some("/d/stick"), &[]);
        let devices = [stick];
        for point in [
            "/home/other",
            "/srv",
            "/opt",
            "/nix",
            "/var/lib/docker",
            "/var/lib/flatpak",
            "/var/log",
            "/tmp",
            "/mnt/usb",
            "/isodevice",
        ] {
            let sources = SystemSources::parse(&format!("/dev/sdc1 {point} ext4 rw 0 0\n"), "");
            assert_eq!(
                of(&sources, &devices, "/d/stick"),
                SystemUse::SystemFolder { path: point.into() },
                "{point}"
            );
        }
        let sources = SystemSources::parse("/dev/sdc1 /media/me/STICK vfat rw 0 0\n", "");
        assert_eq!(of(&sources, &devices, "/d/stick"), SystemUse::Free);
    }

    #[test]
    fn the_running_system_on_a_usb_drive_holds_every_volume_on_that_drive() {
        let devices = [
            volume("/b/sdb1", "/dev/sdb1", Some("/d/usb"), &["/boot/efi"]),
            volume("/b/sdb2", "/dev/sdb2", Some("/d/usb"), &["/"]),
            volume("/b/sdb3", "/dev/sdb3", Some("/d/usb"), &["/media/me/DATA"]),
            volume(
                "/b/sdc1",
                "/dev/sdc1",
                Some("/d/stick"),
                &["/media/me/STICK"],
            ),
        ];
        let sources = SystemSources::parse(
            "/dev/sdb2 / ext4 rw 0 0\n/dev/sdb1 /boot/efi vfat rw 0 0\n/dev/sdb3 /media/me/DATA ext4 rw 0 0\n/dev/sdc1 /media/me/STICK vfat rw 0 0\n",
            "",
        );
        let system = SystemDrives::find(&devices, Some(&sources));
        assert_eq!(
            system.of_device(&devices[2], &devices),
            SystemUse::RunningSystem
        );
        assert_eq!(system.of_device(&devices[3], &devices), SystemUse::Free);
    }

    #[test]
    fn swap_partitions_hold_their_drive_and_memory_swap_is_nothing() {
        let devices = [
            volume("/b/nvme", "/dev/nvme0n1p2", Some("/d/nvme"), &["/"]),
            volume("/b/sdc1", "/dev/sdc1", Some("/d/swap"), &[]),
        ];
        let sources = SystemSources::parse(
            "/dev/nvme0n1p2 / ext4 rw 0 0\n",
            "Filename Type Size Used Priority\n/dev/sdc1 partition 1 0 -2\n/dev/zram0 partition 4 0 100\n",
        );
        assert_eq!(of(&sources, &devices, "/d/swap"), SystemUse::Swap);
    }

    #[test]
    fn an_fstab_line_outside_better_os_s_block_holds_a_drive_that_is_not_mounted() {
        let mut data = volume("/b/sdc1", "/dev/sdc1", Some("/d/data"), &[]);
        data.block.id_uuid = Some("ABCD-1234".into());
        let mut ours = volume("/b/sdd1", "/dev/sdd1", Some("/d/ours"), &[]);
        ours.block.id_uuid = Some("FEED-0001".into());
        let devices = [data, ours];
        let fstab = format!(
            "UUID=abcd-1234 /data ext4 defaults,noauto 0 2\nproc /proc proc defaults 0 0\n{FSTAB_BEGIN}\nUUID=FEED-0001 /mnt/better-os/FEED-0001 vfat nofail 0 0\n{FSTAB_END}\n"
        );
        let sources = SystemSources::parse_with_fstab("", "", &fstab);
        assert_eq!(of(&sources, &devices, "/d/data"), SystemUse::SystemTable);
        assert_eq!(of(&sources, &devices, "/d/ours"), SystemUse::Free);
        assert_eq!(
            parse_fstab("LABEL=Backup /b ext4 0 0\n/dev/sdz1 /c ext4 0 0\n").len(),
            2
        );
    }

    #[test]
    fn an_unlocked_root_is_traced_through_its_container_and_a_link() {
        let container = volume("/b/sdb2", "/dev/sdb2", Some("/d/usb"), &[]);
        let mut cleartext = volume("/b/dm0", "/dev/dm-0", None, &[]);
        cleartext.block.crypto_backing = Some("/b/sdb2".into());
        cleartext.block.symlinks = vec!["/dev/mapper/luks-1234".into()];
        let devices = [container, cleartext];
        let sources = SystemSources::parse("/dev/mapper/luks-1234 / ext4 rw 0 0\n", "");
        assert_eq!(of(&sources, &devices, "/d/usb"), SystemUse::RunningSystem);
    }

    #[test]
    fn a_zfs_root_holds_the_drives_of_its_pool_and_only_those() {
        let mut member = volume("/b/sdb1", "/dev/sdb1", Some("/d/pool"), &[]);
        member.block.id_type = Some("zfs_member".into());
        member.block.id_label = Some("rpool".into());
        let stick = volume("/b/sdc1", "/dev/sdc1", Some("/d/stick"), &[]);
        let devices = [member, stick];
        let sources = SystemSources::parse(
            "28 1 0:25 / / rw - zfs rpool/ROOT/ubuntu_abc rw\n35 28 0:26 / /home rw - zfs rpool/USERDATA/home rw\n",
            "",
        );
        assert_eq!(of(&sources, &devices, "/d/pool"), SystemUse::RunningSystem);
        assert_eq!(of(&sources, &devices, "/d/stick"), SystemUse::Free);
        // A pool UDisks2 does not list: the drives cannot be told.
        let other = SystemSources::parse("28 1 0:25 / / rw - zfs bpool/BOOT rw\n", "");
        assert_eq!(of(&other, &devices, "/d/stick"), SystemUse::Unknown);
    }

    #[test]
    fn a_bcachefs_root_holds_every_member() {
        let devices = [
            volume("/b/sdb1", "/dev/sdb1", Some("/d/one"), &[]),
            volume("/b/sdc1", "/dev/sdc1", Some("/d/two"), &[]),
            volume("/b/sdd1", "/dev/sdd1", Some("/d/stick"), &[]),
        ];
        let sources =
            SystemSources::parse("28 1 0:30 / / rw - bcachefs /dev/sdb1:/dev/sdc1 rw\n", "");
        assert_eq!(of(&sources, &devices, "/d/one"), SystemUse::RunningSystem);
        assert_eq!(of(&sources, &devices, "/d/two"), SystemUse::RunningSystem);
        assert_eq!(of(&sources, &devices, "/d/stick"), SystemUse::Free);
    }

    #[test]
    fn what_cannot_be_told_is_unknown_and_never_free() {
        let stick = volume("/b/sdc1", "/dev/sdc1", Some("/d/stick"), &["/media/me/S"]);
        let devices = [stick];
        assert_eq!(
            SystemDrives::find(&devices, None).of_drive(Some("/d/stick")),
            SystemUse::Unknown
        );
        // A root UDisks2 does not list: any drive might hold it.
        let sources = SystemSources::parse("/dev/sda2 / ext4 rw 0 0\n", "");
        assert_eq!(of(&sources, &devices, "/d/stick"), SystemUse::Unknown);
        // A loop device whose file was not traced (no `/sys` read).
        let looped = SystemSources::parse("/dev/loop0 /rofs squashfs ro 0 0\n", "");
        assert_eq!(of(&looped, &devices, "/d/stick"), SystemUse::Unknown);
        // A swap file whose filesystem was not traced.
        let file = SystemSources::parse("", "Filename Type\n/swap.img file 1 0 -2\n");
        assert_eq!(of(&file, &devices, "/d/stick"), SystemUse::Unknown);
        // An overlay root (a live session) names no drive and holds none.
        let live = SystemSources::parse("overlay / overlay rw 0 0\n", "");
        assert_eq!(of(&live, &devices, "/d/stick"), SystemUse::Free);
        let complete = SystemDrives::find(&devices, Some(&SystemSources::default()));
        assert_eq!(complete.of_drive(None), SystemUse::Unknown);
        assert_eq!(complete.of_drive(Some("/d/stick")), SystemUse::Free);
        assert!(!SystemUse::Unknown.permits_removal());
    }

    #[test]
    fn a_btrfs_root_across_two_drives_holds_both() {
        let mut first = volume("/b/sdb1", "/dev/sdb1", Some("/d/one"), &["/"]);
        let mut second = volume("/b/sdc1", "/dev/sdc1", Some("/d/two"), &[]);
        for device in [&mut first, &mut second] {
            device.block.id_type = Some("btrfs".into());
            device.block.id_uuid = Some("same".into());
        }
        let devices = [first, second];
        let system = SystemDrives::find(&devices, Some(&SystemSources::default()));
        assert_eq!(system.of_drive(Some("/d/two")), SystemUse::RunningSystem);
    }

    #[test]
    fn a_refusal_names_its_reason_and_reads_back() {
        let home = SystemUse::SystemFolder {
            path: "/home".into(),
        };
        assert_eq!(
            home.refusal_detail().as_deref(),
            Some("storage.system.folder:/home")
        );
        assert_eq!(
            SystemUse::from_refusal("storage.system.folder:/home"),
            Some(home)
        );
        assert_eq!(
            SystemUse::from_refusal("storage.system.table"),
            Some(SystemUse::SystemTable)
        );
        assert_eq!(SystemUse::from_refusal("storage.erase.busy"), None);
        assert_eq!(SystemUse::Free.refusal(), None);
    }

    /// A fixture tree for [`SystemSources::read`]: `proc`, `sys`, `etc`.
    fn tree() -> (tempfile::TempDir, Roots) {
        let root = tempfile::tempdir().unwrap();
        let roots = Roots::at(root.path());
        std::fs::create_dir_all(roots.proc.join("self")).unwrap();
        std::fs::create_dir_all(root.path().join("etc")).unwrap();
        (root, roots)
    }

    #[test]
    fn slaves_are_followed_in_a_fixture_tree() {
        let (_root, roots) = tree();
        std::fs::write(
            roots.proc.join("self/mounts"),
            "/dev/dm-7 / ext4 rw 0 0\n/dev/dm-8 /mnt/better-os/x ext4 rw 0 0\n",
        )
        .unwrap();
        std::fs::create_dir_all(roots.sys.join("class/block/dm-7/slaves/dm-6")).unwrap();
        std::fs::create_dir_all(roots.sys.join("class/block/dm-6/slaves/sdz3")).unwrap();
        let read = SystemSources::read(&roots).unwrap();
        assert_eq!(read.sources.len(), 1);
        assert_eq!(
            read.sources[0].nodes,
            ["/dev/dm-7", "/dev/dm-6", "/dev/sdz3"]
        );
        assert_eq!(
            SystemSources::read(&Roots::at(_root.path().join("none"))),
            None
        );
    }

    #[test]
    fn a_source_udisks_does_not_know_is_found_by_its_device_number() {
        let (_root, roots) = tree();
        // `/dev/root` at 8:18; `/sys/dev/block/8:18` is sdb2.
        std::fs::write(
            roots.proc.join("self/mountinfo"),
            "22 1 8:18 / / rw - ext4 /dev/root rw\n",
        )
        .unwrap();
        std::fs::create_dir_all(roots.sys.join("dev/block")).unwrap();
        std::os::unix::fs::symlink(
            "../../devices/pci0000:00/usb1/1-1/block/sdb/sdb2",
            roots.sys.join("dev/block/8:18"),
        )
        .unwrap();
        let read = SystemSources::read(&roots).unwrap();
        assert!(
            read.sources[0].nodes.contains(&"/dev/sdb2".to_string()),
            "{read:?}"
        );
        let devices = [volume("/b/sdb2", "/dev/sdb2", Some("/d/usb"), &[])];
        assert_eq!(of(&read, &devices, "/d/usb"), SystemUse::RunningSystem);
    }

    #[test]
    fn a_loop_device_and_a_swap_file_are_traced_to_the_drive_their_file_is_on() {
        let (root, roots) = tree();
        // The backing file and the swap file are real files in the tree;
        // their device is whatever the temporary directory is on.
        let iso = root.path().join("ubuntu.iso");
        let swap = root.path().join("swapfile");
        std::fs::write(&iso, b"iso").unwrap();
        std::fs::write(&swap, b"swap").unwrap();
        let (major, minor) = device_of_file(&iso).unwrap();
        // The kernel names that device: a link under /sys/dev/block when it
        // is a block device, else the mount with that anonymous number.
        let mut mountinfo = "40 1 7:0 / /rofs ro - squashfs /dev/loop0 ro\n".to_string();
        if major == 0 {
            mountinfo.push_str(&format!(
                "41 1 {major}:{minor} / /isodevice rw - btrfs /dev/sdb1 rw\n"
            ));
        } else {
            std::fs::create_dir_all(roots.sys.join("dev/block")).unwrap();
            std::os::unix::fs::symlink(
                "../../devices/usb/block/sdb/sdb1",
                roots.sys.join(format!("dev/block/{major}:{minor}")),
            )
            .unwrap();
        }
        std::fs::write(roots.proc.join("self/mountinfo"), mountinfo).unwrap();
        std::fs::create_dir_all(roots.sys.join("class/block/loop0/loop")).unwrap();
        std::fs::write(
            roots.sys.join("class/block/loop0/loop/backing_file"),
            format!("{}\n", iso.display()),
        )
        .unwrap();
        std::fs::write(
            roots.proc.join("swaps"),
            format!(
                "Filename Type Size Used Priority\n{} file 4 0 -2\n",
                swap.display()
            ),
        )
        .unwrap();
        let read = SystemSources::read(&roots).unwrap();
        let devices = [volume("/b/sdb1", "/dev/sdb1", Some("/d/usb"), &[])];
        let system = SystemDrives::find(&devices, Some(&read));
        assert_eq!(
            system.of_drive(Some("/d/usb")),
            SystemUse::SystemFolder {
                path: "/rofs".into()
            }
        );
        // Every source was traced, so another drive is free.
        assert_eq!(system.of_drive(Some("/d/other")), SystemUse::Free);
        let swap_only = read
            .sources
            .iter()
            .find(|source| source.held == SystemUse::Swap)
            .unwrap();
        assert!(
            swap_only.nodes.contains(&"/dev/sdb1".to_string()),
            "{read:?}"
        );
    }

    /// Reads the running host (read-only) and prints what it holds: every
    /// held mount and swap must name some node, or every drive would be
    /// unknown on this machine.
    #[test]
    #[ignore = "reads the running host's /proc, /sys and /etc/fstab"]
    fn the_host_names_a_node_for_every_held_mount() {
        let read = SystemSources::read(&Roots::system()).expect("a mount table");
        for source in &read.sources {
            eprintln!("{:?} {:?} {:?}", source.held, source.nodes, source.zfs_pool);
            assert!(
                !source.nodes.is_empty() || source.zfs_pool.is_some(),
                "{source:?}"
            );
        }
        eprintln!("fstab: {:?}", read.fstab);
    }

    #[test]
    fn the_fstab_beside_proc_is_read() {
        let (root, roots) = tree();
        std::fs::write(roots.proc.join("self/mounts"), "").unwrap();
        std::fs::write(root.path().join("etc/fstab"), "UUID=X /srv ext4 0 0\n").unwrap();
        assert_eq!(SystemSources::read(&roots).unwrap().fstab, ["UUID=X"]);
    }
}
