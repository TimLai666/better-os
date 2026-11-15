//! Whether each drive mounts automatically, read from the system's own files
//! (ADR 0032), and which drives the sidebar shows (ticket 127).
//!
//! Defaults, set by the owner on 2026-11-15: the system drive and removable
//! drives are on; another internal partition is on only when the system
//! mounts it at boot today, through a line someone else wrote, or because
//! Better OS was asked to.

use crate::fstab::{DriveNames, Fstab};
use crate::listing::{Drive, DriveKind};
use crate::rules::Rules;

/// Why a drive's choice cannot be changed here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Locked {
    /// The drive the system runs from.
    SystemDrive,
    /// A line in `/etc/fstab` that Better OS did not write names it; that
    /// line decides, and Better OS never touches it.
    SystemTable { line: usize, mount_point: String },
    /// `/etc/fstab` or the rules file could not be read or is malformed;
    /// what it says is unknown, and nothing is written to it.
    Unreadable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoMount {
    /// Whether it mounts at login or on arrival.
    pub on: bool,
    pub locked: Option<Locked>,
    /// Why the system could not mount a drive that is on, when it said.
    pub failure: Option<String>,
}

impl AutoMount {
    fn open(on: bool) -> Self {
        Self {
            on,
            locked: None,
            failure: None,
        }
    }
}

/// What a scan read of the system's files: each `Err` is why it could not.
pub struct SystemFiles {
    pub fstab: Result<Fstab, String>,
    pub rules: Result<Rules, String>,
}

impl SystemFiles {
    /// Reads `/etc/fstab` and the rules file, or the files at the given
    /// paths (fixtures). A rules file that does not exist is an empty one.
    pub fn read(fstab: &std::path::Path, rules: &std::path::Path) -> Self {
        let fstab = std::fs::read(fstab)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Fstab::parse_bytes(&bytes).map_err(|e| e.to_string()));
        let rules = match std::fs::read(rules) {
            Ok(bytes) => Rules::parse_bytes(&bytes).map_err(|e| e.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Rules::default()),
            Err(error) => Err(error.to_string()),
        };
        Self { fstab, rules }
    }

    /// What a window reads: the system's files, or in a debug build the
    /// fixtures `BETTER_OS_DEBUG_FSTAB` and `BETTER_OS_DEBUG_DRIVE_RULES`
    /// name, so a sandbox never shows the machine's own.
    pub fn for_window() -> Self {
        if cfg!(debug_assertions)
            && let Some(fstab) = std::env::var_os("BETTER_OS_DEBUG_FSTAB")
        {
            let rules = std::env::var_os("BETTER_OS_DEBUG_DRIVE_RULES")
                .unwrap_or_else(|| "/nonexistent/better-os-drive-rules".into());
            return Self::read(std::path::Path::new(&fstab), std::path::Path::new(&rules));
        }
        Self::system()
    }

    pub fn system() -> Self {
        Self::read(
            std::path::Path::new("/etc/fstab"),
            std::path::Path::new(crate::rules::RULES_PATH),
        )
    }
}

/// The names a listed drive goes by.
pub fn names_of(drive: &Drive) -> Option<DriveNames> {
    Some(DriveNames {
        uuid: drive.uuid.clone()?,
        label: drive.label.clone(),
        partuuid: None,
        device_paths: drive.device_path.iter().cloned().collect(),
    })
}

/// A drive's choice, or `None` when it has none (a phone, a share, a drive
/// with no UUID).
pub fn setting(drive: &Drive, files: &SystemFiles) -> Option<AutoMount> {
    if drive.kind == DriveKind::System {
        return Some(AutoMount {
            on: true,
            locked: Some(Locked::SystemDrive),
            failure: None,
        });
    }
    let names = names_of(drive)?;
    match drive.kind {
        DriveKind::System | DriveKind::Phone | DriveKind::Network => None,
        DriveKind::Removable | DriveKind::Optical => Some(match &files.rules {
            Ok(rules) => AutoMount::open(!rules.is_off(&names.uuid)),
            Err(error) => AutoMount {
                on: true,
                locked: Some(Locked::Unreadable(error.clone())),
                failure: None,
            },
        }),
        DriveKind::Internal => Some(match &files.fstab {
            Ok(fstab) => {
                if fstab.entry(&names.uuid).is_some() {
                    AutoMount::open(true)
                } else if let Some(line) = fstab.foreign_line(&names) {
                    AutoMount {
                        on: !line.noauto,
                        locked: Some(Locked::SystemTable {
                            line: line.line,
                            mount_point: line.mount_point,
                        }),
                        failure: None,
                    }
                } else {
                    AutoMount::open(false)
                }
            }
            Err(error) => AutoMount {
                on: drive.mount_point.is_some(),
                locked: Some(Locked::Unreadable(error.clone())),
                failure: None,
            },
        }),
    }
}

/// Fills every drive's choice.
pub fn apply(drives: &mut [Drive], files: &SystemFiles) {
    for drive in drives {
        drive.auto_mount = setting(drive, files);
    }
}

/// Whether the sidebar lists it: a drive that is mounted, or one that should
/// be and is not (shown with why, and Retry), or a locked encrypted drive
/// that is on (it waits for its passphrase). A drive that is off and not
/// mounted is not shown; Better Manager's Drives section and "Show other
/// drives" reach it.
pub fn shown(drive: &Drive) -> bool {
    drive.mount_point.is_some()
        || drive
            .auto_mount
            .as_ref()
            .is_some_and(|auto| auto.on || matches!(auto.locked, Some(Locked::Unreadable(_))))
}

/// An internal drive that is on and should have been mounted at start-up
/// but is not: shown with the reason and Retry. A removable drive that is on
/// and not mounted is not a failure: the desktop mounts it as it arrives,
/// and until then it is shown "Not mounted" and mounts when clicked.
pub fn failed(drive: &Drive) -> bool {
    drive.kind == DriveKind::Internal
        && drive.mount_point.is_none()
        && !drive.locked
        && drive
            .auto_mount
            .as_ref()
            .is_some_and(|auto| auto.on && auto.locked.is_none())
}

/// What a window does itself once the daemon changed a drive's choice: a
/// removable drive turned on is mounted now, and a mount UDisks2 made (a
/// stick, a partition the person opened) of a drive turned off is
/// unmounted. A mount under [`crate::ids::MOUNT_BASE`] is the daemon's, and
/// the daemon already unmounted it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FollowUp {
    Nothing,
    Mount,
    Unmount,
}

pub fn follow_up(drive: &Drive, on: bool) -> FollowUp {
    let removable = matches!(drive.kind, DriveKind::Removable | DriveKind::Optical);
    match (&drive.mount_point, on) {
        (None, true) if removable && !drive.locked => FollowUp::Mount,
        (Some(point), false)
            if drive.kind != DriveKind::System && !point.starts_with(crate::ids::MOUNT_BASE) =>
        {
            FollowUp::Unmount
        }
        _ => FollowUp::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fstab::{BEGIN, END, Entry};

    fn drive(kind: DriveKind, uuid: Option<&str>, mounted: bool) -> Drive {
        Drive {
            key: "k".into(),
            label: Some("Data".into()),
            kind,
            mount_point: mounted.then(|| "/media/me/Data".into()),
            object_path: None,
            device_path: Some("/dev/sdb1".into()),
            drive_object: None,
            backing: None,
            locked: false,
            uuid: uuid.map(str::to_string),
            size: 1,
            ejectable: false,
            capacity: None,
            auto_mount: None,
            form: None,
        }
    }

    fn files(fstab: &str, rules: &str) -> SystemFiles {
        SystemFiles {
            fstab: Fstab::parse(fstab).map_err(|e| e.to_string()),
            rules: Rules::parse(rules).map_err(|e| e.to_string()),
        }
    }

    #[test]
    fn the_owner_s_defaults() {
        let none = files("UUID=AA / ext4 defaults 0 1\n", "");
        let system = setting(&drive(DriveKind::System, Some("AA"), true), &none).unwrap();
        assert!(system.on);
        assert_eq!(system.locked, Some(Locked::SystemDrive));
        assert!(
            setting(&drive(DriveKind::Removable, Some("AB"), false), &none)
                .unwrap()
                .on
        );
        assert!(
            setting(&drive(DriveKind::Optical, Some("AB"), false), &none)
                .unwrap()
                .on
        );
        // A Windows partition nobody mounts at boot: off until turned on.
        assert_eq!(
            setting(&drive(DriveKind::Internal, Some("01D9"), false), &none),
            Some(AutoMount::open(false))
        );
        assert_eq!(setting(&drive(DriveKind::Phone, None, true), &none), None);
        assert_eq!(
            setting(&drive(DriveKind::Internal, None, false), &none),
            None
        );
    }

    #[test]
    fn the_system_s_files_decide() {
        let entry = Entry::new("01D9", "ntfs", None).unwrap().line();
        let ours = files(&format!("{BEGIN}\n{entry}\n{END}\n"), "");
        assert!(
            setting(&drive(DriveKind::Internal, Some("01D9"), false), &ours)
                .unwrap()
                .on
        );
        let theirs = files("LABEL=Data /data ext4 defaults,noauto 0 2\n", "");
        let theirs = setting(&drive(DriveKind::Internal, Some("EE"), false), &theirs).unwrap();
        assert!(!theirs.on);
        assert!(matches!(
            theirs.locked,
            Some(Locked::SystemTable { line: 1, .. })
        ));
        let off = files("", "ENV{ID_FS_UUID}==\"AB\", ENV{UDISKS_AUTO}=\"0\"\n");
        assert!(
            !setting(&drive(DriveKind::Removable, Some("AB"), false), &off)
                .unwrap()
                .on
        );
        let broken = files(&format!("{BEGIN}\n"), "");
        let broken = setting(&drive(DriveKind::Internal, Some("EE"), false), &broken).unwrap();
        assert!(matches!(broken.locked, Some(Locked::Unreadable(_))));
    }

    #[test]
    fn the_sidebar_shows_only_what_is_mounted_or_should_be() {
        let none = files("", "");
        let mut list = vec![
            drive(DriveKind::Internal, Some("01D9"), false),
            drive(DriveKind::Internal, Some("DA7A"), true),
            drive(DriveKind::Removable, Some("AB12"), false),
            drive(DriveKind::Removable, Some("CD34"), true),
        ];
        apply(&mut list, &none);
        let visible: Vec<bool> = list.iter().map(shown).collect();
        assert_eq!(visible, [false, true, true, true]);
        // The stick that is on and not mounted is shown and waits for the
        // desktop; it is not a failure.
        assert!(!failed(&list[2]));
        assert!(!failed(&list[0]) && !failed(&list[1]));
        // An internal drive Better OS mounts at start-up that is not mounted
        // is one, with Retry.
        let entry = Entry::new("01D9", "ntfs", None).unwrap().line();
        let mut windows = drive(DriveKind::Internal, Some("01D9"), false);
        apply(
            std::slice::from_mut(&mut windows),
            &files(&format!("{BEGIN}\n{entry}\n{END}\n"), ""),
        );
        assert!(shown(&windows) && failed(&windows));
        let mut locked = drive(DriveKind::Removable, Some("1234"), false);
        locked.locked = true;
        apply(std::slice::from_mut(&mut locked), &none);
        assert!(shown(&locked) && !failed(&locked));
        let mut off = drive(DriveKind::Removable, Some("AB12"), false);
        apply(
            std::slice::from_mut(&mut off),
            &files("", "ENV{ID_FS_UUID}==\"AB12\", ENV{UDISKS_AUTO}=\"0\"\n"),
        );
        assert!(!shown(&off));
    }

    #[test]
    fn the_window_mounts_a_stick_turned_on_and_unmounts_what_udisks_mounted() {
        let stick = drive(DriveKind::Removable, Some("AB12"), false);
        assert_eq!(follow_up(&stick, true), FollowUp::Mount);
        assert_eq!(follow_up(&stick, false), FollowUp::Nothing);
        let mut locked = stick.clone();
        locked.locked = true;
        assert_eq!(follow_up(&locked, true), FollowUp::Nothing);
        let opened = drive(DriveKind::Internal, Some("01D9"), true);
        assert_eq!(follow_up(&opened, false), FollowUp::Unmount);
        // Under /mnt/better-os: the daemon's mount, which it unmounted.
        let mut ours = opened.clone();
        ours.mount_point = Some("/mnt/better-os/01D9".into());
        assert_eq!(follow_up(&ours, false), FollowUp::Nothing);
        assert_eq!(follow_up(&ours, true), FollowUp::Nothing);
        let system = drive(DriveKind::System, Some("AA"), true);
        assert_eq!(follow_up(&system, false), FollowUp::Nothing);
    }
}
