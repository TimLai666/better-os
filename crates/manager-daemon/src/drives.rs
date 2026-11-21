//! `org.betteros.Drives1`: whether a drive mounts automatically (ticket 127,
//! ADR 0032).
//!
//! One method, one polkit action. The only argument besides on/off is a
//! filesystem UUID, checked against `drive_mounts::ids::valid_uuid` before
//! polkit is asked. The daemon then asks UDisks2 for the device itself:
//! whether it is internal or removable, its filesystem, and where it is
//! mounted are UDisks2's answer, never the caller's.
//!
//! An internal drive is a line in Better OS's block in `/etc/fstab`, started
//! and stopped as the mount unit systemd's generator makes of it. A removable
//! drive turned off is a line in Better OS's udev rules file. Nothing outside
//! the block, and no file but those two, is ever written.

use std::collections::HashMap;
use std::io::Write as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use drive_mounts::fstab::{DriveNames, ENTRY_FILESYSTEMS, Entry, Fstab, OWNERLESS};
use drive_mounts::ids::{mount_point, mount_unit, valid_uuid};
use drive_mounts::rules::Rules;
use zbus::message::Header;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{fdo, interface};

use crate::DaemonError;
use crate::authorize::{Authorization, Authorizer, DRIVE_MOUNTING_ACTION};
use crate::service::{CallGate, refuse};

pub const INTERFACE: &str = "org.betteros.Drives1";

/// What UDisks2 says about the device with a UUID.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedDrive {
    /// `IdType`: `ext4`, `ntfs`, `crypto_LUKS`, …
    pub filesystem: String,
    /// `IdUsage`: `filesystem`, `crypto`, `other`, …
    pub usage: String,
    /// The drive it is on is removable, or has removable media.
    pub removable: bool,
    /// Where it is mounted now.
    pub mount_points: Vec<PathBuf>,
    pub names: DriveNames,
    /// What of the running system its drive holds (ticket 171): a drive
    /// that holds `/`, a system folder or swap is the system's, and its
    /// choice is refused like the system drive's.
    pub system_use: drive_mounts::listing::SystemUse,
}

/// Finds the device with a UUID.
pub trait DriveResolver: Send + Sync {
    fn resolve(&self, uuid: &str) -> Result<Option<ResolvedDrive>, DaemonError>;
}

/// systemd and udev, as the daemon uses them.
pub trait MountUnits: Send + Sync {
    /// `systemctl daemon-reload`, so the generator reads the file again.
    fn reload(&self) -> Result<(), String>;
    /// Mounts the unit and waits; the error is why it failed.
    fn start(&self, unit: &str) -> Result<(), String>;
    /// Unmounts the unit and waits; the error is why it could not.
    fn stop(&self, unit: &str) -> Result<(), String>;
    /// Tells udev the rules changed. Best effort: udev also notices itself.
    fn reload_udev(&self);
}

/// The files the daemon writes. The system's in the shipped binary;
/// fixtures in tests.
#[derive(Clone, Debug)]
pub struct DrivePaths {
    pub fstab: PathBuf,
    pub rules: PathBuf,
    /// `/mnt`'s stand-in for removing empty mount points; the entry itself
    /// always names `/mnt/better-os/<uuid>`.
    pub mount_root: PathBuf,
    /// The lock every change holds from reading a file to renaming the new
    /// one over it (ADR 0032).
    pub lock: PathBuf,
    /// Who must own the lock file: root on a machine; the test's own user
    /// in tests (the seam that lets a test make one that is not root's).
    pub lock_owner: u32,
    /// How long a change waits for the lock before it says the mount table
    /// is busy.
    pub lock_wait: std::time::Duration,
}

impl DrivePaths {
    pub fn system() -> Self {
        Self {
            fstab: PathBuf::from("/etc/fstab"),
            rules: PathBuf::from(drive_mounts::rules::RULES_PATH),
            mount_root: PathBuf::from("/"),
            lock: PathBuf::from("/run/better-os/fstab.lock"),
            lock_owner: 0,
            lock_wait: LOCK_WAIT,
        }
    }

    fn local(&self, path: &Path) -> PathBuf {
        self.mount_root.join(path.strip_prefix("/").unwrap_or(path))
    }
}

/// UDisks2 over the system bus.
pub struct UdisksResolver {
    pub connection: zbus::blocking::Connection,
}

const UDISKS: &str = "org.freedesktop.UDisks2";

fn bytes_path(value: &OwnedValue) -> Option<String> {
    let bytes: Vec<u8> = value.try_clone().ok()?.try_into().ok()?;
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).ok()
}

fn bytes_paths(value: &OwnedValue) -> Vec<String> {
    let Ok(list) = <Vec<Vec<u8>>>::try_from(value.try_clone().unwrap_or_else(|_| 0u8.into()))
    else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(|bytes| {
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            String::from_utf8(bytes[..end].to_vec()).ok()
        })
        .collect()
}

impl UdisksResolver {
    fn all(&self, path: &str, interface: &str) -> Option<HashMap<String, OwnedValue>> {
        self.connection
            .call_method(
                Some(UDISKS),
                path,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(interface,),
            )
            .ok()?
            .body()
            .deserialize()
            .ok()
    }
}

fn string(map: &HashMap<String, OwnedValue>, key: &str) -> String {
    map.get(key)
        .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
        .unwrap_or_default()
}

fn flag(map: &HashMap<String, OwnedValue>, key: &str) -> bool {
    map.get(key)
        .and_then(|v| bool::try_from(v.try_clone().ok()?).ok())
        .unwrap_or(false)
}

impl DriveResolver for UdisksResolver {
    fn resolve(&self, uuid: &str) -> Result<Option<ResolvedDrive>, DaemonError> {
        let spec: HashMap<&str, zbus::zvariant::Value> =
            HashMap::from([("uuid", zbus::zvariant::Value::from(uuid))]);
        let options: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
        let found: Vec<OwnedObjectPath> = self
            .connection
            .call_method(
                Some(UDISKS),
                "/org/freedesktop/UDisks2/Manager",
                Some("org.freedesktop.UDisks2.Manager"),
                "ResolveDevice",
                &(spec, options),
            )
            .map_err(|e| DaemonError::WriteFailed(format!("UDisks2: {e}")))?
            .body()
            .deserialize()
            .map_err(|e| DaemonError::WriteFailed(format!("UDisks2: {e}")))?;
        // The device whose own filesystem has the UUID, never a whole disk
        // a partition of it happens to match.
        for path in found {
            let Some(block) = self.all(path.as_str(), "org.freedesktop.UDisks2.Block") else {
                continue;
            };
            if !string(&block, "IdUUID").eq_ignore_ascii_case(uuid) {
                continue;
            }
            let drive = block
                .get("Drive")
                .and_then(|v| OwnedObjectPath::try_from(v.try_clone().ok()?).ok())
                .filter(|p| p.as_str() != "/")
                .and_then(|p| self.all(p.as_str(), "org.freedesktop.UDisks2.Drive"));
            // A volume with no drive is doubtful, and doubtful is removable:
            // a udev rule, never an fstab line.
            let removable = drive.as_ref().is_none_or(|d| {
                drive_mounts::listing::hot_pluggable(
                    flag(d, "Removable"),
                    flag(d, "MediaRemovable"),
                    &string(d, "ConnectionBus"),
                )
            });
            let mount_points = self
                .all(path.as_str(), "org.freedesktop.UDisks2.Filesystem")
                .and_then(|fs| fs.get("MountPoints").map(bytes_paths))
                .unwrap_or_default()
                .into_iter()
                .map(PathBuf::from)
                .collect();
            let partuuid = self
                .all(path.as_str(), "org.freedesktop.UDisks2.Partition")
                .map(|p| string(&p, "UUID"))
                .filter(|p| !p.is_empty());
            let mut device_paths: Vec<String> = block
                .get("Device")
                .and_then(bytes_path)
                .into_iter()
                .collect();
            device_paths.extend(block.get("Symlinks").map(bytes_paths).unwrap_or_default());
            let label = Some(string(&block, "IdLabel")).filter(|l| !l.is_empty());
            let system_use = drive_mounts::listing::system_use_now(&self.connection, path.as_str());
            return Ok(Some(ResolvedDrive {
                system_use,
                filesystem: string(&block, "IdType"),
                usage: string(&block, "IdUsage"),
                removable,
                mount_points,
                names: DriveNames {
                    // UDisks2's own spelling: `/dev/disk/by-uuid` is named
                    // by it, and fstab's `UUID=` is looked up there.
                    uuid: string(&block, "IdUUID"),
                    label,
                    partuuid,
                    device_paths,
                },
            }));
        }
        Ok(None)
    }
}

/// `systemctl` and `udevadm`.
pub struct SystemctlUnits;

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Whether systemd is the init. It always is on a desktop; a container (the
/// end-to-end check's) has none, and there nothing was mounted by a unit, so
/// there is nothing to reload or stop, and a start says why it cannot.
fn systemd_runs() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

impl MountUnits for SystemctlUnits {
    fn reload(&self) -> Result<(), String> {
        if !systemd_runs() {
            return Ok(());
        }
        run("systemctl", &["daemon-reload"])
    }

    fn start(&self, unit: &str) -> Result<(), String> {
        if !systemd_runs() {
            return Err("systemd is not running".to_string());
        }
        run("systemctl", &["start", unit]).map_err(|message| {
            // "Job for … failed" says nothing; the unit's result does.
            let result = std::process::Command::new("systemctl")
                .args(["show", "--property=Result", "--value", unit])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|r| !r.is_empty());
            match result {
                Some(result) => format!("{result}: {message}"),
                None => message,
            }
        })
    }

    fn stop(&self, unit: &str) -> Result<(), String> {
        if !systemd_runs() {
            return Ok(());
        }
        run("systemctl", &["stop", unit])
    }

    fn reload_udev(&self) {
        let _ = run("udevadm", &["control", "--reload"]);
    }
}

/// Replaces `path` with `text` atomically, only while it still holds
/// `original` (`None`: it did not exist). The new file gets the old one's
/// mode and owner (0644 and its directory's owner for a new one) and is synced before the rename;
/// the directory is synced after. `text` `None` removes the file.
pub fn replace_file(
    path: &Path,
    original: Option<&[u8]>,
    text: Option<&str>,
) -> Result<(), DaemonError> {
    let failed = |e: std::io::Error| DaemonError::WriteFailed(format!("{}: {e}", path.display()));
    // A link is not written through: the rename would replace the link with
    // a file, and the file it pointed to may be anyone's.
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(DaemonError::WriteFailed(format!(
            "{} is a symbolic link; Better OS does not write through it",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| DaemonError::WriteFailed(path.display().to_string()))?;
    let still = || -> Result<(), DaemonError> {
        let now = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(failed(e)),
        };
        if now.as_deref() != original {
            return Err(DaemonError::WriteFailed(format!(
                "{} changed while it was being written; nothing was changed",
                path.display()
            )));
        }
        Ok(())
    };
    match text {
        None => {
            still()?;
            if original.is_some() {
                std::fs::remove_file(path).map_err(failed)?;
            }
        }
        Some(text) => {
            // A new file is 0644 and belongs to its directory's owner (root,
            // for `/etc/udev/rules.d`).
            let (mode, uid, gid) = match std::fs::metadata(path) {
                Ok(meta) => (meta.mode() & 0o7777, meta.uid(), meta.gid()),
                Err(_) => {
                    let dir = std::fs::metadata(parent).map_err(failed)?;
                    (0o644, dir.uid(), dir.gid())
                }
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let temporary = parent.join(format!(".{name}.better-os.tmp"));
            let _ = std::fs::remove_file(&temporary);
            let result = (|| -> Result<(), DaemonError> {
                use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temporary)
                    .map_err(failed)?;
                file.write_all(text.as_bytes()).map_err(failed)?;
                file.set_permissions(std::fs::Permissions::from_mode(mode))
                    .map_err(failed)?;
                // Only root can give a file away; a test's own file already
                // has its owner.
                let meta = file.metadata().map_err(failed)?;
                if meta.uid() != uid || meta.gid() != gid {
                    std::os::unix::fs::fchown(&file, Some(uid), Some(gid)).map_err(failed)?;
                }
                file.sync_all().map_err(failed)?;
                still()?;
                std::fs::rename(&temporary, path).map_err(failed)
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&temporary);
            }
            result?;
        }
    }
    std::fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(failed)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, DaemonError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(DaemonError::WriteFailed(format!("{}: {e}", path.display()))),
    }
}

/// How long a change waits for another one to finish with the files.
pub const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// An exclusive `flock` on the lock file, held until dropped. Every Better
/// OS writer of the two files takes it; other programs that edit fstab do
/// not, and the check that the file still holds what was read before the
/// rename is what guards against them.
pub struct FileLock {
    /// Held, not read: closing it releases the lock.
    _file: std::fs::File,
}

impl FileLock {
    /// Takes the lock on `paths.lock`. The file is made `0600` and must be
    /// owned by `paths.lock_owner` (root): `flock` needs no write access, so
    /// a file anyone could open would let anyone hold the lock forever. It
    /// is opened without following a link. A lock still held by someone
    /// else after `paths.lock_wait` is
    /// [`DaemonError::MountTableBusy`], which the windows offer Retry for.
    pub fn take(paths: &DrivePaths) -> Result<Self, DaemonError> {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        use std::os::unix::io::AsRawFd as _;
        let path = &paths.lock;
        let failed =
            |e: std::io::Error| DaemonError::WriteFailed(format!("{}: {e}", path.display()));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(failed)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(failed)?;
        let meta = file.metadata().map_err(failed)?;
        if !meta.is_file() || meta.uid() != paths.lock_owner {
            return Err(DaemonError::WriteFailed(format!(
                "{} is not a file of its own owner's; it was left alone",
                path.display()
            )));
        }
        if meta.mode() & 0o7777 != 0o600 {
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(failed)?;
        }
        let deadline = std::time::Instant::now() + paths.lock_wait;
        loop {
            // SAFETY: a valid descriptor owned by `file`, which outlives the lock.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self { _file: file });
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EWOULDBLOCK) {
                return Err(failed(error));
            }
            if std::time::Instant::now() >= deadline {
                return Err(DaemonError::MountTableBusy);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// Everything one call changes, with what it needs from outside.
pub struct DriveChange<'a> {
    pub resolver: &'a dyn DriveResolver,
    pub units: &'a dyn MountUnits,
    pub paths: &'a DrivePaths,
    /// The caller's user and group, for a FAT, exFAT or NTFS drive's files.
    pub owner: Option<(u32, u32)>,
}

impl DriveChange<'_> {
    /// Turns `uuid` on or off and returns where it is mounted (empty when it
    /// was turned off or is removable).
    pub fn apply(&self, uuid: &str, on: bool) -> Result<String, DaemonError> {
        if !valid_uuid(uuid) {
            return Err(DaemonError::InvalidArgument("uuid".to_string()));
        }
        let drive = self
            .resolver
            .resolve(uuid)?
            .ok_or(DaemonError::DriveAbsent)?;
        // From here on the UUID is UDisks2's, not the caller's.
        let uuid = drive.names.uuid.clone();
        if !valid_uuid(&uuid) {
            return Err(DaemonError::DriveRefused("not_a_filesystem".into()));
        }
        let uuid = uuid.as_str();
        let _lock = FileLock::take(self.paths)?;
        // The running system's own drive, and any drive holding a system
        // folder or swap (ticket 171): the same refusal Better Files' menu
        // and the storage service give. A drive whose use could not be told
        // is not refused here: nothing this method does takes it away.
        let held = !matches!(
            drive.system_use,
            drive_mounts::listing::SystemUse::Free
                | drive_mounts::listing::SystemUse::Unknown
                // Refused below as `system_table`, with its own words.
                | drive_mounts::listing::SystemUse::SystemTable
        );
        if held || drive.mount_points.iter().any(|p| p == Path::new("/")) {
            return Err(DaemonError::DriveRefused("system_drive".into()));
        }
        if drive.removable {
            if !matches!(drive.usage.as_str(), "filesystem" | "crypto") {
                return Err(DaemonError::DriveRefused("not_a_filesystem".into()));
            }
            return self.set_removable(uuid, on).map(|()| String::new());
        }
        if drive.usage != "filesystem" || !ENTRY_FILESYSTEMS.contains(&drive.filesystem.as_str()) {
            return Err(DaemonError::DriveRefused("not_a_filesystem".into()));
        }
        if on {
            self.turn_on(uuid, &drive)
        } else {
            self.turn_off(uuid).map(|()| String::new())
        }
    }

    fn read_fstab(&self) -> Result<(Vec<u8>, Fstab), DaemonError> {
        let bytes = std::fs::read(&self.paths.fstab).map_err(|e| {
            DaemonError::WriteFailed(format!("{}: {e}", self.paths.fstab.display()))
        })?;
        let fstab = Fstab::parse_bytes(&bytes)
            .map_err(|e| DaemonError::MountTableMalformed(e.to_string()))?;
        Ok((bytes, fstab))
    }

    fn turn_on(&self, uuid: &str, drive: &ResolvedDrive) -> Result<String, DaemonError> {
        let (original, mut fstab) = self.read_fstab()?;
        if fstab.foreign_line(&drive.names).is_some() {
            return Err(DaemonError::DriveRefused("system_table".into()));
        }
        let owner = if OWNERLESS.contains(&drive.filesystem.as_str()) {
            Some(self.owner.ok_or_else(|| {
                DaemonError::WriteFailed("the caller's user could not be found".into())
            })?)
        } else {
            None
        };
        let entry = Entry::new(uuid, &drive.filesystem, owner)
            .ok_or_else(|| DaemonError::DriveRefused("not_a_filesystem".into()))?;
        fstab.set(entry);
        let text = fstab.render();
        if text.as_bytes() == original.as_slice() {
            // Already ours and unchanged: a Retry. Nothing to roll back to.
            self.units.reload().map_err(DaemonError::WriteFailed)?;
            return self.mount_now(uuid, drive);
        }
        replace_file(&self.paths.fstab, Some(&original), Some(&text))?;
        // A line that cannot be mounted now is taken out again, so "on"
        // never stays written for a drive that did not come up (ADR 0032).
        // A drive that fails at a later start keeps its line and shows
        // Retry.
        let result = self
            .units
            .reload()
            .map_err(DaemonError::WriteFailed)
            .and_then(|()| self.mount_now(uuid, drive));
        if let Err(failure) = &result {
            let rollback = replace_file(
                &self.paths.fstab,
                Some(text.as_bytes()),
                Some(std::str::from_utf8(&original).unwrap_or_default()),
            );
            let _ = self.units.reload();
            self.remove_mount_point(&mount_point(uuid));
            if let Err(error) = rollback {
                return Err(DaemonError::WriteFailed(format!(
                    "{failure}; and the line could not be taken out again: {error}"
                )));
            }
        }
        result
    }

    /// Mounted already (the person opened it in Files): it stays where it
    /// is, and the line takes over from the next start. Otherwise systemd
    /// mounts it now.
    fn mount_now(&self, uuid: &str, drive: &ResolvedDrive) -> Result<String, DaemonError> {
        if let Some(point) = drive.mount_points.first() {
            return Ok(point.display().to_string());
        }
        let point = mount_point(uuid);
        self.units
            .start(&mount_unit(&point))
            .map_err(DaemonError::MountFailed)?;
        Ok(point.display().to_string())
    }

    fn turn_off(&self, uuid: &str) -> Result<(), DaemonError> {
        let (original, mut fstab) = self.read_fstab()?;
        let Some(entry) = fstab.entry(uuid) else {
            // Not Better OS's: nothing of ours to take away. A mount the
            // person made is theirs to unmount, through UDisks2.
            return Ok(());
        };
        // The line's own spelling names its mount point and unit.
        let written = entry.uuid.clone();
        let uuid = written.as_str();
        let point = mount_point(uuid);
        self.units
            .stop(&mount_unit(&point))
            .map_err(DaemonError::DriveBusy)?;
        fstab.remove(uuid);
        replace_file(&self.paths.fstab, Some(&original), Some(&fstab.render()))?;
        self.units.reload().map_err(DaemonError::WriteFailed)?;
        self.remove_mount_point(&point);
        Ok(())
    }

    fn remove_mount_point(&self, point: &Path) {
        let local = self.paths.local(point);
        let _ = std::fs::remove_dir(&local);
        if let Some(base) = local.parent() {
            let _ = std::fs::remove_dir(base);
        }
    }

    fn set_removable(&self, uuid: &str, on: bool) -> Result<(), DaemonError> {
        let original = read_optional(&self.paths.rules)?;
        let mut rules = match &original {
            Some(bytes) => Rules::parse_bytes(bytes)
                .map_err(|e| DaemonError::MountTableMalformed(e.to_string()))?,
            None => Rules::default(),
        };
        if rules.set_off(uuid, !on) {
            replace_file(
                &self.paths.rules,
                original.as_deref(),
                rules.render().as_deref(),
            )?;
            self.units.reload_udev();
        }
        Ok(())
    }

    /// The daemon package's removal: every entry and rule Better OS wrote
    /// goes, and what anyone else wrote stays. Returns what it could not do,
    /// which the prerm prints and does not fail on.
    pub fn remove_all(units: &dyn MountUnits, paths: &DrivePaths) -> Vec<String> {
        let mut problems = Vec::new();
        let _lock = match FileLock::take(paths) {
            Ok(lock) => Some(lock),
            Err(error) => {
                problems.push(error.to_string());
                None
            }
        };
        match std::fs::read(&paths.fstab) {
            Ok(original) => match Fstab::parse_bytes(&original) {
                Ok(mut fstab) => {
                    let entries = fstab.clear();
                    if !entries.is_empty() {
                        for entry in &entries {
                            let unit = mount_unit(&mount_point(&entry.uuid));
                            if let Err(e) = units.stop(&unit) {
                                problems.push(format!("{unit}: {e}"));
                            }
                        }
                        match replace_file(&paths.fstab, Some(&original), Some(&fstab.render())) {
                            Ok(()) => {
                                if let Err(e) = units.reload() {
                                    problems.push(e);
                                }
                                for entry in &entries {
                                    let local = paths.local(&mount_point(&entry.uuid));
                                    let _ = std::fs::remove_dir(&local);
                                }
                                let _ = std::fs::remove_dir(
                                    paths.local(Path::new(drive_mounts::ids::MOUNT_BASE)),
                                );
                            }
                            Err(e) => problems.push(e.to_string()),
                        }
                    }
                }
                Err(e) => problems.push(format!(
                    "{}: {e}; Better OS's lines were left in place",
                    paths.fstab.display()
                )),
            },
            Err(e) => problems.push(format!("{}: {e}", paths.fstab.display())),
        }
        match read_optional(&paths.rules) {
            Ok(Some(original)) => match Rules::parse_bytes(&original) {
                Ok(_) => match replace_file(&paths.rules, Some(&original), None) {
                    Ok(()) => units.reload_udev(),
                    Err(e) => problems.push(e.to_string()),
                },
                Err(e) => problems.push(format!(
                    "{}: {e}; the file was left in place",
                    paths.rules.display()
                )),
            },
            Ok(None) => {}
            Err(e) => problems.push(e.to_string()),
        }
        problems
    }
}

/// A user's primary group, from the system's user database.
pub fn primary_group(uid: u32) -> Option<u32> {
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is to a live local of the size passed.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    (status == 0 && !result.is_null()).then_some(entry.pw_gid)
}

pub struct DrivesService<A: Authorizer + 'static> {
    authorizer: A,
    resolver: Arc<dyn DriveResolver>,
    units: Arc<dyn MountUnits>,
    paths: DrivePaths,
    /// One change at a time: two would each read the file the other is
    /// about to replace.
    lock: Arc<Mutex<()>>,
    gate: Arc<CallGate>,
    /// See `ManagerService::runtime`.
    runtime: tokio::runtime::Handle,
}

impl<A: Authorizer + 'static> DrivesService<A> {
    pub fn new(
        authorizer: A,
        resolver: Arc<dyn DriveResolver>,
        units: Arc<dyn MountUnits>,
        paths: DrivePaths,
        gate: Arc<CallGate>,
    ) -> Self {
        Self {
            authorizer,
            resolver,
            units,
            paths,
            lock: Arc::default(),
            gate,
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

#[interface(name = "org.betteros.Drives1")]
impl<A: Authorizer + 'static> DrivesService<A> {
    /// Turns "Mount automatically" on or off for the drive whose filesystem
    /// has `uuid`. Returns where it is mounted: an internal drive turned on;
    /// empty otherwise.
    async fn set_mount_automatically(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        uuid: &str,
        on: bool,
    ) -> Result<String, fdo::Error> {
        let _call = self.gate.enter().map_err(refuse)?;
        if !valid_uuid(uuid) {
            // Not echoed: it is free text from the caller.
            return Err(refuse(DaemonError::InvalidArgument("uuid".to_string())));
        }
        match self.authorizer.decide(&header, DRIVE_MOUNTING_ACTION).await {
            Ok(Authorization::Granted) => {}
            Ok(Authorization::Dismissed) => return Err(refuse(DaemonError::AuthCancelled)),
            Ok(Authorization::Denied) => return Err(refuse(DaemonError::Unauthorized)),
            Err(error) => return Err(refuse(error)),
        }
        let uid = match header.sender() {
            Some(sender) => fdo::DBusProxy::new(connection)
                .await?
                .get_connection_unix_user(sender.clone().into())
                .await
                .ok(),
            None => None,
        };
        let owner = uid.and_then(|uid| primary_group(uid).map(|gid| (uid, gid)));
        let uuid = uuid.to_string();
        let resolver = self.resolver.clone();
        let units = self.units.clone();
        let paths = self.paths.clone();
        let lock = self.lock.clone();
        self.runtime
            .spawn_blocking(move || {
                let _held = lock.lock().unwrap_or_else(|p| p.into_inner());
                DriveChange {
                    resolver: resolver.as_ref(),
                    units: units.as_ref(),
                    paths: &paths,
                    owner,
                }
                .apply(&uuid, on)
            })
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?
            .map_err(refuse)
    }
}

/// Fakes for tests: a resolver from a table, and units that record.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use super::*;

    #[derive(Default)]
    pub struct TableResolver(pub HashMap<String, ResolvedDrive>);

    impl DriveResolver for TableResolver {
        fn resolve(&self, uuid: &str) -> Result<Option<ResolvedDrive>, DaemonError> {
            // UDisks2 finds a UUID whatever its case.
            Ok(self
                .0
                .iter()
                .find(|(known, _)| known.eq_ignore_ascii_case(uuid))
                .map(|(_, drive)| drive.clone()))
        }
    }

    #[derive(Default)]
    pub struct RecordingUnits {
        pub calls: Mutex<Vec<String>>,
        /// A start fails with this.
        pub start_fails: Option<String>,
        /// A stop fails with this.
        pub stop_fails: Option<String>,
    }

    impl RecordingUnits {
        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }
    }

    impl MountUnits for RecordingUnits {
        fn reload(&self) -> Result<(), String> {
            self.record("reload".into());
            Ok(())
        }
        fn start(&self, unit: &str) -> Result<(), String> {
            self.record(format!("start {unit}"));
            self.start_fails.clone().map_or(Ok(()), Err)
        }
        fn stop(&self, unit: &str) -> Result<(), String> {
            self.record(format!("stop {unit}"));
            self.stop_fails.clone().map_or(Ok(()), Err)
        }
        fn reload_udev(&self) {
            self.record("reload-udev".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use drive_mounts::fstab::{BEGIN, END};

    const SYSTEM: &str = "UUID=1111-2222 / ext4 errors=remount-ro 0 1\n\
UUID=7A3B-11CC /boot/efi vfat umask=0077 0 1\n\
LABEL=Backup /backup ext4 defaults,noauto 0 2\n";

    struct Machine {
        dir: tempfile::TempDir,
        paths: DrivePaths,
    }

    impl Machine {
        fn new(fstab: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join("etc/udev/rules.d")).unwrap();
            std::fs::write(dir.path().join("etc/fstab"), fstab).unwrap();
            let paths = DrivePaths {
                fstab: dir.path().join("etc/fstab"),
                rules: dir
                    .path()
                    .join("etc/udev/rules.d/90-better-os-drives.rules"),
                mount_root: dir.path().to_path_buf(),
                lock: dir.path().join("run/fstab.lock"),
                // SAFETY: getuid has no preconditions.
                lock_owner: unsafe { libc::getuid() },
                lock_wait: LOCK_WAIT,
            };
            Self { dir, paths }
        }
        fn fstab(&self) -> String {
            std::fs::read_to_string(&self.paths.fstab).unwrap()
        }
    }

    fn drives() -> TableResolver {
        let drive = |filesystem: &str, removable: bool, mounted: &[&str], label: Option<&str>| {
            ResolvedDrive {
                filesystem: filesystem.into(),
                usage: if filesystem == "swap" {
                    "other".into()
                } else {
                    "filesystem".into()
                },
                removable,
                mount_points: mounted.iter().map(PathBuf::from).collect(),
                names: DriveNames {
                    label: label.map(str::to_string),
                    ..DriveNames::default()
                },
                system_use: if mounted.contains(&"/") {
                    drive_mounts::listing::SystemUse::RunningSystem
                } else {
                    drive_mounts::listing::SystemUse::Free
                },
            }
        };
        let mut table = HashMap::new();
        for (uuid, d) in [
            (
                "01D9F00DBEEF1234",
                drive("ntfs", false, &[], Some("Windows")),
            ),
            ("DA7A-0001", drive("ext4", false, &["/media/me/Data"], None)),
            ("1111-2222", drive("ext4", false, &["/"], None)),
            ("BACC-0001", drive("ext4", false, &[], Some("Backup"))),
            ("5AA5-0001", drive("swap", false, &[], None)),
            // A data partition on the USB drive the system runs from, and
            // `/home` on a USB disk (ticket 171).
            ("5157-0001", {
                let mut d = drive("ext4", true, &["/media/me/DATA"], None);
                d.system_use = drive_mounts::listing::SystemUse::RunningSystem;
                d
            }),
            ("0403-0001", {
                let mut d = drive("ext4", true, &["/home"], None);
                d.system_use = drive_mounts::listing::SystemUse::SystemFolder {
                    path: "/home".into(),
                };
                d
            }),
            (
                "AB12-CD34",
                drive("exfat", true, &["/media/me/STICK"], None),
            ),
        ] {
            let mut d = d;
            d.names.uuid = uuid.into();
            table.insert(uuid.to_string(), d);
        }
        TableResolver(table)
    }

    fn change<'a>(
        resolver: &'a TableResolver,
        units: &'a RecordingUnits,
        machine: &'a Machine,
    ) -> DriveChange<'a> {
        DriveChange {
            resolver,
            units,
            paths: &machine.paths,
            owner: Some((1000, 1000)),
        }
    }

    #[test]
    fn an_internal_drive_turned_on_is_one_line_and_a_started_unit() {
        let machine = Machine::new(SYSTEM);
        let resolver = drives();
        let units = RecordingUnits::default();
        let at = change(&resolver, &units, &machine)
            .apply("01D9F00DBEEF1234", true)
            .unwrap();
        assert_eq!(at, "/mnt/better-os/01D9F00DBEEF1234");
        assert_eq!(
            machine.fstab(),
            format!(
                "{SYSTEM}{BEGIN}\nUUID=01D9F00DBEEF1234 /mnt/better-os/01D9F00DBEEF1234 ntfs nosuid,nodev,nofail,x-gvfs-show,x-systemd.device-timeout=10s,uid=1000,gid=1000 0 0\n{END}\n"
            )
        );
        assert_eq!(
            units.calls(),
            ["reload", "start mnt-better\\x2dos-01D9F00DBEEF1234.mount"]
        );
        // Off: unmounted, the line and the block gone, the file as it was.
        std::fs::create_dir_all(machine.dir.path().join("mnt/better-os/01D9F00DBEEF1234")).unwrap();
        change(&resolver, &units, &machine)
            .apply("01D9F00DBEEF1234", false)
            .unwrap();
        assert_eq!(machine.fstab(), SYSTEM);
        assert!(!machine.dir.path().join("mnt/better-os").exists());
        assert_eq!(
            units.calls()[2..],
            [
                "stop mnt-better\\x2dos-01D9F00DBEEF1234.mount".to_string(),
                "reload".to_string()
            ]
        );
    }

    #[test]
    fn a_drive_already_mounted_stays_where_it_is() {
        let machine = Machine::new(SYSTEM);
        let resolver = drives();
        let units = RecordingUnits::default();
        let at = change(&resolver, &units, &machine)
            .apply("DA7A-0001", true)
            .unwrap();
        assert_eq!(at, "/media/me/Data");
        assert_eq!(units.calls(), ["reload"]);
        // ext4 has owners: no uid.
        assert!(
            machine.fstab().contains(
                "ext4 nosuid,nodev,nofail,x-gvfs-show,x-systemd.device-timeout=10s 0 0\n"
            )
        );
    }

    #[test]
    fn what_is_not_ours_is_refused_and_nothing_is_written() {
        let machine = Machine::new(SYSTEM);
        let resolver = drives();
        let units = RecordingUnits::default();
        let apply = |uuid: &str, on: bool| change(&resolver, &units, &machine).apply(uuid, on);
        assert_eq!(
            apply("1111-2222", true),
            Err(DaemonError::DriveRefused("system_drive".into()))
        );
        assert_eq!(
            apply("BACC-0001", true),
            Err(DaemonError::DriveRefused("system_table".into()))
        );
        for held in ["5157-0001", "0403-0001"] {
            for on in [true, false] {
                assert_eq!(
                    apply(held, on),
                    Err(DaemonError::DriveRefused("system_drive".into()))
                );
            }
        }
        assert_eq!(
            apply("5AA5-0001", true),
            Err(DaemonError::DriveRefused("not_a_filesystem".into()))
        );
        assert_eq!(apply("FFFF-0000", true), Err(DaemonError::DriveAbsent));
        assert_eq!(
            apply("../../etc", true),
            Err(DaemonError::InvalidArgument("uuid".into()))
        );
        // Off for a drive Better OS never wrote a line for changes nothing.
        assert_eq!(apply("BACC-0001", false), Ok(String::new()));
        assert_eq!(machine.fstab(), SYSTEM);
        assert!(units.calls().is_empty());
    }

    #[test]
    fn a_malformed_fstab_is_never_written() {
        let broken = format!("{SYSTEM}{BEGIN}\n");
        let machine = Machine::new(&broken);
        let resolver = drives();
        let units = RecordingUnits::default();
        assert!(matches!(
            change(&resolver, &units, &machine).apply("01D9F00DBEEF1234", true),
            Err(DaemonError::MountTableMalformed(_))
        ));
        assert_eq!(machine.fstab(), broken);
    }

    #[test]
    fn a_line_that_cannot_be_mounted_is_taken_out_again() {
        let machine = Machine::new(SYSTEM);
        let resolver = drives();
        let units = RecordingUnits {
            start_fails: Some("exit-code: wrong fs type".into()),
            ..RecordingUnits::default()
        };
        assert_eq!(
            change(&resolver, &units, &machine).apply("01D9F00DBEEF1234", true),
            Err(DaemonError::MountFailed("exit-code: wrong fs type".into()))
        );
        assert_eq!(machine.fstab(), SYSTEM);
        assert_eq!(
            units.calls(),
            [
                "reload",
                "start mnt-better\\x2dos-01D9F00DBEEF1234.mount",
                "reload"
            ]
        );
    }

    #[test]
    fn a_busy_drive_keeps_its_line_and_its_mount() {
        let machine = Machine::new(SYSTEM);
        let resolver = drives();
        change(&resolver, &RecordingUnits::default(), &machine)
            .apply("01D9F00DBEEF1234", true)
            .unwrap();
        let written = machine.fstab();
        let busy = RecordingUnits {
            stop_fails: Some("target is busy".into()),
            ..RecordingUnits::default()
        };
        assert_eq!(
            change(&resolver, &busy, &machine).apply("01D9F00DBEEF1234", false),
            Err(DaemonError::DriveBusy("target is busy".into()))
        );
        assert_eq!(machine.fstab(), written);
        // A Retry of a line already there writes nothing and mounts again.
        let units = RecordingUnits::default();
        change(&resolver, &units, &machine)
            .apply("01D9F00DBEEF1234", true)
            .unwrap();
        assert_eq!(machine.fstab(), written);
        assert_eq!(
            units.calls(),
            ["reload", "start mnt-better\\x2dos-01D9F00DBEEF1234.mount"]
        );
    }

    #[test]
    fn the_line_uses_udisks_spelling_whatever_case_was_asked() {
        let machine = Machine::new(SYSTEM);
        let resolver = drives();
        let units = RecordingUnits::default();
        change(&resolver, &units, &machine)
            .apply("01d9f00dbeef1234", true)
            .unwrap();
        assert!(
            machine
                .fstab()
                .contains("UUID=01D9F00DBEEF1234 /mnt/better-os/01D9F00DBEEF1234 ")
        );
        change(&resolver, &units, &machine)
            .apply("01d9F00DBEEF1234", false)
            .unwrap();
        assert_eq!(machine.fstab(), SYSTEM);
        assert_eq!(
            units.calls()[2],
            "stop mnt-better\\x2dos-01D9F00DBEEF1234.mount"
        );
        change(&resolver, &units, &machine)
            .apply("ab12-cd34", false)
            .unwrap();
        assert!(
            std::fs::read_to_string(&machine.paths.rules)
                .unwrap()
                .contains("==\"AB12-CD34\"")
        );
    }

    #[test]
    fn a_usb_disk_that_says_it_is_not_removable_never_gets_an_fstab_line() {
        // What the resolver concludes from Removable=false on a usb bus.
        let machine = Machine::new(SYSTEM);
        let mut resolver = drives();
        let mut usb = resolver.0.get("BACC-0001").unwrap().clone();
        usb.names.uuid = "05B0-0001".into();
        usb.names.label = None;
        usb.removable = drive_mounts::listing::hot_pluggable(false, false, "usb");
        resolver.0.insert("05B0-0001".into(), usb);
        let units = RecordingUnits::default();
        change(&resolver, &units, &machine)
            .apply("05B0-0001", true)
            .unwrap();
        change(&resolver, &units, &machine)
            .apply("05B0-0001", false)
            .unwrap();
        assert_eq!(machine.fstab(), SYSTEM);
        assert!(
            std::fs::read_to_string(&machine.paths.rules)
                .unwrap()
                .contains("05B0-0001")
        );
    }

    #[test]
    fn a_linked_fstab_is_refused_and_left_alone() {
        let machine = Machine::new(SYSTEM);
        let real = machine.dir.path().join("etc/fstab.real");
        std::fs::rename(&machine.paths.fstab, &real).unwrap();
        std::os::unix::fs::symlink(&real, &machine.paths.fstab).unwrap();
        let resolver = drives();
        let units = RecordingUnits::default();
        let refused = change(&resolver, &units, &machine).apply("01D9F00DBEEF1234", true);
        assert!(
            matches!(&refused, Err(DaemonError::WriteFailed(why)) if why.contains("symbolic link")),
            "{refused:?}"
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), SYSTEM);
        assert!(
            std::fs::symlink_metadata(&machine.paths.fstab)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_change_waits_for_the_lock() {
        let machine = Machine::new(SYSTEM);
        let held = FileLock::take(&machine.paths).unwrap();
        let paths = machine.paths.clone();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        let worker = std::thread::spawn(move || {
            let resolver = drives();
            let units = RecordingUnits::default();
            let result = DriveChange {
                resolver: &resolver,
                units: &units,
                paths: &paths,
                owner: Some((1000, 1000)),
            }
            .apply("01D9F00DBEEF1234", true);
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            result
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!done.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(machine.fstab(), SYSTEM);
        drop(held);
        assert!(worker.join().unwrap().is_ok());
        assert!(machine.fstab().contains("UUID=01D9F00DBEEF1234"));
    }

    #[test]
    fn a_removable_drive_turned_off_is_a_udev_rule() {
        let machine = Machine::new(SYSTEM);
        let resolver = drives();
        let units = RecordingUnits::default();
        change(&resolver, &units, &machine)
            .apply("AB12-CD34", false)
            .unwrap();
        let rules = std::fs::read_to_string(&machine.paths.rules).unwrap();
        assert!(rules.contains("ENV{ID_FS_UUID}==\"AB12-CD34\", ENV{UDISKS_AUTO}=\"0\"\n"));
        assert_eq!(machine.fstab(), SYSTEM);
        change(&resolver, &units, &machine)
            .apply("AB12-CD34", true)
            .unwrap();
        assert!(!machine.paths.rules.exists());
        assert_eq!(units.calls(), ["reload-udev", "reload-udev"]);
        std::fs::write(&machine.paths.rules, "KERNEL==\"sd*\", RUN+=\"/x\"\n").unwrap();
        assert!(matches!(
            change(&resolver, &units, &machine).apply("AB12-CD34", false),
            Err(DaemonError::MountTableMalformed(_))
        ));
    }

    #[test]
    fn removal_takes_every_line_and_rule_and_leaves_the_rest() {
        let machine = Machine::new(SYSTEM);
        let resolver = drives();
        let units = RecordingUnits::default();
        change(&resolver, &units, &machine)
            .apply("01D9F00DBEEF1234", true)
            .unwrap();
        change(&resolver, &units, &machine)
            .apply("AB12-CD34", false)
            .unwrap();
        std::fs::create_dir_all(machine.dir.path().join("mnt/better-os/01D9F00DBEEF1234")).unwrap();
        std::fs::create_dir_all(machine.dir.path().join("mnt/other")).unwrap();
        let problems = DriveChange::remove_all(&units, &machine.paths);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(machine.fstab(), SYSTEM);
        assert!(!machine.paths.rules.exists());
        assert!(!machine.dir.path().join("mnt/better-os").exists());
        assert!(machine.dir.path().join("mnt/other").exists());
        // Nothing of ours: nothing happens.
        let units = RecordingUnits::default();
        assert!(DriveChange::remove_all(&units, &machine.paths).is_empty());
        assert!(units.calls().is_empty());
    }

    #[test]
    fn the_file_is_replaced_whole_keeps_its_mode_and_refuses_a_file_that_moved() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fstab");
        std::fs::write(&path, "a\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        replace_file(&path, Some(b"a\n"), Some("b\n")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "b\n");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert!(replace_file(&path, Some(b"a\n"), Some("c\n")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "b\n");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_lock_someone_keeps_makes_the_change_busy_within_the_bound() {
        let mut machine = Machine::new(SYSTEM);
        machine.paths.lock_wait = std::time::Duration::from_millis(300);
        let other = FileLock::take(&machine.paths).unwrap();
        let started = std::time::Instant::now();
        let resolver = drives();
        let units = RecordingUnits::default();
        assert_eq!(
            change(&resolver, &units, &machine).apply("01D9F00DBEEF1234", true),
            Err(DaemonError::MountTableBusy)
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(machine.fstab(), SYSTEM);
        // The package's removal says so and goes on.
        let problems = DriveChange::remove_all(&units, &machine.paths);
        assert_eq!(problems.len(), 1, "{problems:?}");
        drop(other);
    }

    #[test]
    fn a_world_readable_lock_file_is_tightened() {
        use std::os::unix::fs::PermissionsExt as _;
        let machine = Machine::new(SYSTEM);
        std::fs::create_dir_all(machine.paths.lock.parent().unwrap()).unwrap();
        std::fs::write(&machine.paths.lock, "").unwrap();
        std::fs::set_permissions(&machine.paths.lock, std::fs::Permissions::from_mode(0o644))
            .unwrap();
        drop(FileLock::take(&machine.paths).unwrap());
        let mode = std::fs::metadata(&machine.paths.lock)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        // A new one is made 0600 from the start.
        std::fs::remove_file(&machine.paths.lock).unwrap();
        drop(FileLock::take(&machine.paths).unwrap());
        let mode = std::fs::metadata(&machine.paths.lock)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_lock_file_of_someone_else_or_a_link_is_refused() {
        let mut machine = Machine::new(SYSTEM);
        machine.paths.lock_owner = machine.paths.lock_owner.wrapping_add(1);
        let resolver = drives();
        let units = RecordingUnits::default();
        assert!(matches!(
            change(&resolver, &units, &machine).apply("01D9F00DBEEF1234", true),
            Err(DaemonError::WriteFailed(_))
        ));
        assert_eq!(machine.fstab(), SYSTEM);
        let machine = Machine::new(SYSTEM);
        std::fs::create_dir_all(machine.paths.lock.parent().unwrap()).unwrap();
        let target = machine.dir.path().join("elsewhere");
        std::fs::write(&target, "").unwrap();
        std::os::unix::fs::symlink(&target, &machine.paths.lock).unwrap();
        assert!(FileLock::take(&machine.paths).is_err());
    }
}
