//! Every drive a person would call one, with how full it is (ticket 122), as
//! Windows 11 File Explorer's "This PC" shows them.
//!
//! **Which drives.** UDisks2's block devices (the system drive, other
//! internal partitions mounted or not, removable drives, optical media),
//! GVfs's phones, cameras and servers, and mounted network shares. Not loop
//! devices, snaps, swap, `tmpfs`, boot, EFI or recovery partitions, or any
//! other mount the system keeps for itself. Without UDisks2 the mount table
//! alone answers, with the drives it can see.
//!
//! **Nothing here blocks the window.** [`DriveScanner`] reads UDisks2, the
//! mount table, GVfs's folder and every drive's free space on a thread of its
//! own; a drive whose filesystem does not answer within
//! [`CAPACITY_TIMEOUT`] (a stalled network share) is listed without a bar.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use files_platform::MountTable;
use storage_platform::model::PlatformDevice;

// The listing itself is shared with Better Manager's Drives section (ticket
// 127): which block devices are drives, and their choices.
pub use drive_mounts::listing::{
    Capacity, Drive, DriveKind, DriveName, NEARLY_FULL, RemovableForm, SystemUse,
    drives_from_devices, drives_with_system, is_system_mount, mount_points_of, removable_form,
    running_system_name,
};
use storage_platform::system_use::{SystemDrives, SystemSources};

/// How long a drive's free space is waited for before it is shown without a
/// bar.
pub const CAPACITY_TIMEOUT: Duration = Duration::from_secs(1);

/// How often the drives are read again while the window is open.
pub const DRIVES_REFRESH: Duration = Duration::from_secs(30);

/// A drive's words, in the window's language.
pub trait DriveText {
    /// What the row says: its label, or "256 GB drive".
    fn name(&self, c: &crate::i18n::Copy) -> String;
    /// The line under the name.
    fn caption(&self, c: &crate::i18n::Copy) -> Option<String>;
}

/// A capacity's words.
pub trait CapacityText {
    fn line(&self, c: &crate::i18n::Copy) -> String;
}

impl CapacityText for Capacity {
    /// "12.3 GB free of 256 GB", in the window's language.
    fn line(&self, c: &crate::i18n::Copy) -> String {
        c.drive_free_of
            .replace("{free}", &crate::format::bytes(self.free))
            .replace("{total}", &crate::format::bytes(self.total))
    }
}

/// A drive's name in the window's language (ticket 138): the running
/// system's drive by the system's name, any other by its label or its size.
pub fn name_words(name: DriveName<'_>, c: &crate::i18n::Copy) -> String {
    match name {
        DriveName::System(name) | DriveName::Label(name) => name.to_string(),
        DriveName::UnnamedSystem => c.drive_system_unnamed.to_string(),
        DriveName::Size(size) => c
            .drive_of_size
            .replace("{size}", &crate::format::bytes(size)),
    }
}

/// Whether the storage service's `device` is this drive: the same UDisks2
/// block, or the same mount point. The sidebar draws such a pair as one row
/// named by the drive, so everything else that names the device (its toasts,
/// its dialogs) finds the drive the same way.
pub fn is_device(drive: &Drive, device: &crate::devices::DeviceRow) -> bool {
    drive.object_path.as_deref() == Some(device.object_path.as_str())
        || (drive.mount_point.is_some() && drive.mount_point == device.mount_point)
}

/// What of the running system a sidebar row's drive holds (ticket 171): the
/// one answer the menu, Eject and Format all ask. A row the drive listing
/// and the storage service both know is held when either says so; one
/// neither has worked out is unknown. Only `Free` is ever ejected,
/// unmounted or formatted.
pub fn system_use_of(
    drive: Option<&Drive>,
    device: Option<&crate::devices::DeviceRow>,
) -> SystemUse {
    match (drive, device) {
        (Some(drive), Some(device)) => drive
            .system_use
            .clone()
            .strongest(device.system_use.clone()),
        (Some(drive), None) => drive.system_use.clone(),
        (None, Some(device)) => device.system_use.clone(),
        (None, None) => SystemUse::Unknown,
    }
}

/// What the drive the running system is on is called: the root crumb of the
/// path bar names it the way the sidebar does.
pub fn system_drive_name(system: Option<&str>, c: &crate::i18n::Copy) -> String {
    name_words(DriveName::of_system(system), c)
}

impl DriveText for Drive {
    fn name(&self, c: &crate::i18n::Copy) -> String {
        name_words(self.naming(), c)
    }

    /// The line under the name: how full it is, or that it is not mounted
    /// or still locked, with its size unless the name already is its size;
    /// nothing when it did not answer.
    fn caption(&self, c: &crate::i18n::Copy) -> Option<String> {
        let named_by_size = matches!(self.naming(), DriveName::Size(_));
        let state = |state: &str| {
            if !named_by_size && self.size > 0 {
                format!("{} · {state}", crate::format::bytes(self.size))
            } else {
                state.to_string()
            }
        };
        let auto = self.auto_mount.as_ref();
        match (&self.capacity, &self.mount_point) {
            (Some(capacity), _) => Some(capacity.line(c)),
            (None, None) if self.locked => Some(state(c.drive_locked)),
            // On and not mounted: why, with Retry on the row (ticket 127).
            // Without its size: the reason is what the narrow row has room for.
            (None, None) if drive_mounts::choice::failed(self) => Some(
                match auto.and_then(|auto| auto.failure.as_deref()) {
                    Some("timeout") => c.drive_mount_timeout,
                    Some(_) => c.drive_mount_refused,
                    None => c.drive_mount_failed,
                }
                .to_string(),
            ),
            (None, None) if auto.is_some_and(|auto| !auto.on) => Some(state(c.drive_off)),
            (None, None) if self.size > 0 => Some(state(c.drive_not_mounted)),
            _ => None,
        }
    }
}

/// Network filesystems the kernel mounts itself.
const NETWORK_FILESYSTEMS: &[&str] = &["cifs", "smb3", "nfs", "nfs4", "fuse.sshfs", "davfs"];

/// The drives the mount table shows that UDisks2 does not: network shares,
/// and, without UDisks2 (`devices_known` false), the system drive and the
/// media mounted under `/media`, `/run/media` and `/mnt`.
pub fn drives_from_mounts(mounts: &MountTable, devices_known: bool) -> Vec<Drive> {
    let mut drives = Vec::new();
    for mount in mounts.mounts() {
        let point = &mount.mount_point;
        let network = NETWORK_FILESYSTEMS.contains(&mount.filesystem.as_str());
        let block = mount.source.starts_with("/dev/") && !mount.source.starts_with("/dev/loop");
        let kind = if network {
            DriveKind::Network
        } else if devices_known || !block || is_system_mount(point) {
            continue;
        } else if point == Path::new("/") {
            DriveKind::System
        } else if mount.is_probably_external() {
            DriveKind::Removable
        } else {
            continue;
        };
        if drives
            .iter()
            .any(|drive: &Drive| drive.mount_point.as_ref() == Some(point))
        {
            continue;
        }
        drives.push(Drive {
            // Without UDisks2 nothing says which drive a mount is on: the
            // system's own is the system's, a share holds nothing, and a
            // medium under /media cannot be told.
            system_use: match kind {
                DriveKind::System => SystemUse::RunningSystem,
                DriveKind::Network => SystemUse::Free,
                _ => SystemUse::Unknown,
            },
            key: point.to_string_lossy().into_owned(),
            label: point
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|_| point != Path::new("/")),
            kind,
            mount_point: Some(point.clone()),
            object_path: None,
            device_path: None,
            drive_object: None,
            backing: None,
            locked: false,
            uuid: None,
            size: 0,
            ejectable: false,
            capacity: None,
            auto_mount: None,
            form: None,
        });
    }
    drives
}

/// The phones, cameras and servers GVfs has mounted: the folders directly
/// inside its own FUSE mount (`/run/user/<uid>/gvfs/mtp:host=…`).
pub fn drives_from_gvfs(children: &[PathBuf]) -> Vec<Drive> {
    children
        .iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let (scheme, rest) = name.split_once(':')?;
            let kind = match scheme {
                "mtp" | "gphoto2" | "afc" => DriveKind::Phone,
                "smb-share" | "sftp" | "dav" | "davs" | "ftp" | "ftps" | "nfs" | "afp-volume" => {
                    DriveKind::Network
                }
                _ => return None,
            };
            // `host=server,share=photos` names itself by its host and share.
            let label = rest
                .split(',')
                .filter_map(|pair| pair.split_once('='))
                .filter(|(key, _)| matches!(*key, "host" | "share"))
                .map(|(_, value)| percent_decode(value))
                .collect::<Vec<_>>()
                .join(" · ");
            Some(Drive {
                // A phone or a server GVfs mounted holds nothing of the
                // running system.
                system_use: SystemUse::Free,
                key: path.to_string_lossy().into_owned(),
                label: (!label.is_empty()).then_some(label),
                kind,
                mount_point: Some(path.clone()),
                object_path: None,
                device_path: None,
                drive_object: None,
                backing: None,
                locked: false,
                uuid: None,
                size: 0,
                ejectable: false,
                capacity: None,
                auto_mount: None,
                form: None,
            })
        })
        .collect()
}

/// `%20` and the like in a GVfs folder name, decoded; anything malformed is
/// kept as it is.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = text.get(index + 1..index + 3)
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The order a person reads them in: the system drive, internal, removable,
/// optical, phones, network; by name within each.
pub fn sort_drives(drives: &mut [Drive]) {
    let rank = |kind: DriveKind| match kind {
        DriveKind::System => 0,
        DriveKind::Internal => 1,
        DriveKind::Removable => 2,
        DriveKind::Optical => 3,
        DriveKind::Phone => 4,
        DriveKind::Network => 5,
    };
    drives.sort_by(|left, right| {
        rank(left.kind)
            .cmp(&rank(right.kind))
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.key.cmp(&right.key))
    });
}

/// A filesystem's size and free space, from `statvfs`. Reads the disk.
pub fn statvfs(path: &Path) -> Option<Capacity> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut raw: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid NUL-terminated string and `raw` a valid,
    // writable `statvfs` the call fills in.
    if unsafe { libc::statvfs(path.as_ptr(), &mut raw) } != 0 {
        return None;
    }
    let block = raw.f_frsize as u64;
    Some(Capacity {
        total: raw.f_blocks as u64 * block,
        free: raw.f_bavail as u64 * block,
    })
}

/// A capacity read in flight, which anyone asking for the same filesystem
/// meanwhile waits on too.
#[derive(Default)]
struct Pending {
    result: Mutex<Option<Option<Capacity>>>,
    done: std::sync::Condvar,
}

/// Filesystems whose read has not come back yet. A stalled share is not
/// asked again until it answers, so its reads do not pile up a thread per
/// refresh; two windows reading the same drive at once share one read.
static WAITING: Mutex<Vec<(PathBuf, Arc<Pending>)>> = Mutex::new(Vec::new());

/// Whether a read of `path` is still in flight. For tests.
#[cfg(test)]
fn is_waiting(path: &Path) -> bool {
    WAITING
        .lock()
        .is_ok_and(|waiting| waiting.iter().any(|(known, _)| known == path))
}

/// The read in flight for `path`, or a new one started on a thread of its
/// own.
fn read_of(path: &Path, read: &CapacityRead) -> Option<Arc<Pending>> {
    let mut waiting = WAITING.lock().ok()?;
    if let Some((_, pending)) = waiting.iter().find(|(known, _)| known == path) {
        return Some(Arc::clone(pending));
    }
    let pending = Arc::new(Pending::default());
    waiting.push((path.to_path_buf(), Arc::clone(&pending)));
    drop(waiting);
    let (reading, read, filling) = (path.to_path_buf(), Arc::clone(read), Arc::clone(&pending));
    let spawned = std::thread::Builder::new()
        .name("files-drive-capacity".to_string())
        .spawn(move || {
            let capacity = read(&reading);
            if let Ok(mut waiting) = WAITING.lock() {
                waiting.retain(|(known, _)| *known != reading);
            }
            if let Ok(mut result) = filling.result.lock() {
                *result = Some(capacity);
            }
            filling.done.notify_all();
        });
    if spawned.is_err() {
        if let Ok(mut waiting) = WAITING.lock() {
            waiting.retain(|(known, _)| known != path);
        }
        return None;
    }
    Some(pending)
}

/// Reads drives' capacities, each on a thread of its own, all at once, and
/// waits at most `timeout` for the lot: a filesystem that does not answer is
/// left waiting on its thread, never on the caller's, and reads `None`.
pub fn capacities_within(
    paths: &[PathBuf],
    timeout: Duration,
    read: CapacityRead,
) -> Vec<Option<Capacity>> {
    let deadline = std::time::Instant::now() + timeout;
    let reads: Vec<Option<Arc<Pending>>> = paths.iter().map(|path| read_of(path, &read)).collect();
    reads
        .into_iter()
        .map(|pending| {
            let pending = pending?;
            let mut result = pending.result.lock().ok()?;
            while result.is_none() {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    // Timed out: no bar.
                    return None;
                }
                result = pending.done.wait_timeout(result, left).ok()?.0;
            }
            result.flatten()
        })
        .collect()
}

/// One drive's capacity, as [`capacities_within`] reads it.
pub fn capacity_within(path: &Path, timeout: Duration, read: CapacityRead) -> Option<Capacity> {
    capacities_within(&[path.to_path_buf()], timeout, read)
        .pop()
        .flatten()
}

/// Reads a mounted filesystem's capacity; it may block.
pub type CapacityRead = Arc<dyn Fn(&Path) -> Option<Capacity> + Send + Sync>;

/// Unlocks an encrypted drive with a passphrase and says where what is
/// inside was mounted.
pub type DriveUnlock =
    Arc<dyn Fn(&Drive, &crate::secret::Secret) -> Result<PathBuf, UnlockError> + Send + Sync>;

/// Mounts a drive, off the window's thread: where, or why it could not.
pub type DriveMount = Arc<dyn Fn(&Drive) -> Result<PathBuf, String> + Send + Sync>;

/// Ejects a drive, off the window's thread.
pub type DriveEject = Arc<dyn Fn(&Drive) -> Result<Ejected, EjectError> + Send + Sync>;

/// Blocks, sending on the channel each time a drive may have come, gone,
/// mounted or unmounted, until the flag is set or the channel is closed.
pub type DriveChanges = Arc<dyn Fn(Sender<()>, Arc<AtomicBool>) + Send + Sync>;

/// How an eject ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Ejected {
    /// Every volume on it is unmounted and it is powered off (or cannot
    /// be): safe to remove.
    Safe,
    /// Every volume on it is unmounted, but powering it off (or opening a
    /// disc's tray) was refused, in UDisks2's words.
    NotPoweredOff(String),
}

/// Why an eject stopped before the drive was safe to remove.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EjectError {
    /// Another volume on the same drive is still in use; nothing was
    /// powered off.
    InUse { volume: String, detail: String },
    /// Anything else, in UDisks2's words.
    Failed(String),
}

/// Why an encrypted drive did not open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnlockError {
    /// The passphrase was wrong: asked again.
    WrongPassphrase,
    /// The person dismissed the system's authentication prompt: nothing is
    /// said.
    Dismissed,
    /// UDisks2 is not there.
    Unavailable,
    /// It did not unlock, in UDisks2's words.
    Failed(String),
    /// It unlocked, but what is inside did not mount.
    MountFailed(String),
}

/// Unmounts one volume, leaving the drive powered.
pub type DriveUnmount = Arc<dyn Fn(&Drive) -> Result<(), String> + Send + Sync>;
/// Reads `/etc/fstab` and Better OS's udev rules.
pub type SystemFilesRead = Arc<dyn Fn() -> drive_mounts::choice::SystemFiles + Send + Sync>;
/// Why an internal drive that is on did not mount.
pub type MountFailureRead = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;
/// Asks the daemon to turn a drive's choice on or off.
pub type SetAutoMount = Arc<dyn Fn(&str, bool) -> drive_mounts::client::SetOutcome + Send + Sync>;

/// Where a scan gets what it reads. The real ones read UDisks2, the mount
/// table and GVfs's folder; tests give their own.
#[derive(Clone)]
pub struct DriveSources {
    /// UDisks2's block devices, or `None` when UDisks2 is not reachable.
    pub devices: Arc<dyn Fn() -> Option<Vec<PlatformDevice>> + Send + Sync>,
    pub mounts: Arc<dyn Fn() -> MountTable + Send + Sync>,
    /// The folders directly inside GVfs's FUSE mount.
    pub gvfs: Arc<dyn Fn() -> Vec<PathBuf> + Send + Sync>,
    pub capacity: CapacityRead,
    /// Mounts a drive through UDisks2 and says where. polkit may ask the
    /// person first.
    pub mount: DriveMount,
    /// Unlocks an encrypted drive with a passphrase and mounts what is in it.
    pub unlock: DriveUnlock,
    /// Unmounts every volume on the drive, locks what is encrypted, and
    /// powers it off where it can be.
    pub eject: DriveEject,
    /// What says the drives changed; `None` where nothing does.
    pub changes: Option<DriveChanges>,
    /// A fixed list instead of all of the above, for a sandbox's screenshots
    /// ([`FIXTURE_VARIABLE`]).
    pub fixture: Option<Arc<dyn Fn() -> Vec<Drive> + Send + Sync>>,
    /// Unmounts one volume through UDisks2, leaving the drive powered: a
    /// drive turned off that is mounted now (ticket 127).
    pub unmount: DriveUnmount,
    /// `/etc/fstab` and Better OS's udev rules, which say whether each drive
    /// mounts automatically.
    pub system_files: SystemFilesRead,
    /// Why an internal drive that is on did not mount, from systemd.
    pub mount_failure: MountFailureRead,
    /// Asks `manager-daemon` to turn a drive's choice on or off.
    pub set_auto_mount: SetAutoMount,
    /// What the running system has mounted and swapped (ticket 171): the
    /// kernel's own record on a machine, nothing in tests. `None` when it
    /// cannot be read, which makes every drive unknown.
    pub system: SystemSourcesRead,
}

/// Reads what the running system has mounted and swapped.
pub type SystemSourcesRead = Arc<dyn Fn() -> Option<SystemSources> + Send + Sync>;

pub use drive_mounts::listing::FIXTURE_VARIABLE;

/// The drives a fixture file names, in reading order.
pub fn fixture_drives(path: &Path) -> Vec<Drive> {
    let mut drives = drive_mounts::listing::fixture_drives(path);
    sort_drives(&mut drives);
    drives
}

#[zbus::proxy(
    interface = "org.freedesktop.UDisks2.Encrypted",
    default_service = "org.freedesktop.UDisks2"
)]
trait Encrypted {
    fn unlock(
        &self,
        passphrase: &str,
        options: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    fn lock(
        &self,
        options: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.UDisks2.Drive",
    default_service = "org.freedesktop.UDisks2"
)]
trait Tray {
    /// Opens a disc drive's tray.
    fn eject(
        &self,
        options: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;
}

/// One connection to UDisks2, made when first needed and made again after
/// it fails: at `address` (a private bus in tests), or on the system bus.
struct UdisksLink {
    address: Option<String>,
    connection: Mutex<Option<zbus::Connection>>,
}

impl UdisksLink {
    async fn connect(address: Option<&str>) -> Option<zbus::Connection> {
        match address {
            Some(address) => zbus::connection::Builder::address(address)
                .ok()?
                .build()
                .await
                .ok(),
            None => zbus::Connection::system().await.ok(),
        }
    }

    async fn connection(&self) -> Option<zbus::Connection> {
        let kept = self.connection.lock().ok()?.clone();
        if let Some(connection) = kept {
            return Some(connection);
        }
        let connection = Self::connect(self.address.as_deref()).await?;
        if let Ok(mut kept) = self.connection.lock() {
            *kept = Some(connection.clone());
        }
        Some(connection)
    }

    fn forget(&self) {
        if let Ok(mut kept) = self.connection.lock() {
            *kept = None;
        }
    }

    async fn client(&self) -> Result<storage_platform::UDisks2, String> {
        self.connection()
            .await
            .map(storage_platform::UDisks2::from_connection)
            .ok_or_else(|| "UDisks2 is not available".to_string())
    }
}

/// A drive's UDisks2 address, for the storage layer's calls.
fn address_of(drive: &Drive) -> Result<storage_platform::model::DeviceAddress, String> {
    Ok(storage_platform::model::DeviceAddress {
        object_path: drive
            .object_path
            .clone()
            .ok_or("the drive is not a UDisks2 device")?,
        device_path: drive.device_path.clone().unwrap_or_default(),
    })
}

/// The name a volume is called by in a message: its label, else its node.
fn volume_name(device: &PlatformDevice) -> String {
    device
        .block
        .id_label
        .clone()
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| device.address.device_path.clone())
}

/// Ejects a drive as Nautilus does: unmounts every volume on the same
/// drive (unlocked volumes first), locks every container it opened, then
/// opens a disc's tray or powers the drive off. A volume that will not
/// unmount stops it there, named, and nothing is powered off; a power-off
/// that is refused is reported, the volumes being unmounted all the same.
async fn eject_through(
    link: &UdisksLink,
    drive: &Drive,
    system: &SystemSourcesRead,
) -> Result<Ejected, EjectError> {
    use storage_platform::traits::DeviceControl;
    let failed = |detail: String| EjectError::Failed(detail);
    let udisks = link.client().await.map_err(failed)?;
    let devices = udisks
        .enumerate()
        .await
        .map_err(|error| failed(error.to_string()))?;
    let object = drive
        .object_path
        .clone()
        .ok_or_else(|| failed("the drive is not a UDisks2 device".to_string()))?;
    // Asked again here, of what UDisks2 and the kernel say now, whatever
    // the window decided (ticket 171).
    refuse_if_system(&devices, &object, system).map_err(failed)?;
    let on_drive = |device: &PlatformDevice| match &drive.drive_object {
        Some(drive_object) => {
            let own = device.drive.as_ref().map(|info| info.object_path.as_str());
            let through_backing = device.block.crypto_backing.as_deref().and_then(|backing| {
                devices
                    .iter()
                    .find(|other| other.address.object_path == backing)?
                    .drive
                    .as_ref()
                    .map(|info| info.object_path.as_str())
            });
            own == Some(drive_object.as_str()) || through_backing == Some(drive_object.as_str())
        }
        None => device.address.object_path == object,
    };
    let mut volumes: Vec<&PlatformDevice> =
        devices.iter().filter(|device| on_drive(device)).collect();
    // Unlocked volumes before the rest, so their containers can be locked.
    volumes.sort_by_key(|device| device.block.crypto_backing.is_none());
    for volume in &volumes {
        if mount_points_of(volume).is_empty() {
            continue;
        }
        if let Err(error) = udisks.unmount(&volume.address).await {
            return Err(EjectError::InUse {
                volume: volume_name(volume),
                detail: error.to_string(),
            });
        }
    }
    let connection = link
        .connection()
        .await
        .ok_or_else(|| failed("UDisks2 is not available".to_string()))?;
    for backing in volumes
        .iter()
        .filter_map(|volume| volume.block.crypto_backing.clone())
    {
        EncryptedProxy::builder(&connection)
            .path(backing)
            .map_err(|error| failed(error.to_string()))?
            .build()
            .await
            .map_err(|error| failed(error.to_string()))?
            .lock(std::collections::HashMap::new())
            .await
            .map_err(|error| failed(error.to_string()))?;
    }
    let Some(drive_object) = &drive.drive_object else {
        return Ok(Ejected::Safe);
    };
    if drive.kind == DriveKind::Optical {
        // A disc comes out of its tray rather than the drive powering off.
        let tray = TrayProxy::builder(&connection)
            .path(drive_object.clone())
            .map_err(|error| failed(error.to_string()))?
            .build()
            .await
            .map_err(|error| failed(error.to_string()))?;
        return Ok(match tray.eject(std::collections::HashMap::new()).await {
            Ok(()) => Ejected::Safe,
            Err(error) => Ejected::NotPoweredOff(error.to_string()),
        });
    }
    let proxy = storage_platform::udisks::DriveProxy::builder(&connection)
        .path(drive_object.clone())
        .map_err(|error| failed(error.to_string()))?
        .build()
        .await
        .map_err(|error| failed(error.to_string()))?;
    if !proxy.can_power_off().await.unwrap_or(false) {
        return Ok(Ejected::Safe);
    }
    Ok(
        match proxy.power_off(std::collections::HashMap::new()).await {
            Ok(()) => Ejected::Safe,
            Err(error) => Ejected::NotPoweredOff(error.to_string()),
        },
    )
}

/// Refuses a drive that holds the running system, or that cannot be told
/// apart from one, with the storage service's own key
/// (`storage.system.running_system`, …).
fn refuse_if_system(
    devices: &[PlatformDevice],
    object: &str,
    system: &SystemSourcesRead,
) -> Result<(), String> {
    let sources = system();
    let found = SystemDrives::find(devices, sources.as_ref());
    let held = devices
        .iter()
        .find(|device| device.address.object_path == object)
        .map_or(SystemUse::Unknown, |device| {
            found.of_device(device, devices)
        });
    match held.refusal_detail() {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

/// What an `Unlock` that failed means. UDisks2 answers a wrong passphrase
/// with cryptsetup's words (`Failed to activate device: Operation not
/// permitted`, or "incorrect passphrase" in newer releases); a dismissed
/// polkit prompt with its own error name.
pub fn unlock_error(error: &zbus::Error) -> UnlockError {
    let (name, detail) = match error {
        zbus::Error::MethodError(name, detail, _) => (
            name.as_str().to_string(),
            detail.clone().unwrap_or_default(),
        ),
        other => (String::new(), other.to_string()),
    };
    if name.ends_with("NotAuthorizedDismissed") {
        return UnlockError::Dismissed;
    }
    if name.ends_with(".ServiceUnknown") || name.ends_with(".NameHasNoOwner") {
        return UnlockError::Unavailable;
    }
    let lower = detail.to_lowercase();
    if [
        "passphrase",
        "operation not permitted",
        "no key available",
        "bad password",
    ]
    .iter()
    .any(|words| lower.contains(words))
    {
        UnlockError::WrongPassphrase
    } else {
        UnlockError::Failed(detail)
    }
}

/// Unlocks an encrypted drive and mounts the volume inside. No options go
/// with the passphrase: UDisks2 keeps nothing of it.
async fn unlock_through(
    link: &UdisksLink,
    drive: &Drive,
    passphrase: &crate::secret::Secret,
) -> Result<PathBuf, UnlockError> {
    use storage_platform::traits::DeviceControl;
    let connection = link.connection().await.ok_or(UnlockError::Unavailable)?;
    let object = drive
        .object_path
        .clone()
        .ok_or_else(|| UnlockError::Failed("the drive is not a UDisks2 device".to_string()))?;
    let proxy = EncryptedProxy::builder(&connection)
        .path(object)
        .map_err(|error| unlock_error(&error))?
        .build()
        .await
        .map_err(|error| unlock_error(&error))?;
    let cleartext = proxy
        .unlock(passphrase.expose(), std::collections::HashMap::new())
        .await
        .map_err(|error| unlock_error(&error))?;
    let address = storage_platform::model::DeviceAddress {
        object_path: cleartext.as_str().to_string(),
        device_path: String::new(),
    };
    link.client()
        .await
        .map_err(|_| UnlockError::Unavailable)?
        .mount(&address)
        .await
        .map_err(|error| UnlockError::MountFailed(error.to_string()))
}

/// The UDisks2 interfaces whose changes can change the list of drives; job
/// progress and the rest are left out, so a format or an image write does
/// not read the drives several times a second.
const WATCHED_INTERFACES: [&str; 4] = [
    "org.freedesktop.UDisks2.Block",
    "org.freedesktop.UDisks2.Filesystem",
    "org.freedesktop.UDisks2.Drive",
    "org.freedesktop.UDisks2.Encrypted",
];

/// Sends on `notify` whenever UDisks2 says a drive came, went or changed,
/// on the scanner's own connection, until `closed` is set (the window let
/// go), `notify` is closed or the bus goes away.
async fn watch_udisks(link: &UdisksLink, notify: Sender<()>, closed: Arc<AtomicBool>) {
    use smol::stream::StreamExt;
    let Some(connection) = link.connection().await else {
        return;
    };
    let Ok(manager) = zbus::fdo::ObjectManagerProxy::builder(&connection)
        .destination(storage_platform::udisks::UDISKS2_SERVICE)
        .and_then(|builder| builder.path(storage_platform::udisks::UDISKS2_OBJECT_PATH))
    else {
        return;
    };
    let Ok(manager) = manager.build().await else {
        return;
    };
    let (Ok(added), Ok(removed)) = (
        manager.receive_interfaces_added().await,
        manager.receive_interfaces_removed().await,
    ) else {
        return;
    };
    let mut events: std::pin::Pin<Box<dyn smol::stream::Stream<Item = ()> + Send>> =
        Box::pin(added.map(|_| ()).or(removed.map(|_| ())));
    // Only a rescan follows, so a signal from a spoofed sender costs one
    // read and nothing else.
    for interface in WATCHED_INTERFACES {
        let Ok(rule) = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(storage_platform::udisks::UDISKS2_SERVICE)
            .and_then(|rule| rule.interface("org.freedesktop.DBus.Properties"))
            .and_then(|rule| rule.member("PropertiesChanged"))
            .and_then(|rule| rule.arg(0, interface))
            .map(|rule| rule.build())
        else {
            return;
        };
        let Ok(changed) = zbus::MessageStream::for_match_rule(rule, &connection, None).await else {
            return;
        };
        events = Box::pin(events.or(changed.map(|_| ())));
    }
    loop {
        let next = smol::future::or(async { events.next().await.map(|_| true) }, async {
            smol::Timer::after(Duration::from_secs(1)).await;
            Some(false)
        })
        .await;
        if closed.load(Ordering::Acquire) {
            return;
        }
        match next {
            None => return,
            Some(true) if notify.send(()).is_err() => return,
            Some(_) => {}
        }
    }
}

impl DriveSources {
    /// This session's: UDisks2 on the system bus, or a sandbox's fixture.
    pub fn from_env() -> Self {
        let mut sources = Self::with_udisks(None);
        // Debug builds only: a release build never hides the machine's
        // drives behind a fixture.
        if cfg!(debug_assertions)
            && let Some(path) = std::env::var_os(FIXTURE_VARIABLE)
        {
            let path = PathBuf::from(path);
            sources.fixture = Some(Arc::new(move || fixture_drives(&path)));
            sources.changes = None;
        }
        sources
    }

    /// UDisks2 at `address` instead of the system bus (a private bus with a
    /// fake UDisks2 in tests), or the system bus for `None`.
    pub fn with_udisks(address: Option<String>) -> Self {
        use storage_platform::traits::DeviceControl;
        let private = address.clone();
        let link = Arc::new(UdisksLink {
            address,
            connection: Mutex::new(None),
        });
        let at = Arc::clone(&link);
        let devices = move || -> Option<Vec<PlatformDevice>> {
            smol::block_on(async {
                let found = at.client().await.ok()?.enumerate().await;
                if found.is_err() {
                    at.forget();
                }
                found.ok()
            })
        };
        let at = Arc::clone(&link);
        let mount = move |drive: &Drive| {
            smol::block_on(async {
                at.client()
                    .await?
                    .mount(&address_of(drive)?)
                    .await
                    .map_err(|error| error.to_string())
            })
        };
        let at = Arc::clone(&link);
        let unlock = move |drive: &Drive, passphrase: &crate::secret::Secret| {
            smol::block_on(unlock_through(&at, drive, passphrase))
        };
        // The system's own files, and the system bus for the daemon and
        // systemd; a private bus in tests, where the files are empty.
        let testing = private.is_some();
        // The kernel's record of what the running system uses; nothing in
        // tests, where the private bus's drives are all there is.
        let system: SystemSourcesRead = if testing {
            Arc::new(|| Some(SystemSources::every_mount_personal()))
        } else {
            Arc::new(|| SystemSources::read(&storage_platform::Roots::system()))
        };
        let at = Arc::clone(&link);
        let reading = Arc::clone(&system);
        let eject = move |drive: &Drive| smol::block_on(eject_through(&at, drive, &reading));
        let at = Arc::clone(&link);
        let reading = Arc::clone(&system);
        let unmount = move |drive: &Drive| {
            smol::block_on(async {
                let udisks = at.client().await?;
                let address = address_of(drive)?;
                let devices = udisks
                    .enumerate()
                    .await
                    .map_err(|error| error.to_string())?;
                refuse_if_system(&devices, &address.object_path, &reading)?;
                udisks
                    .unmount(&address)
                    .await
                    .map_err(|error| error.to_string())
            })
        };
        let system_bus = move || -> Option<zbus::blocking::Connection> {
            match &private {
                Some(address) => zbus::blocking::connection::Builder::address(address.as_str())
                    .ok()?
                    .build()
                    .ok(),
                None => zbus::blocking::Connection::system().ok(),
            }
        };
        let system_bus = Arc::new(system_bus);
        let system_files = move || {
            if testing {
                drive_mounts::choice::SystemFiles {
                    fstab: Ok(drive_mounts::fstab::Fstab::default()),
                    rules: Ok(drive_mounts::rules::Rules::default()),
                }
            } else {
                drive_mounts::choice::SystemFiles::for_window()
            }
        };
        let bus = Arc::clone(&system_bus);
        let mount_failure =
            move |uuid: &str| bus().and_then(|bus| drive_mounts::client::mount_failure(&bus, uuid));
        let bus = Arc::clone(&system_bus);
        let set_auto_mount = move |uuid: &str, on: bool| match bus() {
            Some(bus) => drive_mounts::client::set_mount_automatically(&bus, uuid, on),
            None => drive_mounts::client::SetOutcome::Failed(
                "the system bus is not reachable".to_string(),
            ),
        };
        let at = link;
        let changes = move |notify: Sender<()>, closed: Arc<AtomicBool>| {
            smol::block_on(watch_udisks(&at, notify, closed))
        };
        Self {
            devices: Arc::new(devices),
            mounts: Arc::new(MountTable::from_env),
            gvfs: Arc::new(gvfs_children),
            capacity: Arc::new(statvfs),
            mount: Arc::new(mount),
            unlock: Arc::new(unlock),
            eject: Arc::new(eject),
            changes: Some(Arc::new(changes)),
            fixture: None,
            unmount: Arc::new(unmount),
            system_files: Arc::new(system_files),
            mount_failure: Arc::new(mount_failure),
            set_auto_mount: Arc::new(set_auto_mount),
            system,
        }
    }

    /// Reads every drive and how full each is. Runs on the scanner's thread.
    pub fn scan(&self) -> Vec<Drive> {
        self.scan_with_system().0
    }

    /// [`Self::scan`], with which UDisks2 drives hold the running system
    /// (ticket 171): what the storage service's device rows are stamped
    /// with, by drive object.
    pub fn scan_with_system(&self) -> (Vec<Drive>, SystemDrives) {
        if let Some(fixture) = &self.fixture {
            let mut drives = fixture();
            let files = (self.system_files)();
            for drive in &mut drives {
                // A fixture's failure stands for systemd's answer.
                let failure = drive.auto_mount.take().and_then(|auto| auto.failure);
                drive.auto_mount = drive_mounts::choice::setting(drive, &files);
                if let (Some(auto), Some(failure)) = (drive.auto_mount.as_mut(), failure) {
                    auto.failure = Some(failure);
                }
            }
            return (drives, SystemDrives::unknown());
        }
        let devices = (self.devices)();
        let mounts = (self.mounts)();
        let root_source = mounts
            .mounts()
            .iter()
            .find(|mount| mount.mount_point == Path::new("/"))
            .map(|mount| mount.source.clone());
        let sources = (self.system)();
        let system = devices
            .as_deref()
            .map_or_else(SystemDrives::unknown, |devices| {
                SystemDrives::find(devices, sources.as_ref())
            });
        let mut drives = devices
            .as_deref()
            .map(|devices| drives_with_system(devices, root_source.as_deref(), &system))
            .unwrap_or_default();
        // No UDisks2 block is `/` (ZFS, an overlay live session, composefs):
        // the system drive comes from the mount table.
        if devices.is_some() && !drives.iter().any(|drive| drive.kind == DriveKind::System) {
            drives.push(system_drive_from_mounts());
        }
        for drive in drives_from_mounts(&mounts, devices.is_some())
            .into_iter()
            .chain(drives_from_gvfs(&(self.gvfs)()))
        {
            if !drives
                .iter()
                .any(|known| known.mount_point.is_some() && known.mount_point == drive.mount_point)
            {
                drives.push(drive);
            }
        }
        let points: Vec<PathBuf> = drives
            .iter()
            .filter_map(|drive| drive.mount_point.clone())
            .collect();
        let mut capacities =
            capacities_within(&points, CAPACITY_TIMEOUT, Arc::clone(&self.capacity)).into_iter();
        for drive in &mut drives {
            if drive.mount_point.is_some() {
                drive.capacity = capacities.next().flatten();
            }
        }
        // Whether each mounts automatically, and why one that should be
        // mounted is not (ticket 127).
        drive_mounts::choice::apply(&mut drives, &(self.system_files)());
        for drive in &mut drives {
            if drive_mounts::choice::failed(drive)
                && drive.kind == DriveKind::Internal
                && let Some(uuid) = drive.uuid.clone()
                && let Some(auto) = drive.auto_mount.as_mut()
            {
                auto.failure = (self.mount_failure)(&uuid);
            }
        }
        sort_drives(&mut drives);
        (drives, system)
    }
}

/// The system drive when only the mount table knows it: `/`, named after the
/// running system like any system drive (`Drive::naming`), never ejectable.
fn system_drive_from_mounts() -> Drive {
    Drive {
        key: "/".to_string(),
        label: None,
        kind: DriveKind::System,
        mount_point: Some(PathBuf::from("/")),
        object_path: None,
        device_path: None,
        drive_object: None,
        backing: None,
        locked: false,
        uuid: None,
        size: 0,
        ejectable: false,
        system_use: SystemUse::RunningSystem,
        capacity: None,
        auto_mount: None,
        form: None,
    }
}

/// Whether a listing of GVfs's folder is still waiting for an answer.
static GVFS_WAITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The folders inside GVfs's FUSE mount, read with the same patience as a
/// drive's capacity: GVfs's FUSE daemon can stall, and a stalled one is not
/// asked again until it answers.
fn gvfs_children() -> Vec<PathBuf> {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return Vec::new();
    };
    if GVFS_WAITING.swap(true, Ordering::AcqRel) {
        return Vec::new();
    }
    let gvfs = PathBuf::from(runtime).join("gvfs");
    let (sender, receiver) = channel();
    let spawned = std::thread::Builder::new()
        .name("files-gvfs-list".to_string())
        .spawn(move || {
            let children: Vec<PathBuf> = std::fs::read_dir(&gvfs)
                .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
                .unwrap_or_default();
            GVFS_WAITING.store(false, Ordering::Release);
            let _ = sender.send(children);
        });
    if spawned.is_err() {
        GVFS_WAITING.store(false, Ordering::Release);
    }
    receiver.recv_timeout(CAPACITY_TIMEOUT).unwrap_or_default()
}

/// One scan: the drives, and which UDisks2 drives hold the running system.
pub type Scanned = (Vec<Drive>, SystemDrives);

/// Reads the drives on a thread of its own, when asked and whenever UDisks2
/// says something changed; the window takes the newest list each frame.
pub struct DriveScanner {
    latest: Arc<Mutex<Option<Scanned>>>,
    generation: Arc<AtomicU64>,
    requests: Sender<()>,
    /// Set when the window lets go of the scanner, so its thread ends even
    /// while the UDisks2 watcher still holds a way to wake it.
    closed: Arc<AtomicBool>,
}

/// How long a burst of changes (a stick's partitions arriving one by one)
/// is let settle before the drives are read.
const SETTLE: Duration = Duration::from_millis(150);

impl DriveScanner {
    pub fn start(sources: DriveSources) -> Self {
        let latest: Arc<Mutex<Option<Scanned>>> = Arc::default();
        let generation = Arc::new(AtomicU64::new(0));
        let closed = Arc::new(AtomicBool::new(false));
        let (requests, inbox): (Sender<()>, Receiver<()>) = channel();
        if let Some(changes) = sources.changes.clone() {
            let notify = requests.clone();
            let closed = Arc::clone(&closed);
            let _ = std::thread::Builder::new()
                .name("files-drives-watch".to_string())
                .spawn(move || changes(notify, closed));
        }
        {
            let latest = Arc::clone(&latest);
            let generation = Arc::clone(&generation);
            let closed = Arc::clone(&closed);
            let _ = std::thread::Builder::new()
                .name("files-drives".to_string())
                .spawn(move || {
                    loop {
                        match inbox.recv_timeout(Duration::from_secs(1)) {
                            Ok(()) => {}
                            Err(RecvTimeoutError::Timeout) if !closed.load(Ordering::Acquire) => {
                                continue;
                            }
                            Err(_) => return,
                        }
                        if closed.load(Ordering::Acquire) {
                            return;
                        }
                        std::thread::sleep(SETTLE);
                        // Requests made meanwhile are this one scan.
                        while inbox.try_recv().is_ok() {}
                        let scanned = sources.scan_with_system();
                        if let Ok(mut latest) = latest.lock() {
                            *latest = Some(scanned);
                        }
                        generation.fetch_add(1, Ordering::Release);
                    }
                });
        }
        let scanner = Self {
            latest,
            generation,
            requests,
            closed,
        };
        scanner.refresh();
        scanner
    }

    /// Asks for the drives to be read again.
    pub fn refresh(&self) {
        let _ = self.requests.send(());
    }

    /// How many scans have finished; changes when there is a new list.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The newest list, once one has been read, with which drives hold the
    /// running system.
    pub fn take(&self) -> Option<Scanned> {
        self.latest.lock().ok().and_then(|mut latest| latest.take())
    }
}

impl Drop for DriveScanner {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{EN_US, ZH_TW};
    use storage_platform::fake::{internal_disk, luks_cleartext, usb_stick};

    fn mount_table(text: &str) -> MountTable {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), text).unwrap();
        files_platform::read_mount_table(file.path())
    }

    fn partition(
        object: &str,
        device: &str,
        kind: &str,
        label: Option<&str>,
        mount: Option<&str>,
    ) -> PlatformDevice {
        let mut device_ = internal_disk(object, device);
        device_.block.id_type = Some(kind.to_string());
        device_.block.id_label = label.map(str::to_string);
        device_.block.hint_system = false;
        device_.block.size = 500 * 1024 * 1024 * 1024;
        device_.mount_point = mount.map(PathBuf::from);
        device_
    }

    #[test]
    fn only_the_drives_a_person_would_name_are_listed() {
        let devices = vec![
            internal_disk("/b/nvme0n1p2", "/dev/nvme0n1p2"),
            partition("/b/data", "/dev/nvme0n1p3", "ntfs", Some("Data"), None),
            partition("/b/home", "/dev/nvme0n1p4", "ext4", None, Some("/home")),
            usb_stick("/b/sdb1", "/dev/sdb1", "aaaa"),
            {
                let mut disc = usb_stick("/b/sr0", "/dev/sr0", "disc");
                disc.block.id_type = Some("iso9660".into());
                disc.block.id_label = Some("Holiday DVD".into());
                if let Some(drive) = disc.drive.as_mut() {
                    drive.optical = true;
                }
                disc
            },
            // Not drives:
            partition("/b/efi", "/dev/nvme0n1p1", "vfat", None, Some("/boot/efi")),
            {
                let mut esp = partition("/b/esp2", "/dev/sda1", "vfat", None, None);
                esp.block.size = 512 * 1024 * 1024;
                esp
            },
            partition("/b/recovery", "/dev/sda2", "ntfs", Some("Recovery"), None),
            partition("/b/swap", "/dev/nvme0n1p5", "swap", None, None),
            partition(
                "/b/loop0",
                "/dev/loop0",
                "squashfs",
                None,
                Some("/snap/core/1"),
            ),
            partition("/b/loop1", "/dev/loop1", "ext4", None, None),
            partition("/b/raw", "/dev/sdc", "", None, None),
        ];
        let drives = drives_from_devices(&devices, None);
        let keys: Vec<&str> = drives.iter().map(|drive| drive.key.as_str()).collect();
        assert_eq!(
            keys,
            ["/b/nvme0n1p2", "/b/data", "/b/home", "/b/sdb1", "/b/sr0"]
        );
        let kind = |key: &str| drives.iter().find(|drive| drive.key == key).unwrap().kind;
        assert_eq!(kind("/b/nvme0n1p2"), DriveKind::System);
        assert_eq!(kind("/b/data"), DriveKind::Internal);
        assert_eq!(kind("/b/sdb1"), DriveKind::Removable);
        assert_eq!(kind("/b/sr0"), DriveKind::Optical);
        assert!(
            drives
                .iter()
                .find(|drive| drive.key == "/b/sdb1")
                .unwrap()
                .ejectable
        );
        assert!(
            !drives
                .iter()
                .find(|drive| drive.key == "/b/data")
                .unwrap()
                .ejectable
        );
        // An unlocked encrypted volume with a filesystem is a drive too.
        let unlocked = drives_from_devices(
            &[luks_cleartext(
                "/b/dm0",
                "/dev/dm-0",
                "/b/sda3",
                "/media/a/Vault",
            )],
            None,
        );
        assert_eq!(unlocked.len(), 1);
        assert_eq!(unlocked[0].kind, DriveKind::Internal);
    }

    #[test]
    fn network_shares_and_gvfs_servers_and_phones_are_drives() {
        let mounts = mount_table(concat!(
            "28 1 8:2 / / rw - ext4 /dev/sda2 rw\n",
            "30 28 0:40 / /mnt/nas rw - cifs //nas/share rw\n",
            "31 28 0:41 / /mnt/backup rw - nfs4 server:/backup rw\n",
            "32 28 0:42 / /run/user/1000/doc rw - fuse.portal portal rw\n",
            "33 28 0:5 / /tmp rw - tmpfs tmpfs rw\n",
            "34 28 7:1 / /snap/core/1 ro - squashfs /dev/loop1 ro\n",
            "35 28 8:17 / /media/a/STICK rw - vfat /dev/sdb1 rw\n",
        ));
        // With UDisks2, the mount table adds only the network shares.
        let with = drives_from_mounts(&mounts, true);
        let mut points: Vec<String> = with
            .iter()
            .map(|drive| drive.mount_point.as_ref().unwrap().display().to_string())
            .collect();
        points.sort();
        assert_eq!(points, ["/mnt/backup", "/mnt/nas"]);
        assert!(with.iter().all(|drive| drive.kind == DriveKind::Network));
        // Without it, the system drive and removable media too.
        let without = drives_from_mounts(&mounts, false);
        assert!(without.iter().any(|drive| drive.kind == DriveKind::System));
        assert!(
            without
                .iter()
                .any(|drive| drive.kind == DriveKind::Removable
                    && drive.label.as_deref() == Some("STICK"))
        );
        assert!(
            !without
                .iter()
                .any(|drive| drive.mount_point.as_deref() == Some(Path::new("/tmp")))
        );

        let gvfs = drives_from_gvfs(&[
            PathBuf::from("/run/user/1000/gvfs/mtp:host=Pixel_8"),
            PathBuf::from("/run/user/1000/gvfs/smb-share:server=nas,share=photos"),
            PathBuf::from("/run/user/1000/gvfs/sftp:host=build.local"),
            PathBuf::from("/run/user/1000/gvfs/trash:"),
        ]);
        assert_eq!(gvfs.len(), 3);
        assert_eq!(gvfs[0].kind, DriveKind::Phone);
        assert_eq!(gvfs[0].label.as_deref(), Some("Pixel_8"));
        assert_eq!(gvfs[1].kind, DriveKind::Network);
        assert_eq!(gvfs[1].label.as_deref(), Some("photos"));
        assert_eq!(gvfs[2].label.as_deref(), Some("build.local"));
    }

    #[test]
    fn the_running_systems_drive_is_named_after_the_system_and_no_other_is() {
        // The root (unlabelled, as Ubuntu-based installers leave it), another
        // Linux install on a second partition with a label, and one without.
        let mut root = partition("/b/root", "/dev/nvme0n1p2", "ext4", None, Some("/"));
        root.block.size = 263 * 1024 * 1024 * 1024;
        let devices = vec![
            root.clone(),
            partition("/b/fedora", "/dev/nvme0n1p3", "ext4", Some("fedora"), None),
            partition("/b/other", "/dev/nvme0n1p4", "ext4", None, None),
        ];
        let drives = drives_from_devices(&devices, None);
        let find = |key: &str| drives.iter().find(|drive| drive.key == key).unwrap();
        let named =
            |drive: &Drive, system: Option<&str>, c| name_words(drive.naming_with(system), c);
        assert_eq!(find("/b/root").kind, DriveKind::System);
        assert_eq!(named(find("/b/root"), Some("Zorin OS"), &EN_US), "Zorin OS");
        assert_eq!(named(find("/b/root"), Some("Zorin OS"), &ZH_TW), "Zorin OS");
        assert_eq!(named(find("/b/fedora"), Some("Zorin OS"), &EN_US), "fedora");
        assert_eq!(
            named(find("/b/other"), Some("Zorin OS"), &EN_US),
            "500 GB drive"
        );
        // A labelled root takes the system's name too.
        let mut labelled = root;
        labelled.block.id_label = Some("rootfs".into());
        let labelled = drives_from_devices(&[labelled], None).remove(0);
        assert_eq!(named(&labelled, Some("Zorin OS"), &EN_US), "Zorin OS");
        // Nothing in os-release: the fixed wording, never a guess.
        assert_eq!(named(&labelled, None, &EN_US), "System drive");
        assert_eq!(named(&labelled, None, &ZH_TW), "系統磁碟");
        assert_eq!(system_drive_name(None, &ZH_TW), "系統磁碟");
        assert_eq!(system_drive_name(Some("Ubuntu"), &EN_US), "Ubuntu");
        // Named by the system, the line under it gives its size.
        let mut unmounted = labelled.clone();
        unmounted.mount_point = None;
        unmounted.label = None;
        assert!(
            unmounted
                .caption(&EN_US)
                .is_some_and(|line| line.starts_with("263 GB · ")),
            "{:?}",
            unmounted.caption(&EN_US)
        );
    }

    #[test]
    fn a_drive_is_named_by_its_label_or_its_size_and_says_how_full_it_is() {
        let mut drive =
            drives_from_devices(&[usb_stick("/b/sdb1", "/dev/sdb1", "aaaa")], None).remove(0);
        assert_eq!(drive.name(&EN_US), "FIELD DATA");
        drive.label = None;
        assert_eq!(drive.name(&EN_US), "32.0 GB drive");
        assert_eq!(drive.name(&ZH_TW), "32.0 GB 磁碟");
        // Named by its size: the line does not say it again.
        assert_eq!(drive.caption(&EN_US).as_deref(), Some("Not mounted"));
        drive.label = Some("FIELD DATA".into());
        assert_eq!(
            drive.caption(&EN_US).as_deref(),
            Some("32.0 GB · Not mounted")
        );
        assert_eq!(drive.caption(&ZH_TW).as_deref(), Some("32.0 GB · 未掛載"));
        drive.label = None;
        drive.mount_point = Some(PathBuf::from("/media/a/STICK"));
        drive.capacity = Some(Capacity {
            total: 32 * 1024 * 1024 * 1024,
            free: 12 * 1024 * 1024 * 1024,
        });
        assert_eq!(
            drive.caption(&EN_US).as_deref(),
            Some("12.0 GB free of 32.0 GB")
        );
        assert_eq!(
            drive.caption(&ZH_TW).as_deref(),
            Some("可用 12.0 GB，共 32.0 GB")
        );
        // Mounted but did not answer: no line, no bar.
        drive.capacity = None;
        assert_eq!(drive.caption(&EN_US), None);
    }

    #[test]
    fn the_bar_turns_to_caution_at_ninety_percent() {
        let at = |used: u64| Capacity {
            total: 100,
            free: 100 - used,
        };
        assert!(!at(0).is_nearly_full());
        assert!(!at(89).is_nearly_full());
        assert!(at(90).is_nearly_full());
        assert!(at(100).is_nearly_full());
        assert_eq!(at(25).used_fraction(), 0.25);
        assert!(!Capacity { total: 0, free: 0 }.is_nearly_full());
    }

    #[test]
    fn a_drive_that_does_not_answer_is_listed_without_a_bar_and_without_waiting() {
        let stalled: CapacityRead = Arc::new(|_| {
            std::thread::sleep(Duration::from_secs(30));
            Some(Capacity { total: 1, free: 1 })
        });
        let started = std::time::Instant::now();
        assert_eq!(
            capacity_within(Path::new("/mnt/nas"), Duration::from_millis(100), stalled),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        // A real one answers.
        let root = tempfile::tempdir().unwrap();
        let read: CapacityRead = Arc::new(statvfs);
        let capacity = capacity_within(root.path(), CAPACITY_TIMEOUT, read).unwrap();
        assert!(capacity.total >= capacity.free && capacity.total > 0);
    }

    #[test]
    fn a_locked_drive_asks_for_its_passphrase_and_an_unlocked_one_is_the_volume_inside() {
        let mut container = usb_stick("/b/sdc1", "/dev/sdc1", "cccc");
        container.block.id_type = Some("crypto_LUKS".into());
        container.block.id_label = None;
        let drives = drives_from_devices(std::slice::from_ref(&container), None);
        assert_eq!(drives.len(), 1);
        assert!(drives[0].locked);
        assert_eq!(drives[0].kind, DriveKind::Removable);
        assert_eq!(drives[0].name(&EN_US), "32.0 GB drive");
        assert_eq!(drives[0].caption(&EN_US).as_deref(), Some("Locked"));
        assert_eq!(drives[0].caption(&ZH_TW).as_deref(), Some("已鎖定"));

        // Unlocked: the container is not listed again, and the volume in it
        // is on the container's USB drive, so it ejects and locks again.
        let cleartext = luks_cleartext("/b/dm1", "/dev/dm-1", "/b/sdc1", "/media/a/Secret");
        let drives = drives_from_devices(&[container, cleartext], None);
        assert_eq!(drives.len(), 1);
        let volume = &drives[0];
        assert_eq!(volume.key, "/b/dm1");
        assert!(!volume.locked);
        assert_eq!(volume.kind, DriveKind::Removable);
        assert!(volume.ejectable);
        assert_eq!(volume.backing.as_deref(), Some("/b/sdc1"));
        assert_eq!(
            volume.drive_object.as_deref(),
            Some("/org/freedesktop/UDisks2/drives/SN_cccc")
        );
    }

    fn sources(devices: Vec<PlatformDevice>, mounts: &str) -> DriveSources {
        let mounts = mount_table(mounts);
        let mut sources = DriveSources::with_udisks(Some("unix:path=/nonexistent".into()));
        sources.devices = Arc::new(move || Some(devices.clone()));
        sources.mounts = Arc::new(move || mounts.clone());
        sources.gvfs = Arc::new(Vec::new);
        sources.capacity = Arc::new(|_| {
            Some(Capacity {
                total: 100,
                free: 40,
            })
        });
        sources.changes = None;
        sources
    }

    #[test]
    fn the_system_drive_is_found_by_the_mount_table_when_udisks_names_another_mount_point() {
        // btrfs: one volume at `/home` and `/`; UDisks2 named `/home` first.
        let mut root = internal_disk("/b/nvme0n1p2", "/dev/nvme0n1p2");
        root.mount_point = Some(PathBuf::from("/home"));
        let drives = sources(
            vec![root],
            concat!(
                "28 1 0:30 /@ / rw - btrfs /dev/nvme0n1p2 rw\n",
                "29 28 0:30 /@home /home rw - btrfs /dev/nvme0n1p2 rw\n",
            ),
        )
        .scan();
        assert_eq!(drives.len(), 1);
        assert_eq!(drives[0].kind, DriveKind::System);
        assert_eq!(drives[0].mount_point.as_deref(), Some(Path::new("/")));
        assert_eq!(
            drives[0].capacity,
            Some(Capacity {
                total: 100,
                free: 40
            })
        );
    }

    #[test]
    fn two_windows_reading_one_drive_at_once_both_get_its_bar() {
        let calls = Arc::new(AtomicU64::new(0));
        let read: CapacityRead = {
            let calls = Arc::clone(&calls);
            Arc::new(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(200));
                Some(Capacity { total: 8, free: 2 })
            })
        };
        let drive = PathBuf::from("/mnt/shared-read-test");
        let both: Vec<Option<Capacity>> = std::thread::scope(|scope| {
            let first =
                scope.spawn(|| capacity_within(&drive, Duration::from_secs(5), Arc::clone(&read)));
            std::thread::sleep(Duration::from_millis(50));
            let second =
                scope.spawn(|| capacity_within(&drive, Duration::from_secs(5), Arc::clone(&read)));
            vec![first.join().unwrap(), second.join().unwrap()]
        });
        assert_eq!(both, [Some(Capacity { total: 8, free: 2 }); 2]);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "one read for both");
    }

    #[test]
    fn a_stalled_share_is_not_asked_again_until_it_answers() {
        let calls = Arc::new(AtomicU64::new(0));
        let (release, gate) = channel::<()>();
        let gate = Arc::new(Mutex::new(gate));
        let read: CapacityRead = {
            let calls = Arc::clone(&calls);
            Arc::new(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                let _ = gate.lock().unwrap().recv();
                Some(Capacity { total: 10, free: 5 })
            })
        };
        let share = PathBuf::from("/mnt/stalled-share-test");
        for _ in 0..3 {
            assert_eq!(
                capacity_within(&share, Duration::from_millis(50), Arc::clone(&read)),
                None
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "one read waits, not three");
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while is_waiting(&share) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        release.send(()).unwrap();
        assert_eq!(
            capacity_within(&share, Duration::from_secs(2), read),
            Some(Capacity { total: 10, free: 5 })
        );
    }

    #[test]
    fn at_the_narrowest_sidebar_the_line_is_dropped_and_the_bar_kept() {
        let line = Capacity {
            total: 256 * 1024 * 1024 * 1024,
            free: 12 * 1024 * 1024 * 1024,
        }
        .line(&EN_US);
        let edge = crate::layout::SIDEBAR_EDGE;
        assert!(crate::layout::drive_caption_fits(&line, edge.default, true));
        assert!(!crate::layout::drive_caption_fits(&line, edge.min, true));
        let zh = Capacity {
            total: 256 * 1024 * 1024 * 1024,
            free: 12 * 1024 * 1024 * 1024,
        }
        .line(&ZH_TW);
        assert!(crate::layout::drive_caption_fits(&zh, edge.default, true));
    }

    const WINRE: &str = "de94bba4-06d1-4d40-a16a-bfd50179d6ac";

    #[test]
    fn recovery_and_firmware_partitions_and_what_udisks_hides_are_not_drives() {
        // Windows' recovery partition: NTFS, no label, only its type says
        // what it is.
        let mut winre = partition("/b/winre", "/dev/nvme0n1p4", "ntfs", None, None);
        winre.block.partition_type = Some(WINRE.into());
        // The same on MBR (type 0x27), and an OEM tool partition UDisks2
        // marked to hide.
        let mut mbr_recovery = partition("/b/mbr", "/dev/sda3", "ntfs", None, None);
        mbr_recovery.block.partition_type = Some("0x27".into());
        let mut oem = partition("/b/oem", "/dev/sda4", "vfat", Some("TOOLS"), None);
        oem.block.size = 8 << 30;
        oem.block.hint_ignore = true;
        let mut data = partition("/b/data", "/dev/sda5", "ntfs", Some("Data"), None);
        data.block.partition_type = Some("ebd0a0a2-b9e5-4433-87c0-68b6b72699c7".into());
        let keys: Vec<String> = drives_from_devices(&[winre, mbr_recovery, oem, data], None)
            .into_iter()
            .map(|drive| drive.key)
            .collect();
        assert_eq!(keys, ["/b/data"]);
    }

    #[test]
    fn nothing_on_the_drive_the_system_runs_from_can_be_ejected() {
        // Linux on an external SSD: `/` and a data partition on one USB
        // drive.
        let mut root = usb_stick("/b/sdb2", "/dev/sdb2", "root");
        root.block.id_type = Some("ext4".into());
        root.mount_point = Some(PathBuf::from("/"));
        let mut data = usb_stick("/b/sdb3", "/dev/sdb3", "root");
        data.block.id_label = Some("Data".into());
        data.mount_point = Some(PathBuf::from("/media/a/Data"));
        let other = usb_stick("/b/sdc1", "/dev/sdc1", "other");
        let drives = drives_from_devices(&[root, data, other], None);
        let find = |key: &str| drives.iter().find(|drive| drive.key == key).unwrap();
        assert_eq!(find("/b/sdb2").kind, DriveKind::System);
        assert!(!find("/b/sdb2").ejectable);
        assert!(!find("/b/sdb3").ejectable, "on the system's drive");
        assert!(find("/b/sdc1").ejectable);
    }

    /// Ticket 171: `/home` and swap on USB disks, the running system on an
    /// internal one, and a stick: only the stick may be ejected, and a scan
    /// whose mount table could not be read knows nothing.
    #[test]
    fn a_drive_holding_a_system_folder_or_swap_is_not_ejectable() {
        let mut root = partition("/b/nvme0n1p2", "/dev/nvme0n1p2", "ext4", None, Some("/"));
        root.drive.as_mut().unwrap().object_path = "/d/nvme".into();
        let mut home = usb_stick("/b/sdb1", "/dev/sdb1", "home");
        home.block.id_type = Some("ext4".into());
        home.mount_point = Some(PathBuf::from("/home"));
        let mut data = usb_stick("/b/sdc1", "/dev/sdc1", "swap");
        data.mount_point = Some(PathBuf::from("/media/a/Data"));
        let mut swap = data.clone();
        swap.address.object_path = "/b/sdc2".into();
        swap.address.device_path = "/dev/sdc2".into();
        swap.block.device_path = "/dev/sdc2".into();
        swap.block.id_type = Some("swap".into());
        swap.mount_point = None;
        let mut stick = usb_stick("/b/sdd1", "/dev/sdd1", "stick");
        stick.mount_point = Some(PathBuf::from("/media/a/STICK"));
        let devices = vec![root, home, data, swap, stick];
        let mut scan = sources(devices.clone(), "");
        scan.system = Arc::new(|| {
            Some(SystemSources::parse(
                "/dev/nvme0n1p2 / ext4 rw 0 0\n/dev/sdb1 /home ext4 rw 0 0\n",
                "Filename Type Size Used Priority\n/dev/sdc2 partition 1 0 -2\n",
            ))
        });
        let drives = scan.scan();
        let find = |key: &str| drives.iter().find(|drive| drive.key == key).unwrap();
        assert_eq!(
            find("/b/sdb1").system_use,
            SystemUse::SystemFolder {
                path: PathBuf::from("/home")
            }
        );
        assert_eq!(find("/b/sdc1").system_use, SystemUse::Swap);
        assert_eq!(find("/b/sdd1").system_use, SystemUse::Free);
        assert!(!find("/b/sdb1").ejectable);
        assert!(!find("/b/sdc1").ejectable, "shares a drive with swap");
        assert!(find("/b/sdd1").ejectable);

        let mut blind = sources(devices, "");
        blind.system = Arc::new(|| None);
        let drives = blind.scan();
        let stick = drives.iter().find(|drive| drive.key == "/b/sdd1").unwrap();
        assert_eq!(stick.system_use, SystemUse::Unknown);
        assert!(!stick.ejectable);
    }

    #[test]
    fn every_mount_point_counts_and_the_root_is_found_by_its_mapper_name() {
        // btrfs at `/home` first and `/` second.
        let mut btrfs = internal_disk("/b/nvme0n1p2", "/dev/nvme0n1p2");
        btrfs.block.id_type = Some("btrfs".into());
        btrfs.mount_point = Some(PathBuf::from("/home"));
        btrfs.block.mount_points = vec![PathBuf::from("/home"), PathBuf::from("/")];
        let drives = drives_from_devices(&[btrfs], None);
        assert_eq!(drives[0].kind, DriveKind::System);
        assert_eq!(drives[0].mount_point.as_deref(), Some(Path::new("/")));

        // btrfs on LUKS: the mount table says `/dev/mapper/luks-…`,
        // UDisks2 `/dev/dm-0` with the mapper name among its symlinks.
        let mut cleartext = luks_cleartext("/b/dm0", "/dev/dm-0", "/b/nvme0n1p3", "/home");
        cleartext.block.symlinks = vec!["/dev/mapper/luks-1234".into()];
        let drives = drives_from_devices(&[cleartext], Some("/dev/mapper/luks-1234"));
        assert_eq!(drives[0].kind, DriveKind::System);
        assert!(!drives[0].ejectable);
    }

    #[test]
    fn a_system_on_zfs_or_an_overlay_comes_from_the_mount_table() {
        let drives = sources(
            vec![usb_stick("/b/sdb1", "/dev/sdb1", "aaaa")],
            concat!(
                "28 1 0:30 / / rw - zfs rpool/ROOT/ubuntu rw\n",
                "35 28 8:17 / /media/a/STICK rw - exfat /dev/sdb1 rw\n",
            ),
        )
        .scan();
        let system = drives
            .iter()
            .find(|drive| drive.kind == DriveKind::System)
            .expect("a system drive");
        assert_eq!(system.mount_point.as_deref(), Some(Path::new("/")));
        assert!(!system.ejectable);
    }

    #[test]
    fn an_image_written_to_a_stick_is_one_removable_drive_not_a_disc() {
        // An ISO on a stick: the whole disk and partition 1 both read as
        // iso9660; the drive is not optical.
        let mut whole = usb_stick("/b/sdb", "/dev/sdb", "iso");
        whole.block.id_type = Some("iso9660".into());
        whole.block.partition_number = None;
        whole.block.partition_table = true;
        let mut first = usb_stick("/b/sdb1", "/dev/sdb1", "iso");
        first.block.id_type = Some("iso9660".into());
        let drives = drives_from_devices(&[whole, first], None);
        assert_eq!(drives.len(), 1);
        assert_eq!(drives[0].key, "/b/sdb1");
        assert_eq!(drives[0].kind, DriveKind::Removable);
    }

    #[test]
    fn a_btrfs_filesystem_on_two_devices_is_one_drive() {
        let mut first = partition(
            "/b/sda1",
            "/dev/sda1",
            "btrfs",
            Some("Pool"),
            Some("/mnt/pool"),
        );
        first.block.id_uuid = Some("pool-uuid".into());
        let mut second = partition("/b/sdb1", "/dev/sdb1", "btrfs", Some("Pool"), None);
        second.block.id_uuid = Some("pool-uuid".into());
        let drives = drives_from_devices(&[second, first], None);
        assert_eq!(drives.len(), 1);
        assert_eq!(
            drives[0].mount_point.as_deref(),
            Some(Path::new("/mnt/pool"))
        );
    }

    #[test]
    fn a_removable_drive_keeps_a_label_an_internal_one_would_be_hidden_for() {
        // A Raspberry Pi card's `boot` partition and a stick labelled EFI.
        let mut card = usb_stick("/b/mmc1", "/dev/mmcblk0p1", "card");
        card.block.id_type = Some("vfat".into());
        card.block.id_label = Some("boot".into());
        let mut stick = usb_stick("/b/sdb1", "/dev/sdb1", "efi");
        stick.block.id_label = Some("EFI".into());
        let internal = partition("/b/sda1", "/dev/sda1", "ntfs", Some("Recovery"), None);
        let keys: Vec<String> = drives_from_devices(&[card, stick, internal], None)
            .into_iter()
            .map(|drive| drive.key)
            .collect();
        assert_eq!(keys, ["/b/mmc1", "/b/sdb1"]);
    }

    #[test]
    fn only_a_wrong_passphrase_asks_again_and_a_dismissed_prompt_says_nothing() {
        let method = |name: &str, detail: &str| {
            zbus::Error::MethodError(
                zbus::names::OwnedErrorName::try_from(name).unwrap(),
                Some(detail.to_string()),
                zbus::message::Message::method_call("/", "Ping")
                    .unwrap()
                    .build(&())
                    .unwrap(),
            )
        };
        assert_eq!(
            unlock_error(&method(
                "org.freedesktop.UDisks2.Error.Failed",
                "Error unlocking /dev/sdc1: Failed to activate device: Operation not permitted"
            )),
            UnlockError::WrongPassphrase
        );
        assert_eq!(
            unlock_error(&method(
                "org.freedesktop.UDisks2.Error.Failed",
                "Error unlocking /dev/sdc1: Failed to activate device: Incorrect passphrase."
            )),
            UnlockError::WrongPassphrase
        );
        assert_eq!(
            unlock_error(&method(
                "org.freedesktop.UDisks2.Error.NotAuthorizedDismissed",
                "Not authorized to perform operation"
            )),
            UnlockError::Dismissed
        );
        assert_eq!(
            unlock_error(&method(
                "org.freedesktop.DBus.Error.ServiceUnknown",
                "The name is not activatable"
            )),
            UnlockError::Unavailable
        );
        assert_eq!(
            unlock_error(&method(
                "org.freedesktop.UDisks2.Error.Failed",
                "Device busy"
            )),
            UnlockError::Failed("Device busy".into())
        );
    }

    #[test]
    fn a_gvfs_name_is_decoded_whole() {
        let gvfs = drives_from_gvfs(&[PathBuf::from(
            "/run/user/1000/gvfs/smb-share:server=nas,share=%E7%85%A7%E7%89%87%20%282024%29",
        )]);
        assert_eq!(gvfs[0].label.as_deref(), Some("照片 (2024)"));
        assert_eq!(percent_decode("50%"), "50%");
    }

    #[test]
    fn drives_are_ordered_system_first_then_by_kind_and_name() {
        let mut drives = drives_from_mounts(
            &mount_table(concat!(
                "30 28 0:40 / /mnt/zeta rw - cifs //nas/z rw\n",
                "28 1 8:2 / / rw - ext4 /dev/sda2 rw\n",
                "31 28 0:41 / /mnt/alpha rw - cifs //nas/a rw\n",
            )),
            false,
        );
        sort_drives(&mut drives);
        let names: Vec<Option<String>> = drives.iter().map(|drive| drive.label.clone()).collect();
        assert_eq!(names, [None, Some("alpha".into()), Some("zeta".into())]);
    }
}
